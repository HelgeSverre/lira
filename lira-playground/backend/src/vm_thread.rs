//! Dedicated VM Thread
//!
//! Spawns a dedicated std::thread per WebSocket session that owns the non-Send VM.
//! Communication happens via tokio::sync::mpsc channels, allowing the async WebSocket
//! handler to interact with the synchronous VM execution.

use std::panic::AssertUnwindSafe;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use liravm::DebugSession;
use tokio::sync::{mpsc, oneshot};

use crate::protocol::{ClientMessage, ServerMessage};
use crate::session_handlers::handle_client_message;

/// Channel buffer size for commands
const CHANNEL_BUFFER_SIZE: usize = 32;

/// Default timeout for command responses (30 seconds)
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Commands sent from WebSocket handler to VM thread
pub enum VmCommand {
    /// Execute a client message and get response
    Execute {
        message: ClientMessage,
        response_tx: oneshot::Sender<VmResponse>,
    },
    /// Shutdown the VM thread
    Shutdown,
}

/// Response from VM thread
pub struct VmResponse {
    /// Server messages to send back to client
    pub messages: Vec<ServerMessage>,
    /// Whether the WebSocket connection should terminate
    pub terminate: bool,
}

/// Errors that can occur when communicating with VM thread
#[derive(Debug)]
pub enum VmError {
    /// VM thread died unexpectedly
    ThreadDied,
    /// Command timed out
    Timeout,
    /// Response channel was closed
    ResponseChannelClosed,
    /// Channel is full (backpressure)
    #[allow(dead_code)]
    ChannelFull,
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::ThreadDied => write!(f, "VM thread died unexpectedly"),
            VmError::Timeout => write!(f, "VM command timed out"),
            VmError::ResponseChannelClosed => write!(f, "Response channel closed"),
            VmError::ChannelFull => write!(f, "Command channel full"),
        }
    }
}

impl std::error::Error for VmError {}

/// Handle to communicate with the VM thread
pub struct VmThreadHandle {
    /// Channel to send commands to the VM thread
    command_tx: mpsc::Sender<VmCommand>,
    /// Thread join handle (used for cleanup)
    join_handle: Option<JoinHandle<()>>,
}

impl VmThreadHandle {
    /// Spawn a new VM thread and return a handle to communicate with it
    pub fn spawn() -> Self {
        let (command_tx, command_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

        let join_handle = thread::spawn(move || {
            vm_thread_main(command_rx);
        });

        Self {
            command_tx,
            join_handle: Some(join_handle),
        }
    }

    /// Send a command to the VM thread and wait for response
    pub async fn send_command(&self, message: ClientMessage) -> Result<VmResponse, VmError> {
        let (response_tx, response_rx) = oneshot::channel();

        // Send command to VM thread
        self.command_tx
            .send(VmCommand::Execute {
                message,
                response_tx,
            })
            .await
            .map_err(|_| VmError::ThreadDied)?;

        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(VmError::ResponseChannelClosed),
            Err(_) => Err(VmError::Timeout),
        }
    }

    /// Shutdown the VM thread gracefully
    pub async fn shutdown(&mut self) {
        // Send shutdown command (ignore errors if thread is already dead)
        let _ = self.command_tx.send(VmCommand::Shutdown).await;

        // Wait for thread to finish
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for VmThreadHandle {
    fn drop(&mut self) {
        // Best-effort shutdown - use try_send since we can't await in Drop
        let _ = self.command_tx.try_send(VmCommand::Shutdown);

        // Don't block in Drop, let the thread finish on its own
        // The thread will exit when it receives Shutdown or when the channel is dropped
    }
}

/// Main loop for the VM thread
fn vm_thread_main(mut command_rx: mpsc::Receiver<VmCommand>) {
    // Create a single-threaded tokio runtime for channel operations
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime in VM thread");

    // Create the debug session (owns the non-Send VM)
    let mut session = DebugSession::new();

    rt.block_on(async {
        // Process commands until shutdown or channel close
        while let Some(command) = command_rx.recv().await {
            match command {
                VmCommand::Execute {
                    message,
                    response_tx,
                } => {
                    // Handle the message with panic recovery
                    let response = match std::panic::catch_unwind(AssertUnwindSafe(|| {
                        handle_client_message(&mut session, message)
                    })) {
                        Ok(response) => response,
                        Err(panic_info) => {
                            // Extract panic message
                            let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                                s.to_string()
                            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                                s.clone()
                            } else {
                                "Unknown panic".to_string()
                            };

                            tracing::error!("VM panic: {}", panic_msg);

                            // Reset session after panic
                            session = DebugSession::new();

                            VmResponse {
                                messages: vec![ServerMessage::RuntimeError {
                                    message: format!("Internal VM error: {}", panic_msg),
                                    location: None,
                                }],
                                terminate: false,
                            }
                        }
                    };

                    // Send response (ignore error if receiver dropped)
                    let _ = response_tx.send(response);
                }
                VmCommand::Shutdown => {
                    // Shutdown requested
                    break;
                }
            }
        }
    });
}
