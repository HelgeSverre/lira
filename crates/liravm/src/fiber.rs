//! Lira Fiber (Green Thread) Scheduler
//!
//! Implements cooperative multitasking for Lira's concurrency model.
//! See docs/lira/04-concurrency.md for the full specification.

use crate::value::{ChannelId, FiberId, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;

const MAX_CHANNEL_ALLOCATION_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_CHANNEL_CAPACITY: usize =
    MAX_CHANNEL_ALLOCATION_BYTES / std::mem::size_of::<Value>();

/// Events emitted by the scheduler for fiber/channel monitoring
#[derive(Debug, Clone)]
pub enum FiberEvent {
    /// A new fiber was spawned
    FiberSpawned { fiber_id: FiberId, ip: usize },
    /// A fiber's state changed (`new_state` is a human display label for the
    /// timeline; the typed state is delivered separately via the snapshot).
    FiberStateChanged {
        fiber_id: FiberId,
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
    /// Blocked waiting on an offloaded blocking syscall (e.g. an HTTP request
    /// running on the I/O thread pool). Woken when the pool delivers the result.
    BlockedIo,
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
/// commits a parked select arm records the communication endpoint here so the
/// woken fiber's re-run of the `Select` opcode can consume the already-completed
/// operation instead of re-polling (which would otherwise risk double
/// send/recv). Ambiguous duplicate offers are deferred until the parked fiber
/// arbitrates them itself, so this endpoint resolution is exact for the value
/// and body that ran.
#[derive(Debug, Clone)]
pub struct SelectResolution {
    /// The select channel id whose arm was committed.
    pub channel_id: ChannelId,
    /// For a committed recv arm, the value (and ok flag) received.
    /// `None` for a committed send arm.
    pub recv: Option<(Value, bool)>,
}

#[derive(Debug, Clone, Copy)]
struct SelectOffer {
    channel_id: ChannelId,
    is_send: bool,
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
    /// Instruction offset that caused a failure, when the scheduler fails a
    /// parked fiber without the VM's live instruction context.
    pub failure_ip: Option<usize>,
    /// Channel ids this fiber registered on while parked in a `select`
    /// (every recv channel and every send channel). When a waker commits one
    /// arm, these are used to de-register the fiber from the *other* channels'
    /// `receivers`/`senders` queues so no phantom send/recv or stale reschedule
    /// can occur. Empty when the fiber is not parked on a select.
    pub select_channels: Vec<ChannelId>,
    /// Exact communication offers registered by this parked select. Keeping
    /// direction as well as channel id lets two interacting selects defer a
    /// rendezvous until the parked selector has arbitrated its other arms.
    select_parked: bool,
    select_offers: Vec<SelectOffer>,
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
            failure_ip: None,
            select_channels: Vec::new(),
            select_parked: false,
            select_offers: Vec::new(),
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
    pub fn new(id: ChannelId, capacity: usize) -> Result<Self, String> {
        if capacity > MAX_CHANNEL_CAPACITY {
            return Err(format!(
                "Channel capacity exceeds VM limit of {MAX_CHANNEL_CAPACITY} elements"
            ));
        }
        let mut buffer = VecDeque::new();
        buffer.try_reserve_exact(capacity).map_err(|_| {
            format!("Channel allocation failed while reserving {capacity} elements")
        })?;
        Ok(Self {
            id,
            buffer,
            capacity,
            receivers: VecDeque::new(),
            senders: VecDeque::new(),
            closed: false,
        })
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

    /// Check whether a receive arm completes immediately. A closed channel is
    /// ready even when it has no buffered value.
    pub fn can_select_receive(&self) -> bool {
        self.can_receive() || self.closed
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
    /// First unhandled fiber failure, in deterministic scheduler order.
    ///
    /// Failures are retained until the VM observes them so a child that is
    /// awakened by a channel close cannot disappear while the root fiber
    /// continues to run to completion.
    first_failure: Option<(FiberId, String)>,
}

impl Scheduler {
    fn is_parked_select(fiber: &Fiber) -> bool {
        matches!(fiber.state, FiberState::BlockedSelect)
            || (matches!(fiber.state, FiberState::Ready) && fiber.select_parked)
    }

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
            first_failure: None,
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

    /// Snapshot of the ready-queue fiber ids, front (next to run) first.
    ///
    /// The queue itself is private (mutated by scheduling); this read-only copy
    /// lets debuggers/visualizers show pending fibers without exposing the
    /// internal `VecDeque`.
    pub fn ready_queue_ids(&self) -> Vec<FiberId> {
        self.ready_queue.iter().copied().collect()
    }

    /// Yield the current fiber voluntarily
    pub fn yield_current(&mut self) {
        if let Some(current_id) = self.current {
            if let Some(fiber) = self.fibers.get_mut(&current_id) {
                if fiber.state == FiberState::Running {
                    fiber.state = FiberState::Yielded;
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id: current_id,
                        new_state: "Yielded".to_string(),
                    });
                    self.ready_queue.push_back(current_id);
                }
            }
            self.current = None;
        }
        self.time_slice = self.default_time_slice;
    }

    /// Park the current fiber on an offloaded blocking syscall. The caller has
    /// already `save_fiber_state`d it and submitted the job to the I/O pool;
    /// clearing `current` hands control back to the outer scheduler loop, which
    /// runs other fibers (or waits on the pool) until this one is woken.
    pub fn block_current_on_io(&mut self) {
        if let Some(current_id) = self.current.take() {
            if let Some(fiber) = self.fibers.get_mut(&current_id) {
                fiber.state = FiberState::BlockedIo;
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: current_id,
                    new_state: "BlockedIo".to_string(),
                });
            }
        }
    }

    /// Wake a fiber parked on I/O: push the syscall's result onto its saved
    /// stack (where the resumed code expects the return value) and re-queue it.
    /// Mirrors the channel-receive handoff.
    pub fn wake_io(&mut self, fiber_id: FiberId, result: Value) {
        if let Some(fiber) = self.fibers.get_mut(&fiber_id) {
            // Only resume a fiber that is actually parked on this I/O. If it is
            // gone (Finished/Failed) the result is dropped — the caller has
            // already re-inserted any checked-out handle, so nothing leaks.
            if fiber.state != FiberState::BlockedIo {
                return;
            }
            fiber.stack.push(result);
            fiber.state = FiberState::Ready;
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id,
                new_state: "Ready".to_string(),
            });
            self.ready_queue.push_back(fiber_id);
        }
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
                        fiber.state = FiberState::Running;
                        self.emit_event(FiberEvent::FiberStateChanged {
                            fiber_id: id,
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
                fiber.state = FiberState::Finished;
                fiber.result = Some(result);
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: current_id,
                    new_state: "Finished".to_string(),
                });
            }
            self.current = None;
        }
    }

    /// Mark the current fiber as failed
    pub fn fail_current(&mut self, error: String) {
        if let Some(current_id) = self.current {
            self.fail_fiber(current_id, error);
            self.current = None;
        }
    }

    /// Mark a fiber failed without changing the currently running fiber.
    ///
    /// This is used for blocked senders awakened by channel close. The first
    /// failure is retained; later failures are still reflected in their fiber
    /// states but cannot race the causal error that happened first.
    fn fail_fiber(&mut self, fiber_id: FiberId, error: String) {
        if let Some(fiber) = self.fibers.get_mut(&fiber_id) {
            if matches!(fiber.state, FiberState::Finished | FiberState::Failed(_)) {
                return;
            }
            fiber.failure_ip = Some(match fiber.state {
                FiberState::BlockedSend(_) => fiber.ip.saturating_sub(1),
                _ => fiber.ip,
            });
            fiber.state = FiberState::Failed(error.clone());
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id,
                new_state: format!("Failed({})", error),
            });
            if self.first_failure.is_none() {
                self.first_failure = Some((fiber_id, error));
            }
        }
    }

    /// Return the first unhandled fiber failure, if any.
    pub fn first_failure(&self) -> Option<(FiberId, String)> {
        self.first_failure.clone()
    }

    /// Check if there are any runnable fibers
    pub fn has_runnable(&self) -> bool {
        !self.ready_queue.is_empty() || self.current.is_some()
    }

    /// Check if all fibers are finished or blocked
    pub fn is_deadlocked(&self) -> bool {
        if self.fibers.is_empty() {
            return true;
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
    pub fn create_channel(&mut self, capacity: usize) -> Result<ChannelId, String> {
        let id = self.next_channel_id;
        let next_id = self
            .next_channel_id
            .checked_add(1)
            .ok_or_else(|| "Channel identifier space exhausted".to_string())?;

        let channel = Channel::new(id, capacity)?;
        self.next_channel_id = next_id;
        self.channels.insert(id, channel);

        // Emit channel created event
        self.emit_event(FiberEvent::ChannelCreated {
            channel_id: id,
            capacity,
        });

        Ok(id)
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
            let is_select = self
                .fibers
                .get(&receiver_id)
                .is_some_and(Self::is_parked_select);
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
                receiver.state = FiberState::Ready;
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: receiver_id,
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
                fiber.state = FiberState::BlockedSend(channel_id);
                self.emit_event(FiberEvent::FiberStateChanged {
                    fiber_id: current_id,
                    new_state: format!("BlockedSend({})", channel_id),
                });
            }
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                channel.senders.push_back((current_id, value));
            }
            woken_selectors.sort_unstable();
            woken_selectors.dedup();
            for receiver_id in woken_selectors {
                if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                    if matches!(receiver.state, FiberState::BlockedSelect) {
                        receiver.state = FiberState::Ready;
                        self.emit_event(FiberEvent::FiberStateChanged {
                            fiber_id: receiver_id,
                            new_state: "Ready".to_string(),
                        });
                        self.ready_queue.push_back(receiver_id);
                    }
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
            fiber.state = FiberState::BlockedSend(channel_id);
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id: current_id,
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

        if !self.channels.contains_key(&channel_id) {
            return Err("Invalid channel".to_string());
        }

        // Check buffer first.
        let buffered = self
            .channels
            .get_mut(&channel_id)
            .and_then(|channel| channel.buffer.pop_front());
        if let Some(value) = buffered {
            let value_str = format!("{:?}", value);
            if let Some(sender_index) = self.first_ordinary_sender(channel_id) {
                if let Some((sender_id, sender_value)) = self
                    .channels
                    .get_mut(&channel_id)
                    .and_then(|channel| channel.senders.remove(sender_index))
                {
                    if let Some(channel) = self.channels.get_mut(&channel_id) {
                        channel.buffer.push_back(sender_value);
                    }
                    if let Some(sender) = self.fibers.get_mut(&sender_id) {
                        if Self::is_parked_select(sender) {
                            sender.select_resolution = Some(SelectResolution {
                                channel_id,
                                recv: None,
                            });
                        }
                        sender.state = FiberState::Ready;
                        self.ready_queue.push_back(sender_id);
                    }
                }
            } else if self
                .channels
                .get(&channel_id)
                .is_some_and(|channel| !channel.senders.is_empty())
            {
                self.wake_select_senders(channel_id);
            }
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "receive".to_string(),
                value: value_str,
            });
            return Ok(Some((value, true)));
        }

        // Check for a waiting sender (unbuffered handoff). Skip deferred
        // parked select senders, but consume a later ordinary sender.
        if let Some(sender_index) = self.first_ordinary_sender(channel_id) {
            if let Some((sender_id, value)) = self
                .channels
                .get_mut(&channel_id)
                .and_then(|channel| channel.senders.remove(sender_index))
            {
                let value_str = format!("{:?}", value);
                if let Some(sender) = self.fibers.get_mut(&sender_id) {
                    if Self::is_parked_select(sender) {
                        sender.select_resolution = Some(SelectResolution {
                            channel_id,
                            recv: None,
                        });
                    }
                    sender.state = FiberState::Ready;
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id: sender_id,
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
        }

        // Channel is empty
        if self
            .channels
            .get(&channel_id)
            .is_some_and(|channel| channel.closed)
        {
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "receive".to_string(),
                value: "null (closed)".to_string(),
            });
            return Ok(Some((Value::Null, false))); // ok = false means closed
        }

        // Need to block
        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            fiber.state = FiberState::BlockedReceive(channel_id);
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id: current_id,
                new_state: format!("BlockedReceive({})", channel_id),
            });
        }

        // Re-get channel
        let select_sender_waiting = self
            .channels
            .get(&channel_id)
            .is_some_and(|channel| !channel.senders.is_empty());
        if let Some(channel) = self.channels.get_mut(&channel_id) {
            channel.receivers.push_back(current_id);
        }

        if select_sender_waiting {
            self.wake_select_senders(channel_id);
        }

        self.current = None;
        Ok(None)
    }

    /// Wake every select sender currently parked on a channel. The sender
    /// remains queued so its resumed Select can choose among all arms; this is
    /// only a readiness notification, not a channel commit.
    fn wake_select_senders(&mut self, channel_id: ChannelId) {
        let sender_ids: Vec<FiberId> = self
            .channels
            .get(&channel_id)
            .map(|channel| {
                channel
                    .senders
                    .iter()
                    .filter_map(|(fiber_id, _)| {
                        matches!(
                            self.fibers.get(fiber_id).map(|fiber| &fiber.state),
                            Some(FiberState::BlockedSelect)
                        )
                        .then_some(*fiber_id)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut sender_ids = sender_ids;
        sender_ids.sort_unstable();
        sender_ids.dedup();
        for fiber_id in sender_ids {
            if let Some(sender) = self.fibers.get_mut(&fiber_id) {
                if matches!(sender.state, FiberState::BlockedSelect) {
                    sender.state = FiberState::Ready;
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id,
                        new_state: "Ready".to_string(),
                    });
                    self.ready_queue.push_back(fiber_id);
                }
            }
        }
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

        // A select can register duplicate arms on one channel. Close wakes a
        // fiber once even when the channel queue contains duplicate entries.
        let mut receiver_ids = receiver_ids;
        receiver_ids.sort_unstable();
        receiver_ids.dedup();

        // Wake all blocked receivers with (null, false). Select-waiters
        // (BlockedSelect) must NOT get the pair pushed onto their stack: they
        // re-run `Select`, and `try_select` reports the closed channel as ready.
        for receiver_id in receiver_ids {
            if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                let is_select = Self::is_parked_select(receiver);
                let should_enqueue = !matches!(receiver.state, FiberState::Ready);
                if !is_select {
                    receiver.stack.push(Value::Null);
                    receiver.stack.push(Value::Bool(false)); // ok = false
                }
                receiver.state = FiberState::Ready;
                if should_enqueue {
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id: receiver_id,
                        new_state: "Ready".to_string(),
                    });
                    self.ready_queue.push_back(receiver_id);
                }
            }
        }

        // Ordinary blocked sends fail on close. A parked select sender is
        // different: its send arm simply becomes unavailable, so wake it to
        // re-arbitrate any remaining arms instead of failing the fiber.
        for (sender_id, _) in sender_ids {
            let is_select = matches!(
                self.fibers.get(&sender_id),
                Some(fiber)
                    if fiber.select_channels.contains(&channel_id)
                        || matches!(fiber.state, FiberState::BlockedSelect)
            );
            if is_select {
                if let Some(sender) = self.fibers.get_mut(&sender_id) {
                    if sender.state != FiberState::Ready {
                        sender.state = FiberState::Ready;
                        self.emit_event(FiberEvent::FiberStateChanged {
                            fiber_id: sender_id,
                            new_state: "Ready".to_string(),
                        });
                        self.ready_queue.push_back(sender_id);
                    }
                }
            } else {
                self.fail_fiber(sender_id, "send on closed channel".to_string());
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
            if let Some((value, ok)) = self.try_select_receive(channel_id) {
                return Some((index, value, ok));
            }
        }
        None
    }

    /// Check whether a receive arm is ready without consuming it.
    pub fn select_receive_ready(&self, channel_id: ChannelId) -> bool {
        self.select_receive_ready_excluding(channel_id, self.current)
    }

    fn select_receive_ready_excluding(
        &self,
        channel_id: ChannelId,
        exclude_fiber: Option<FiberId>,
    ) -> bool {
        self.channels.get(&channel_id).is_some_and(|channel| {
            !channel.buffer.is_empty()
                || channel.closed
                || channel
                    .senders
                    .iter()
                    .any(|(fiber_id, _)| Some(*fiber_id) != exclude_fiber)
        })
    }

    /// Check whether a send arm is ready without consuming or producing a
    /// channel value. A parked select receiver is a valid rendezvous target.
    pub fn select_send_ready(&self, channel_id: ChannelId) -> bool {
        self.select_send_ready_excluding(channel_id, self.current)
    }

    fn select_send_ready_excluding(
        &self,
        channel_id: ChannelId,
        exclude_fiber: Option<FiberId>,
    ) -> bool {
        self.channels.get(&channel_id).is_some_and(|channel| {
            !channel.closed
                && if channel.capacity > 0 {
                    channel.buffer.len() < channel.capacity
                } else {
                    channel
                        .receivers
                        .iter()
                        .any(|fiber_id| Some(*fiber_id) != exclude_fiber)
                }
        })
    }

    fn parked_offer_has_alternate(
        &self,
        fiber_id: FiberId,
        channel_id: ChannelId,
        is_send: bool,
    ) -> bool {
        let Some(fiber) = self.fibers.get(&fiber_id) else {
            return false;
        };
        fiber.select_parked
            && fiber.select_offers.iter().any(|offer| {
                (offer.channel_id != channel_id || offer.is_send != is_send)
                    && if offer.is_send {
                        self.select_send_ready_excluding(offer.channel_id, Some(fiber_id))
                    } else {
                        self.select_receive_ready_excluding(offer.channel_id, Some(fiber_id))
                    }
            })
    }

    /// Duplicate arms on the same endpoint still need one queue registration,
    /// but they remain distinct offers for seeded body/value arbitration.
    fn parked_offer_is_ambiguous(
        &self,
        fiber_id: FiberId,
        channel_id: ChannelId,
        is_send: bool,
    ) -> bool {
        self.fibers.get(&fiber_id).is_some_and(|fiber| {
            fiber.select_parked
                && fiber
                    .select_offers
                    .iter()
                    .filter(|offer| offer.channel_id == channel_id && offer.is_send == is_send)
                    .count()
                    > 1
        })
    }

    /// Find an ordinary blocked sender, deliberately excluding every parked
    /// select sender. A normal receive defers parked select offers so the
    /// selector can arbitrate (and can still consume a later ordinary sender
    /// queued behind them).
    fn first_ordinary_sender(&self, channel_id: ChannelId) -> Option<usize> {
        let channel = self.channels.get(&channel_id)?;
        channel.senders.iter().position(|(sender_id, _)| {
            !self
                .fibers
                .get(sender_id)
                .is_some_and(Self::is_parked_select)
        })
    }

    fn first_committable_sender(&self, channel_id: ChannelId) -> Option<usize> {
        let channel = self.channels.get(&channel_id)?;
        channel.senders.iter().position(|(sender_id, _)| {
            if !self
                .fibers
                .get(sender_id)
                .is_some_and(Self::is_parked_select)
            {
                true
            } else {
                !self.parked_offer_has_alternate(*sender_id, channel_id, true)
                    && !self.parked_offer_is_ambiguous(*sender_id, channel_id, true)
            }
        })
    }

    /// Whether every currently queued sender is a parked select sender that
    /// has another ready offer. Such a rendezvous must be deferred so that the
    /// parked selector can arbitrate its own offers first.
    pub fn select_receive_deferred(&self, channel_id: ChannelId) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else {
            return false;
        };
        if !channel.buffer.is_empty() || channel.closed || channel.senders.is_empty() {
            return false;
        }
        let external: Vec<FiberId> = channel
            .senders
            .iter()
            .filter_map(|(fiber_id, _)| (Some(*fiber_id) != self.current).then_some(*fiber_id))
            .collect();
        if external.is_empty() {
            return false;
        }
        let mut found = false;
        for &fiber_id in &external {
            if !self
                .fibers
                .get(&fiber_id)
                .is_some_and(Self::is_parked_select)
            {
                return false;
            }
            found = true;
            if !self.parked_offer_has_alternate(fiber_id, channel_id, true)
                && !self.parked_offer_is_ambiguous(fiber_id, channel_id, true)
            {
                return false;
            }
        }
        found
    }

    /// Whether every currently queued receiver is a parked select receiver
    /// that has another ready offer and therefore must not be consumed yet.
    pub fn select_send_deferred(&self, channel_id: ChannelId) -> bool {
        let Some(channel) = self.channels.get(&channel_id) else {
            return false;
        };
        if channel.capacity > 0 || channel.closed || channel.receivers.is_empty() {
            return false;
        }
        let external: Vec<FiberId> = channel
            .receivers
            .iter()
            .filter_map(|fiber_id| (Some(*fiber_id) != self.current).then_some(*fiber_id))
            .collect();
        if external.is_empty() {
            return false;
        }
        let mut found = false;
        for fiber_id in &external {
            if !self
                .fibers
                .get(fiber_id)
                .is_some_and(Self::is_parked_select)
            {
                return false;
            }
            found = true;
            if !self.parked_offer_has_alternate(*fiber_id, channel_id, false) {
                return false;
            }
        }
        found
    }

    /// Wake parked selectors offering the opposite direction on the supplied
    /// channels. This is a readiness notification only; the queues remain
    /// untouched until the woken selector commits its chosen arm.
    pub fn wake_select_counterparts(&mut self, recv_ids: &[ChannelId], send_ids: &[ChannelId]) {
        let mut wake = Vec::new();
        for channel_id in recv_ids {
            if let Some(channel) = self.channels.get(channel_id) {
                for (fiber_id, _) in &channel.senders {
                    if matches!(
                        self.fibers.get(fiber_id).map(|fiber| &fiber.state),
                        Some(FiberState::BlockedSelect)
                    ) {
                        wake.push(*fiber_id);
                    }
                }
            }
        }
        for channel_id in send_ids {
            if let Some(channel) = self.channels.get(channel_id) {
                for fiber_id in &channel.receivers {
                    if matches!(
                        self.fibers.get(fiber_id).map(|fiber| &fiber.state),
                        Some(FiberState::BlockedSelect)
                    ) {
                        wake.push(*fiber_id);
                    }
                }
            }
        }
        wake.sort_unstable();
        wake.dedup();
        for fiber_id in wake {
            if let Some(fiber) = self.fibers.get_mut(&fiber_id) {
                fiber.state = FiberState::Ready;
                self.ready_queue.push_back(fiber_id);
            }
        }
    }

    /// Commit a receive on exactly one channel selected by fair arbitration.
    /// Readiness probing is intentionally separate so probing another arm
    /// cannot consume this arm's value.
    pub fn try_select_receive(&mut self, channel_id: ChannelId) -> Option<(Value, bool)> {
        let buffered = self.channels.get(&channel_id)?.buffer.front().cloned();
        if let Some(value) = buffered {
            self.channels.get_mut(&channel_id)?.buffer.pop_front();
            return Some((value, true));
        }

        let sender_index = self.first_committable_sender(channel_id);
        if let Some(sender_index) = sender_index {
            let (sender_id, value) = self
                .channels
                .get_mut(&channel_id)?
                .senders
                .remove(sender_index)?;
            let is_select = self
                .fibers
                .get(&sender_id)
                .is_some_and(Self::is_parked_select);
            let should_enqueue = self
                .fibers
                .get(&sender_id)
                .is_some_and(|sender| !matches!(sender.state, FiberState::Ready));
            if is_select {
                // Record the exact communication before withdrawing every
                // losing offer. This makes a parked selector atomic: another
                // operation cannot commit its stale registrations while it is
                // waiting in the ready queue.
                if let Some(sender) = self.fibers.get_mut(&sender_id) {
                    sender.select_resolution = Some(SelectResolution {
                        channel_id,
                        recv: None,
                    });
                }
                self.deregister_select_waiter(sender_id);
            }
            if let Some(sender) = self.fibers.get_mut(&sender_id) {
                sender.state = FiberState::Ready;
                if should_enqueue {
                    self.ready_queue.push_back(sender_id);
                }
            }
            return Some((value, true));
        }

        if self
            .channels
            .get(&channel_id)
            .is_some_and(|channel| channel.closed)
        {
            return Some((Value::Null, false));
        }

        None
    }

    /// Try to send a value on a channel without blocking (select send arm).
    ///
    /// Returns `true` if the value was handed to a waiting receiver or buffered,
    /// `false` if the send would block (no receiver, full/unbuffered). Never
    /// parks the current fiber.
    pub fn try_select_send(&mut self, channel_id: ChannelId, value: Value) -> bool {
        if self
            .channels
            .get(&channel_id)
            .is_none_or(|channel| channel.closed)
        {
            return false;
        }

        // Direct handoff to a waiting receiver. A parked select receiver with
        // another ready arm is skipped until it has re-arbitrated.
        let receiver_index = self.channels.get(&channel_id).and_then(|channel| {
            channel.receivers.iter().position(|receiver_id| {
                !self
                    .fibers
                    .get(receiver_id)
                    .is_some_and(Self::is_parked_select)
                    || !self.parked_offer_has_alternate(*receiver_id, channel_id, false)
            })
        });
        if let Some(receiver_index) = receiver_index {
            let receiver_id = match self.channels.get_mut(&channel_id) {
                Some(channel) => match channel.receivers.remove(receiver_index) {
                    Some(receiver_id) => receiver_id,
                    None => return false,
                },
                None => return false,
            };
            let value_str = format!("{:?}", value);
            let is_select = self
                .fibers
                .get(&receiver_id)
                .is_some_and(Self::is_parked_select);
            let should_enqueue = self
                .fibers
                .get(&receiver_id)
                .is_some_and(|receiver| !matches!(receiver.state, FiberState::Ready));
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
            }
            if is_select {
                self.deregister_select_waiter(receiver_id);
            }
            if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                receiver.state = FiberState::Ready;
                if should_enqueue {
                    self.emit_event(FiberEvent::FiberStateChanged {
                        fiber_id: receiver_id,
                        new_state: "Ready".to_string(),
                    });
                    self.ready_queue.push_back(receiver_id);
                }
            }
            self.emit_event(FiberEvent::ChannelMessage {
                channel_id,
                operation: "send".to_string(),
                value: value_str,
            });
            return true;
        }

        // Buffer if there is room.
        let channel = match self.channels.get_mut(&channel_id) {
            Some(channel) => channel,
            None => return false,
        };
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

        // A spurious wake can re-enter Select while its old registrations are
        // still present. Withdraw them before re-registering so each fiber has
        // at most one queue entry per channel/direction; duplicate arm offers
        // are retained separately below for fair body/value selection.
        let has_old_registration = self
            .fibers
            .get(&current_id)
            .is_some_and(|fiber| fiber.select_parked || !fiber.select_channels.is_empty());
        if has_old_registration {
            self.deregister_select_waiter(current_id);
        }

        // Record every channel the fiber registers on so the commit paths can
        // de-register it from the losing arms.
        let mut registered: Vec<ChannelId> = Vec::with_capacity(recv_ids.len() + send_specs.len());
        let mut offers = Vec::with_capacity(recv_ids.len() + send_specs.len());
        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            fiber.state = FiberState::BlockedSelect;
            self.emit_event(FiberEvent::FiberStateChanged {
                fiber_id: current_id,
                new_state: "BlockedSelect".to_string(),
            });
        }

        for &channel_id in recv_ids {
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                if !channel.receivers.contains(&current_id) {
                    channel.receivers.push_back(current_id);
                }
                if !registered.contains(&channel_id) {
                    registered.push(channel_id);
                }
                offers.push(SelectOffer {
                    channel_id,
                    is_send: false,
                });
            }
        }
        for (channel_id, value) in send_specs {
            if let Some(channel) = self.channels.get_mut(channel_id) {
                // A send on a closed channel is not selectable. In
                // particular, do not register it as a parked sender: closing
                // the channel must not turn a losing select arm into a fiber
                // failure.
                if channel.closed {
                    continue;
                }
                if !channel
                    .senders
                    .iter()
                    .any(|(fiber_id, _)| *fiber_id == current_id)
                {
                    channel.senders.push_back((current_id, value.clone()));
                }
                if !registered.contains(channel_id) {
                    registered.push(*channel_id);
                }
                offers.push(SelectOffer {
                    channel_id: *channel_id,
                    is_send: true,
                });
            }
        }

        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            fiber.select_channels = registered;
            fiber.select_parked = true;
            fiber.select_offers = offers;
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
        let Some(channels) = self.fibers.get_mut(&fiber_id).map(|fiber| {
            let channels = std::mem::take(&mut fiber.select_channels);
            fiber.select_parked = false;
            fiber.select_offers.clear();
            channels
        }) else {
            return;
        };
        if channels.is_empty() {
            return;
        }
        for &channel_id in &channels {
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                channel.receivers.retain(|&id| id != fiber_id);
                channel.senders.retain(|(id, _)| *id != fiber_id);
            }
        }
        self.wake_select_waiters_after_offer_withdrawal(&channels, fiber_id);
    }

    /// A parked selector withdrawing an offer can make an opposite parked
    /// selector runnable again. Wake each such selector once; a selector that
    /// is already Ready has already been queued and must not be duplicated.
    fn wake_select_waiters_after_offer_withdrawal(
        &mut self,
        channels: &[ChannelId],
        withdrawn_fiber: FiberId,
    ) {
        let mut wake = std::collections::HashSet::new();
        for channel_id in channels {
            if let Some(channel) = self.channels.get(channel_id) {
                wake.extend(channel.receivers.iter().copied());
                wake.extend(channel.senders.iter().map(|(id, _)| *id));
            }
        }
        for fiber_id in wake {
            if fiber_id == withdrawn_fiber {
                continue;
            }
            if let Some(fiber) = self.fibers.get_mut(&fiber_id) {
                if matches!(fiber.state, FiberState::BlockedSelect) {
                    fiber.state = FiberState::Ready;
                    self.ready_queue.push_back(fiber_id);
                }
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

        let ch = sched.create_channel(2).expect("create buffered channel");

        // Send should succeed (buffered)
        let result = sched.channel_send(ch, Value::Int(42));
        assert!(result.is_ok());
        assert!(result.unwrap()); // Should return true (sent immediately)
    }

    #[test]
    fn test_channel_close() {
        let mut sched = Scheduler::new();
        let ch = sched.create_channel(1).expect("create channel");

        let result = sched.close_channel(ch);
        assert!(result.is_ok());

        // Channel should be closed
        assert!(sched.channels.get(&ch).unwrap().closed);
    }

    #[test]
    fn reparking_select_does_not_duplicate_channel_registrations() {
        let mut sched = Scheduler::new();
        let fiber_id = sched.spawn(0);
        assert_eq!(sched.schedule(), Some(fiber_id));
        let ch = sched.create_channel(0).expect("create channel");
        let sends = [(ch, Value::Int(11)), (ch, Value::Int(22))];

        sched.park_select(&[ch], &sends);
        assert_eq!(sched.channels[&ch].receivers, [fiber_id]);
        assert_eq!(
            sched.channels[&ch]
                .senders
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [fiber_id]
        );
        assert_eq!(sched.fibers[&fiber_id].select_offers.len(), 3);

        // Simulate a spurious wake before the select has committed. Re-parking
        // must retain all arm offers but replace, rather than append, queue
        // registrations.
        sched.current = Some(fiber_id);
        sched.fibers.get_mut(&fiber_id).unwrap().state = FiberState::Ready;
        sched.park_select(&[ch], &sends);
        assert_eq!(sched.channels[&ch].receivers, [fiber_id]);
        assert_eq!(
            sched.channels[&ch]
                .senders
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [fiber_id]
        );
        assert_eq!(sched.fibers[&fiber_id].select_channels, [ch]);
        assert_eq!(sched.fibers[&fiber_id].select_offers.len(), 3);
    }

    #[test]
    fn deregister_clears_closed_send_only_select_state() {
        let mut sched = Scheduler::new();
        let fiber_id = sched.spawn(0);
        assert_eq!(sched.schedule(), Some(fiber_id));
        let ch = sched.create_channel(0).expect("create channel");
        sched.close_channel(ch).unwrap();

        // park_select records the select state even though closed sends are
        // intentionally omitted from channel queues.
        sched.park_select(&[], &[(ch, Value::Int(1))]);
        assert!(sched.fibers[&fiber_id].select_parked);
        assert!(sched.fibers[&fiber_id].select_channels.is_empty());

        sched.deregister_select_waiter(fiber_id);
        assert!(!sched.fibers[&fiber_id].select_parked);
        assert!(sched.fibers[&fiber_id].select_offers.is_empty());
    }

    #[test]
    fn select_resolution_withdraws_losing_receive_offers_atomically() {
        let mut sched = Scheduler::new();
        let parked = sched.spawn(0);
        assert_eq!(sched.schedule(), Some(parked));
        let a = sched.create_channel(0).expect("create channel a");
        let b = sched.create_channel(0).expect("create channel b");
        sched.park_select(&[a, b], &[]);

        let sender = sched.spawn(0);
        assert_eq!(sched.schedule(), Some(sender));
        assert!(sched.try_select_send(a, Value::Int(1)));
        assert!(!sched.try_select_send(b, Value::Int(2)));
        assert_eq!(sched.channels[&b].receivers, Vec::<FiberId>::new());
        assert_eq!(
            sched.ready_queue.iter().filter(|id| **id == parked).count(),
            1
        );
        assert_eq!(
            sched.fibers[&parked]
                .select_resolution
                .as_ref()
                .map(|resolution| resolution.channel_id),
            Some(a)
        );
    }

    #[test]
    fn select_resolution_withdraws_losing_send_offers_atomically() {
        let mut sched = Scheduler::new();
        let parked = sched.spawn(0);
        assert_eq!(sched.schedule(), Some(parked));
        let a = sched.create_channel(0).expect("create channel a");
        let b = sched.create_channel(0).expect("create channel b");
        sched.park_select(&[], &[(a, Value::Int(1)), (b, Value::Int(2))]);

        let receiver = sched.spawn(0);
        assert_eq!(sched.schedule(), Some(receiver));
        assert!(matches!(
            sched.try_select_receive(a),
            Some((Value::Int(1), true))
        ));
        assert!(sched.try_select_receive(b).is_none());
        assert_eq!(sched.channels[&b].senders.len(), 0);
        assert_eq!(
            sched.ready_queue.iter().filter(|id| **id == parked).count(),
            1
        );
        assert_eq!(
            sched.fibers[&parked]
                .select_resolution
                .as_ref()
                .map(|resolution| resolution.channel_id),
            Some(a)
        );
    }

    #[test]
    fn test_is_deadlocked_empty_scheduler() {
        let sched = Scheduler::new();
        // An empty scheduler has no fibers at all — it is deadlocked
        assert!(sched.is_deadlocked());
    }

    #[test]
    fn test_is_deadlocked_with_runnable_fiber() {
        let mut sched = Scheduler::new();
        sched.spawn(0);
        // A Ready fiber is runnable → not deadlocked
        assert!(!sched.is_deadlocked());
    }

    #[test]
    fn test_is_deadlocked_all_finished() {
        let mut sched = Scheduler::new();
        let _id = sched.spawn(0);
        sched.schedule(); // make it running
        sched.finish_current(Value::Int(42)); // mark finished
                                              // The only fiber is finished → deadlocked
        assert!(sched.is_deadlocked());
    }

    #[test]
    fn test_is_deadlocked_blocked_and_finished() {
        let mut sched = Scheduler::new();
        // Spawn one that finishes
        let _id1 = sched.spawn(0);
        sched.schedule();
        sched.finish_current(Value::Null);
        // Spawn one that blocks on receive
        let _id2 = sched.spawn(0);
        sched.schedule();
        let ch = sched.create_channel(1).expect("create channel");
        // Simulate blocking: set fiber state directly
        if let Some(fiber) = sched.current_fiber_mut() {
            fiber.state = FiberState::BlockedReceive(ch);
        }
        sched.current = None;
        // One finished + one blocked with no ready_queue → deadlocked
        assert!(sched.is_deadlocked());
    }
}
