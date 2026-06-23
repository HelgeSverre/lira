//! Lira Fiber (Green Thread) Scheduler
//!
//! Implements cooperative multitasking for Lira's concurrency model.
//! See docs/lira/04-concurrency.md for the full specification.

use crate::value::{ChannelId, FiberId, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;

/// Events emitted by the scheduler for fiber/channel monitoring
#[derive(Debug, Clone)]
pub enum FiberEvent {
    /// A new fiber was spawned
    FiberSpawned { fiber_id: FiberId, ip: usize },
    /// A fiber's state changed
    FiberStateChanged {
        fiber_id: FiberId,
        old_state: String,
        new_state: String,
    },
    /// A new channel was created
    ChannelCreated {
        channel_id: ChannelId,
        capacity: usize,
    },
    /// A message was sent or received on a channel
    ChannelMessage {
        channel_id: ChannelId,
        operation: String,
        value: String,
    },
    /// A channel was closed
    ChannelClosed { channel_id: ChannelId },
}

/// Fiber state
#[derive(Debug, Clone, PartialEq)]
pub enum FiberState {
    /// Ready to run
    Ready,
    /// Currently running
    Running,
    /// Blocked waiting on a channel receive
    BlockedReceive(ChannelId),
    /// Blocked waiting on a channel send (for unbuffered/full channels)
    BlockedSend(ChannelId),
    /// Blocked on select
    BlockedSelect,
    /// Yielded voluntarily
    Yielded,
    /// Finished execution
    Finished,
    /// Terminated with error
    Failed(String),
}

/// Resolution of a parked `select` recorded by a waker.
///
/// When a fiber parks on a `select` (no ready arm), it registers as a waiter on
/// its channels. A waker (`channel_send`/`channel_receive`/`close_channel`) that
/// commits a parked select arm records the outcome here so the woken fiber's
/// re-run of the `Select` opcode commits to that exact arm instead of
/// re-polling (which would otherwise risk double send/recv).
#[derive(Debug, Clone)]
pub struct SelectResolution {
    /// The select channel id whose arm was committed.
    pub channel_id: ChannelId,
    /// For a committed recv arm, the value (and ok flag) received.
    /// `None` for a committed send arm.
    pub recv: Option<(Value, bool)>,
}

/// A fiber (green thread)
#[derive(Debug)]
pub struct Fiber {
    /// Fiber ID
    pub id: FiberId,
    /// Current state
    pub state: FiberState,
    /// Instruction pointer
    pub ip: usize,
    /// Operand stack
    pub stack: Vec<Value>,
    /// Local variables
    pub locals: Vec<Value>,
    /// Call stack for function calls
    pub call_stack: Vec<FiberCallFrame>,
    /// Fiber-local storage
    pub fiber_locals: HashMap<String, Value>,
    /// Result value when finished
    pub result: Option<Value>,
    /// Resolution recorded by a waker for a parked `select` (consumed on resume).
    pub select_resolution: Option<SelectResolution>,
    /// Channel ids this fiber registered on while parked in a `select`
    /// (every recv channel and every send channel). When a waker commits one
    /// arm, these are used to de-register the fiber from the *other* channels'
    /// `receivers`/`senders` queues so no phantom send/recv or stale reschedule
    /// can occur. Empty when the fiber is not parked on a select.
    pub select_channels: Vec<ChannelId>,
}

/// Call frame within a fiber
#[derive(Debug, Clone)]
pub struct FiberCallFrame {
    pub return_addr: usize,
    pub locals_base: usize,
}

impl Fiber {
    /// Create a new fiber starting at the given instruction pointer
    pub fn new(id: FiberId, ip: usize) -> Self {
        Self {
            id,
            state: FiberState::Ready,
            ip,
            stack: Vec::with_capacity(256),
            locals: Vec::with_capacity(64),
            call_stack: Vec::with_capacity(16),
            fiber_locals: HashMap::new(),
            result: None,
            select_resolution: None,
            select_channels: Vec::new(),
        }
    }

    /// Get the base index for local variables
    pub fn locals_base(&self) -> usize {
        self.call_stack.last().map(|f| f.locals_base).unwrap_or(0)
    }
}

/// Channel for inter-fiber communication
#[derive(Debug)]
pub struct Channel {
    /// Channel ID
    pub id: ChannelId,
    /// Buffer for messages (empty = unbuffered)
    pub buffer: VecDeque<Value>,
    /// Buffer capacity (0 = unbuffered/synchronous)
    pub capacity: usize,
    /// Fibers waiting to receive
    pub receivers: VecDeque<FiberId>,
    /// Fibers waiting to send (with their values)
    pub senders: VecDeque<(FiberId, Value)>,
    /// Is the channel closed?
    pub closed: bool,
}

impl Channel {
    /// Create a new channel with the given capacity
    pub fn new(id: ChannelId, capacity: usize) -> Self {
        Self {
            id,
            buffer: VecDeque::with_capacity(capacity),
            capacity,
            receivers: VecDeque::new(),
            senders: VecDeque::new(),
            closed: false,
        }
    }

    /// Check if the channel can accept a send without blocking
    pub fn can_send(&self) -> bool {
        if self.closed {
            return false;
        }
        if self.capacity == 0 {
            // Unbuffered: can send only if a receiver is waiting
            !self.receivers.is_empty()
        } else {
            // Buffered: can send if buffer has space
            self.buffer.len() < self.capacity
        }
    }

    /// Check if the channel has data to receive
    pub fn can_receive(&self) -> bool {
        !self.buffer.is_empty() || !self.senders.is_empty()
    }
}

/// The fiber scheduler
pub struct Scheduler {
    /// All fibers by ID
    pub fibers: HashMap<FiberId, Fiber>,
    /// Ready queue (fiber IDs)
    ready_queue: VecDeque<FiberId>,
    /// Currently running fiber ID
    pub current: Option<FiberId>,
    /// Next fiber ID
    next_fiber_id: FiberId,
    /// All channels by ID
    pub channels: HashMap<ChannelId, Channel>,
    /// Next channel ID
    next_channel_id: ChannelId,
    /// Time slice counter for preemption (instructions per slice)
    time_slice: usize,
    /// Default time slice
    default_time_slice: usize,
    /// Optional event sender for monitoring
    event_sender: Option<Sender<FiberEvent>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            fibers: HashMap::new(),
            ready_queue: VecDeque::new(),
            current: None,
            next_fiber_id: 1,
            channels: HashMap::new(),
            next_channel_id: 1,
            time_slice: 1000,
            default_time_slice: 1000,
            event_sender: None,
        }
    }

    /// Set the event sender for fiber/channel monitoring
    pub fn set_event_sender(&mut self, sender: Sender<FiberEvent>) {
        self.event_sender = Some(sender);
    }

    /// Emit an event if the event sender is set
    fn emit_event(&self, event: FiberEvent) {
        if let Some(ref sender) = self.event_sender {
            let _ = sender.send(event);
        }
    }

    /// Helper to format fiber state as string
    fn format_state(state: &FiberState) -> String {
        match state {
            FiberState::Ready => "Ready".to_string(),
            FiberState::Running => "Running".to_string(),
            FiberState::BlockedReceive(ch) => format!("BlockedReceive({})", ch),
            FiberState::BlockedSend(ch) => format!("BlockedSend({})", ch),
            FiberState::BlockedSelect => "BlockedSelect".to_string(),
            FiberState::Yielded => "Yielded".to_string(),
            FiberState::Finished => "Finished".to_string(),
            FiberState::Failed(msg) => format!("Failed({})", msg),
        }
    }

    /// Set the time slice (instructions per fiber before yielding)
    pub fn set_time_slice(&mut self, slice: usize) {
        self.default_time_slice = slice;
        self.time_slice = slice;
    }

    /// Spawn a new fiber starting at the given instruction pointer
    pub fn spawn(&mut self, ip: usize) -> FiberId {
        let id = self.next_fiber_id;
        self.next_fiber_id += 1;

        let fiber = Fiber::new(id, ip);
        self.fibers.insert(id, fiber);
        self.ready_queue.push_back(id);

        // Emit fiber spawned event
        self.emit_event(FiberEvent::FiberSpawned { fiber_id: id, ip });

        id
    }

    /// Spawn a fiber with initial arguments bound as locals.
    ///
    /// Compiled functions read their parameters as locals (slot 0, 1, ...),
    /// so spawn arguments are placed directly into `locals` rather than the
    /// operand stack. The spawned fiber begins at the function body with its
    /// parameters already bound.
    pub fn spawn_with_args(&mut self, ip: usize, args: Vec<Value>) -> FiberId {
        let id = self.spawn(ip);
        if let Some(fiber) = self.fibers.get_mut(&id) {
            fiber.locals = args;
        }
        id
    }

    /// Get the currently running fiber
    pub fn current_fiber(&self) -> Option<&Fiber> {
        self.current.and_then(|id| self.fibers.get(&id))
    }

    /// Get the currently running fiber mutably
    pub fn current_fiber_mut(&mut self) -> Option<&mut Fiber> {
        let id = self.current?;
        self.fibers.get_mut(&id)
    }

    /// Get a fiber by ID
    pub fn get_fiber(&self, id: FiberId) -> Option<&Fiber> {
        self.fibers.get(&id)
    }

    /// Get a fiber by ID mutably
    pub fn get_fiber_mut(&mut self, id: FiberId) -> Option<&mut Fiber> {
        self.fibers.get_mut(&id)
    }

    /// Yield the current fiber voluntarily
    pub fn yield_current(&mut self) {
        if let Some(current_id) = self.current {
            if let Some(fiber) = self.fibers.get_mut(&current_id) {
                if fiber.state == FiberState::Running {
                    let old_state = Self::format_state(&fiber.state);
                    fiber.state = FiberState::Yielded;
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id: current_id,
                        old_state,
                        new_state: "Yielded".to_string(),
                    });
                    self.ready_queue.push_back(current_id);
                }
            }
            self.current = None;
        }
        self.time_slice = self.default_time_slice;
    }

    /// Tick the time slice counter, returns true if should yield
    pub fn tick(&mut self) -> bool {
        if self.time_slice > 0 {
            self.time_slice -= 1;
            false
        } else {
            true
        }
    }

    /// Schedule the next fiber to run
    pub fn schedule(&mut self) -> Option<FiberId> {
        // Reset time slice
        self.time_slice = self.default_time_slice;

        // Find next ready fiber
        while let Some(id) = self.ready_queue.pop_front() {
            if let Some(fiber) = self.fibers.get_mut(&id) {
                match fiber.state {
                    FiberState::Ready | FiberState::Yielded => {
                        let old_state = Self::format_state(&fiber.state);
                        fiber.state = FiberState::Running;
                        self.emit_event(FiberEvent::FiberStateChanged {
                            fiber_id: id,
                            old_state,
                            new_state: "Running".to_string(),
                        });
                        self.current = Some(id);
                        return Some(id);
                    }
                    _ => continue,
                }
            }
        }

        self.current = None;
        None
    }

    /// Mark the current fiber as finished
    pub fn finish_current(&mut self, result: Value) {
        if let Some(current_id) = self.current {
            if let Some(fiber) = self.fibers.get_mut(&current_id) {
                let old_state = Self::format_state(&fiber.state);
                fiber.state = FiberState::Finished;
                fiber.result = Some(result);
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: current_id,
                    old_state,
                    new_state: "Finished".to_string(),
                });
            }
            self.current = None;
        }
    }

    /// Mark the current fiber as failed
    pub fn fail_current(&mut self, error: String) {
        if let Some(current_id) = self.current {
            if let Some(fiber) = self.fibers.get_mut(&current_id) {
                let old_state = Self::format_state(&fiber.state);
                fiber.state = FiberState::Failed(error.clone());
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: current_id,
                    old_state,
                    new_state: format!("Failed({})", error),
                });
            }
            self.current = None;
        }
    }

    /// Check if there are any runnable fibers
    pub fn has_runnable(&self) -> bool {
        !self.ready_queue.is_empty() || self.current.is_some()
    }

    /// Check if all fibers are finished or blocked
    pub fn is_deadlocked(&self) -> bool {
        if self.fibers.is_empty() {
            return false;
        }

        for fiber in self.fibers.values() {
            match fiber.state {
                FiberState::Ready | FiberState::Running | FiberState::Yielded => {
                    return false;
                }
                FiberState::Finished | FiberState::Failed(_) => continue,
                _ => continue, // Blocked fibers
            }
        }

        // All fibers are either finished or blocked
        self.ready_queue.is_empty()
    }

    /// Get the number of active (non-finished) fibers
    pub fn active_count(&self) -> usize {
        self.fibers
            .values()
            .filter(|f| !matches!(f.state, FiberState::Finished | FiberState::Failed(_)))
            .count()
    }

    // Channel operations

    /// Create a new channel with the given capacity (0 = unbuffered)
    pub fn create_channel(&mut self, capacity: usize) -> ChannelId {
        let id = self.next_channel_id;
        self.next_channel_id += 1;

        let channel = Channel::new(id, capacity);
        self.channels.insert(id, channel);

        // Emit channel created event
        self.emit_event(FiberEvent::ChannelCreated {
            channel_id: id,
            capacity,
        });

        id
    }

    /// Send a value on a channel, returns true if sent immediately, false if blocked
    pub fn channel_send(&mut self, channel_id: ChannelId, value: Value) -> Result<bool, String> {
        let current_id = self.current.ok_or("No current fiber")?;
        let value_str = format!("{:?}", value);

        let channel = self
            .channels
            .get_mut(&channel_id)
            .ok_or("Invalid channel")?;

        if channel.closed {
            return Err("Cannot send on closed channel".to_string());
        }

        // Check if there's a waiting receiver. Select-waiters (BlockedSelect)
        // must NOT receive the value directly onto their stack: they re-run the
        // `Select` opcode on wake and pull the value via `try_select`. So we
        // pick the first *blocking* receiver to hand off to, while waking any
        // select-waiters we skip past so they re-evaluate against this send.
        let mut woken_selectors: Vec<FiberId> = Vec::new();
        let mut direct_receiver: Option<FiberId> = None;
        while let Some(receiver_id) = channel.receivers.pop_front() {
            let is_select = matches!(
                self.fibers.get(&receiver_id).map(|f| &f.state),
                Some(FiberState::BlockedSelect)
            );
            if is_select {
                woken_selectors.push(receiver_id);
            } else {
                direct_receiver = Some(receiver_id);
                break;
            }
        }

        if let Some(receiver_id) = direct_receiver {
            // Direct handoff to a blocking receiver.
            if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                receiver.stack.push(value);
                receiver.stack.push(Value::Bool(true)); // ok = true
                let old_state = Self::format_state(&receiver.state);
                receiver.state = FiberState::Ready;
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: receiver_id,
                    old_state,
                    new_state: "Ready".to_string(),
                });
                self.ready_queue.push_back(receiver_id);
            }
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "send".to_string(),
                value: value_str,
            });
            // Re-queue any select-waiters we skipped so they stay registered.
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                for id in woken_selectors {
                    channel.receivers.push_front(id);
                }
            }
            return Ok(true);
        }

        // No blocking receiver: if a select-waiter is parked here, park this
        // send in `senders` and wake the select-waiters so they re-evaluate and
        // pull the value via `try_select`.
        if !woken_selectors.is_empty() {
            if let Some(fiber) = self.fibers.get_mut(&current_id) {
                let old_state = Self::format_state(&fiber.state);
                fiber.state = FiberState::BlockedSend(channel_id);
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: current_id,
                    old_state,
                    new_state: format!("BlockedSend({})", channel_id),
                });
            }
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                channel.senders.push_back((current_id, value));
            }
            for receiver_id in woken_selectors {
                if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                    let old_state = Self::format_state(&receiver.state);
                    receiver.state = FiberState::Ready;
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id: receiver_id,
                        old_state,
                        new_state: "Ready".to_string(),
                    });
                    self.ready_queue.push_back(receiver_id);
                }
            }
            self.current = None;
            return Ok(false);
        }

        // Check if we can buffer
        if channel.capacity > 0 && channel.buffer.len() < channel.capacity {
            channel.buffer.push_back(value);
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "send".to_string(),
                value: value_str,
            });
            return Ok(true);
        }

        // Need to block
        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            let old_state = Self::format_state(&fiber.state);
            fiber.state = FiberState::BlockedSend(channel_id);
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id: current_id,
                old_state,
                new_state: format!("BlockedSend({})", channel_id),
            });
        }

        // Re-get channel as mutable (borrow checker)
        if let Some(channel) = self.channels.get_mut(&channel_id) {
            channel.senders.push_back((current_id, value));
        }

        self.current = None;
        Ok(false)
    }

    /// Receive a value from a channel, returns Some(value) if received, None if blocked
    pub fn channel_receive(
        &mut self,
        channel_id: ChannelId,
    ) -> Result<Option<(Value, bool)>, String> {
        let current_id = self.current.ok_or("No current fiber")?;

        let channel = self
            .channels
            .get_mut(&channel_id)
            .ok_or("Invalid channel")?;

        // Check buffer first
        if let Some(value) = channel.buffer.pop_front() {
            let value_str = format!("{:?}", value);
            // Wake up a blocked sender if any
            if let Some((sender_id, sender_value)) = channel.senders.pop_front() {
                channel.buffer.push_back(sender_value);
                if let Some(sender) = self.fibers.get_mut(&sender_id) {
                    if matches!(sender.state, FiberState::BlockedSelect) {
                        sender.select_resolution = Some(SelectResolution {
                            channel_id,
                            recv: None,
                        });
                    }
                    let old_state = Self::format_state(&sender.state);
                    sender.state = FiberState::Ready;
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id: sender_id,
                        old_state,
                        new_state: "Ready".to_string(),
                    });
                    self.ready_queue.push_back(sender_id);
                }
            }
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "receive".to_string(),
                value: value_str,
            });
            return Ok(Some((value, true)));
        }

        // Check for waiting sender (unbuffered handoff)
        if let Some((sender_id, value)) = channel.senders.pop_front() {
            let value_str = format!("{:?}", value);
            if let Some(sender) = self.fibers.get_mut(&sender_id) {
                // If the sender is a select-waiter, record that its send arm on
                // this channel was committed so its `Select` re-run does NOT
                // send again.
                if matches!(sender.state, FiberState::BlockedSelect) {
                    sender.select_resolution = Some(SelectResolution {
                        channel_id,
                        recv: None,
                    });
                }
                let old_state = Self::format_state(&sender.state);
                sender.state = FiberState::Ready;
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: sender_id,
                    old_state,
                    new_state: "Ready".to_string(),
                });
                self.ready_queue.push_back(sender_id);
            }
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "receive".to_string(),
                value: value_str,
            });
            return Ok(Some((value, true)));
        }

        // Channel is empty
        if channel.closed {
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "receive".to_string(),
                value: "null (closed)".to_string(),
            });
            return Ok(Some((Value::Null, false))); // ok = false means closed
        }

        // Need to block
        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            let old_state = Self::format_state(&fiber.state);
            fiber.state = FiberState::BlockedReceive(channel_id);
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id: current_id,
                old_state,
                new_state: format!("BlockedReceive({})", channel_id),
            });
        }

        // Re-get channel
        if let Some(channel) = self.channels.get_mut(&channel_id) {
            channel.receivers.push_back(current_id);
        }

        self.current = None;
        Ok(None)
    }

    /// Close a channel
    pub fn close_channel(&mut self, channel_id: ChannelId) -> Result<(), String> {
        // First, collect the receiver and sender IDs from the channel
        let (receiver_ids, sender_ids) = {
            let channel = self
                .channels
                .get_mut(&channel_id)
                .ok_or("Invalid channel")?;

            channel.closed = true;

            // Collect IDs to process after releasing channel borrow
            let receivers: Vec<FiberId> = channel.receivers.drain(..).collect();
            let senders: Vec<(FiberId, Value)> = channel.senders.drain(..).collect();
            (receivers, senders)
        };

        // Emit channel closed event
        self.emit_event(FiberEvent::ChannelClosed { channel_id });

        // Wake all blocked receivers with (null, false). Select-waiters
        // (BlockedSelect) must NOT get the pair pushed onto their stack: they
        // re-run `Select`, and `try_select` reports the closed channel as ready.
        for receiver_id in receiver_ids {
            if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                if !matches!(receiver.state, FiberState::BlockedSelect) {
                    receiver.stack.push(Value::Null);
                    receiver.stack.push(Value::Bool(false)); // ok = false
                }
                let old_state = Self::format_state(&receiver.state);
                receiver.state = FiberState::Ready;
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: receiver_id,
                    old_state,
                    new_state: "Ready".to_string(),
                });
                self.ready_queue.push_back(receiver_id);
            }
        }

        // Wake all blocked senders with error
        for (sender_id, _) in sender_ids {
            if let Some(sender) = self.fibers.get_mut(&sender_id) {
                let old_state = Self::format_state(&sender.state);
                sender.state = FiberState::Failed("send on closed channel".to_string());
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: sender_id,
                    old_state,
                    new_state: "Failed(send on closed channel)".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Try to receive from any of the given channels (select).
    ///
    /// Returns `Some((index, value, ok))` for the first ready channel, where
    /// `index` is the position in `channel_ids`. A closed channel is treated as
    /// immediately ready and yields `(index, Null, false)`, matching Go's
    /// semantics where a recv on a closed channel does not block. A waiting
    /// unbuffered sender is woken on handoff.
    pub fn try_select(&mut self, channel_ids: &[ChannelId]) -> Option<(usize, Value, bool)> {
        for (index, &channel_id) in channel_ids.iter().enumerate() {
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                // Check buffer
                if let Some(value) = channel.buffer.pop_front() {
                    return Some((index, value, true));
                }
                // Check waiting senders (unbuffered handoff).
                if let Some((sender_id, value)) = channel.senders.pop_front() {
                    if let Some(sender) = self.fibers.get_mut(&sender_id) {
                        // A select send-waiter must record that its send arm on
                        // this channel committed, so its `Select` re-run does not
                        // send the value again.
                        if matches!(sender.state, FiberState::BlockedSelect) {
                            sender.select_resolution = Some(SelectResolution {
                                channel_id,
                                recv: None,
                            });
                        }
                        sender.state = FiberState::Ready;
                        self.ready_queue.push_back(sender_id);
                    }
                    return Some((index, value, true));
                }
                // A closed channel makes its recv arm immediately ready.
                if channel.closed {
                    return Some((index, Value::Null, false));
                }
            }
        }
        None
    }

    /// Try to send a value on a channel without blocking (select send arm).
    ///
    /// Returns `true` if the value was handed to a waiting receiver or buffered,
    /// `false` if the send would block (no receiver, full/unbuffered). Never
    /// parks the current fiber.
    pub fn try_select_send(&mut self, channel_id: ChannelId, value: Value) -> bool {
        let channel = match self.channels.get_mut(&channel_id) {
            Some(ch) => ch,
            None => return false,
        };

        if channel.closed {
            return false;
        }

        // Direct handoff to a waiting receiver.
        if let Some(receiver_id) = channel.receivers.pop_front() {
            let value_str = format!("{:?}", value);
            let is_select = matches!(
                self.fibers.get(&receiver_id).map(|f| &f.state),
                Some(FiberState::BlockedSelect)
            );
            if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                if is_select {
                    // A select recv-waiter: record the committed recv arm so its
                    // `Select` re-run picks up this value instead of re-polling.
                    receiver.select_resolution = Some(SelectResolution {
                        channel_id,
                        recv: Some((value, true)),
                    });
                } else {
                    receiver.stack.push(value);
                    receiver.stack.push(Value::Bool(true)); // ok = true
                }
                let old_state = Self::format_state(&receiver.state);
                receiver.state = FiberState::Ready;
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: receiver_id,
                    old_state,
                    new_state: "Ready".to_string(),
                });
                self.ready_queue.push_back(receiver_id);
            }
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "send".to_string(),
                value: value_str,
            });
            return true;
        }

        // Buffer if there is room.
        if channel.capacity > 0 && channel.buffer.len() < channel.capacity {
            let value_str = format!("{:?}", value);
            channel.buffer.push_back(value);
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "send".to_string(),
                value: value_str,
            });
            return true;
        }

        false
    }

    /// Park the current fiber on a `select` with no ready arm.
    ///
    /// The fiber is registered as a waiter on every recv channel's `receivers`
    /// queue and every send channel's `senders` queue (with the send value), so
    /// the existing channel wake paths (`channel_send`/`channel_receive`/
    /// `close_channel`/`try_select`) move it back to `Ready`. On resume it
    /// re-executes the `Select` opcode and re-evaluates its arms; because that
    /// re-evaluation re-checks readiness, a spurious wake simply re-parks. The
    /// fiber is marked `BlockedSelect` (treated as blocked by deadlock
    /// detection) and `current` is cleared so the run loop reschedules.
    pub fn park_select(&mut self, recv_ids: &[ChannelId], send_specs: &[(ChannelId, Value)]) {
        let current_id = match self.current {
            Some(id) => id,
            None => return,
        };

        // Record every channel the fiber registers on so the commit paths can
        // de-register it from the losing arms.
        let mut registered: Vec<ChannelId> = Vec::with_capacity(recv_ids.len() + send_specs.len());
        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            let old_state = Self::format_state(&fiber.state);
            fiber.state = FiberState::BlockedSelect;
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id: current_id,
                old_state,
                new_state: "BlockedSelect".to_string(),
            });
        }

        for &channel_id in recv_ids {
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                channel.receivers.push_back(current_id);
                registered.push(channel_id);
            }
        }
        for (channel_id, value) in send_specs {
            if let Some(channel) = self.channels.get_mut(channel_id) {
                channel.senders.push_back((current_id, value.clone()));
                registered.push(*channel_id);
            }
        }

        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            fiber.select_channels = registered;
        }

        self.current = None;
    }

    /// Remove every parked-`select` registration left by `fiber_id`.
    ///
    /// When a parked select-waiter commits to one arm, it is still registered
    /// on the *other* channels it offered (as a receiver and/or a parked
    /// sender). Those stale registrations would otherwise cause a phantom send
    /// (its abandoned send value delivered to a later receiver) or a stale
    /// reschedule (a later send waking the already-committed/finished fiber and
    /// re-running its `Select` against a corrupted stack). This purges the
    /// fiber from all channels it registered on so only its chosen arm commits.
    pub fn deregister_select_waiter(&mut self, fiber_id: FiberId) {
        let channels = match self.fibers.get_mut(&fiber_id) {
            Some(fiber) if !fiber.select_channels.is_empty() => {
                std::mem::take(&mut fiber.select_channels)
            }
            _ => return,
        };
        for channel_id in channels {
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                channel.receivers.retain(|&id| id != fiber_id);
                channel.senders.retain(|(id, _)| *id != fiber_id);
            }
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for a channel reference that can be stored as a Value
#[derive(Debug, Clone)]
pub struct ChannelRef {
    pub id: ChannelId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_fiber() {
        let mut sched = Scheduler::new();
        let id = sched.spawn(100);
        assert_eq!(id, 1);
        assert!(sched.get_fiber(id).is_some());
    }

    #[test]
    fn test_schedule_fibers() {
        let mut sched = Scheduler::new();
        let id1 = sched.spawn(100);
        let id2 = sched.spawn(200);

        // Schedule first fiber
        let next = sched.schedule();
        assert_eq!(next, Some(id1));

        // Yield and schedule second
        sched.yield_current();
        let next = sched.schedule();
        assert_eq!(next, Some(id2));
    }

    #[test]
    fn test_buffered_channel() {
        let mut sched = Scheduler::new();
        let _fiber = sched.spawn(0);
        sched.schedule();

        let ch = sched.create_channel(2);

        // Send should succeed (buffered)
        let result = sched.channel_send(ch, Value::Int(42));
        assert!(result.is_ok());
        assert!(result.unwrap()); // Should return true (sent immediately)
    }

    #[test]
    fn test_channel_close() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(1);

        let result = sched.close_channel(ch);
        assert!(result.is_ok());

        // Channel should be closed
        assert!(sched.channels.get(&ch).unwrap().closed);
    }
}
