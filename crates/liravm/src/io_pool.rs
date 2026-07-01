//! I/O thread pool for offloading blocking syscalls off the single VM thread.
//!
//! Fibers are cooperatively scheduled on one OS thread, so a blocking syscall
//! (HTTP request, disk read, `sleep`, TCP recv, ...) would otherwise stall
//! every other fiber for its whole duration. Instead, a blocking syscall is
//! packaged as an [`IoJob`] — a boxed `Send` closure that owns everything it
//! needs — handed to a pool of worker threads, and the calling fiber is parked
//! (`FiberState::BlockedIo`). Workers run jobs in parallel and return plain,
//! `Send` [`IoValue`]s; the VM thread harvests them, rebuilds the `Value`, and
//! wakes the fiber.
//!
//! Invariant: only owned data crosses the thread boundary — never a `Gc`/`Rc`
//! handle or any VM state. The `+ Send` bound on the job closure enforces this
//! at compile time (an accidentally-captured `Rc`/`Gc` fails to compile). The
//! result `Value` is always reconstructed on the VM thread in `deliver_io`.
//!
//! Stateful handles (`File`/`TcpStream`, both `Send`) are *checked out* of the
//! runtime registry on the VM thread, moved into the job, operated on the pool
//! thread, and ride back inside the [`IoValue`] so the VM thread re-inserts
//! them — so the registry stays single-threaded and lock-free.

use crate::value::FiberId;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

/// Worker thread count — the ceiling on concurrent in-flight blocking calls.
/// I/O-bound, so independent of CPU count.
const POOL_SIZE: usize = 32;

/// Plain, `Send` result data produced on a pool thread; the VM thread converts
/// it to a `Value`. Handle-carrying variants ferry a checked-out `File`/
/// `TcpStream` back so the VM thread can re-insert it into the runtime registry.
pub enum IoValue {
    /// No value (`sleep`) → `Value::Null`.
    Unit,
    Int(i64),
    Str(String),
    Bool(bool),
    /// A list of strings (`listdir`) → `Value::Array` of strings.
    Strs(Vec<String>),
    /// An HTTP response → `[status, body]`.
    HttpResponse { status: i64, body: String },

    /// A freshly opened file: insert `file` at `fd`, then yield `Int(fd)`.
    FileOpened { fd: i64, file: std::fs::File },
    /// An op on a checked-out file: re-insert `file` at `fd`, then yield `result`.
    FileOp {
        fd: i64,
        file: std::fs::File,
        result: Box<IoValue>,
    },
    /// A freshly connected socket: insert `stream` at `id`, then yield `Int(id)`.
    TcpConnected { id: i64, stream: std::net::TcpStream },
    /// An op on a checked-out socket: re-insert `stream` at `id`, then `result`.
    TcpOp {
        id: i64,
        stream: std::net::TcpStream,
        result: Box<IoValue>,
    },
}

/// A blocking task to run off the VM thread, tagged with the fiber to wake.
pub struct IoJob {
    pub fiber: FiberId,
    pub run: Box<dyn FnOnce() -> Result<IoValue, String> + Send + 'static>,
}

impl IoJob {
    pub fn new<F>(fiber: FiberId, f: F) -> Self
    where
        F: FnOnce() -> Result<IoValue, String> + Send + 'static,
    {
        IoJob {
            fiber,
            run: Box::new(f),
        }
    }
}

/// The finished result of an [`IoJob`], routed back to its fiber.
pub struct IoCompletion {
    pub fiber: FiberId,
    pub outcome: Result<IoValue, String>,
}

fn run_job(job: IoJob) -> IoCompletion {
    let fiber = job.fiber;
    let run = job.run;
    // A panic inside an arbitrary job closure must not kill the worker (which
    // would silently shrink the pool) or strand the parked fiber. Convert it
    // to an error the VM thread can surface via the syscall's error contract.
    let outcome = match catch_unwind(AssertUnwindSafe(move || run())) {
        Ok(result) => result,
        Err(_) => Err("I/O task panicked".to_string()),
    };
    IoCompletion { fiber, outcome }
}

/// A fixed pool of worker threads plus the channels to feed them jobs and
/// collect completions. Created lazily on first use, so programs that never do
/// blocking I/O spawn no threads.
pub struct IoPool {
    job_tx: Sender<IoJob>,
    completion_rx: Receiver<IoCompletion>,
    /// Jobs submitted but not yet harvested; drives the scheduler's decision to
    /// wait for I/O vs. declare a deadlock / finish.
    pending: usize,
}

impl IoPool {
    pub fn new() -> Self {
        let (job_tx, job_rx) = channel::<IoJob>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let (comp_tx, completion_rx) = channel::<IoCompletion>();

        for _ in 0..POOL_SIZE {
            let job_rx = Arc::clone(&job_rx);
            let comp_tx = comp_tx.clone();
            thread::spawn(move || loop {
                // Hold the lock only to dequeue; the blocking work runs outside
                // it so all workers can be in-flight simultaneously.
                let job = {
                    let guard = match job_rx.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    match guard.recv() {
                        Ok(job) => job,
                        Err(_) => break, // pool dropped
                    }
                };
                if comp_tx.send(run_job(job)).is_err() {
                    break; // VM gone
                }
            });
        }

        IoPool {
            job_tx,
            completion_rx,
            pending: 0,
        }
    }

    /// Submit a job. On success the pending count is bumped. On failure (the
    /// pool's workers are all gone) the job is handed back so the caller can run
    /// it inline rather than parking a fiber that would never be woken.
    pub fn submit(&mut self, job: IoJob) -> Result<(), IoJob> {
        match self.job_tx.send(job) {
            Ok(()) => {
                self.pending += 1;
                Ok(())
            }
            Err(err) => Err(err.0),
        }
    }

    pub fn pending(&self) -> usize {
        self.pending
    }

    /// Harvest every already-completed job without blocking.
    pub fn drain_completed(&mut self) -> Vec<IoCompletion> {
        let mut out = Vec::new();
        loop {
            match self.completion_rx.try_recv() {
                Ok(c) => {
                    self.pending = self.pending.saturating_sub(1);
                    out.push(c);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// Block until one job completes. Called only when the VM thread has nothing
    /// else to run but I/O is still in flight.
    pub fn wait_one(&mut self) -> Option<IoCompletion> {
        match self.completion_rx.recv() {
            Ok(c) => {
                self.pending = self.pending.saturating_sub(1);
                Some(c)
            }
            Err(_) => None,
        }
    }
}

impl Default for IoPool {
    fn default() -> Self {
        Self::new()
    }
}
