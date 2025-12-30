//! Lira Fiber (Green Thread) Scheduler
//!
//! Implements cooperative multitasking for Lira's concurrency model.
//! See docs/lira/04-concurrency.md for the full specification.

use crate::value::{ChannelId, FiberId, Value};
use std::collections::{HashMap, VecDeque};

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

        id
    }

    /// Spawn a fiber with initial arguments on the stack
    pub fn spawn_with_args(&mut self, ip: usize, args: Vec<Value>) -> FiberId {
        let id = self.spawn(ip);
        if let Some(fiber) = self.fibers.get_mut(&id) {
            fiber.stack = args;
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
                    fiber.state = FiberState::Yielded;
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
                        fiber.state = FiberState::Running;
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
            }
            self.current = None;
        }
    }

    /// Mark the current fiber as failed
    pub fn fail_current(&mut self, error: String) {
        if let Some(current_id) = self.current {
            if let Some(fiber) = self.fibers.get_mut(&current_id) {
                fiber.state = FiberState::Failed(error);
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

        id
    }

    /// Send a value on a channel, returns true if sent immediately, false if blocked
    pub fn channel_send(&mut self, channel_id: ChannelId, value: Value) -> Result<bool, String> {
        let current_id = self.current.ok_or("No current fiber")?;

        let channel = self
            .channels
            .get_mut(&channel_id)
            .ok_or("Invalid channel")?;

        if channel.closed {
            return Err("Cannot send on closed channel".to_string());
        }

        // Check if there's a waiting receiver
        if let Some(receiver_id) = channel.receivers.pop_front() {
            // Direct handoff to receiver
            if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                receiver.stack.push(value);
                receiver.stack.push(Value::Bool(true)); // ok = true
                receiver.state = FiberState::Ready;
                self.ready_queue.push_back(receiver_id);
            }
            return Ok(true);
        }

        // Check if we can buffer
        if channel.capacity > 0 && channel.buffer.len() < channel.capacity {
            channel.buffer.push_back(value);
            return Ok(true);
        }

        // Need to block
        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            fiber.state = FiberState::BlockedSend(channel_id);
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
            // Wake up a blocked sender if any
            if let Some((sender_id, sender_value)) = channel.senders.pop_front() {
                channel.buffer.push_back(sender_value);
                if let Some(sender) = self.fibers.get_mut(&sender_id) {
                    sender.state = FiberState::Ready;
                    self.ready_queue.push_back(sender_id);
                }
            }
            return Ok(Some((value, true)));
        }

        // Check for waiting sender (unbuffered handoff)
        if let Some((sender_id, value)) = channel.senders.pop_front() {
            if let Some(sender) = self.fibers.get_mut(&sender_id) {
                sender.state = FiberState::Ready;
                self.ready_queue.push_back(sender_id);
            }
            return Ok(Some((value, true)));
        }

        // Channel is empty
        if channel.closed {
            return Ok(Some((Value::Null, false))); // ok = false means closed
        }

        // Need to block
        if let Some(fiber) = self.fibers.get_mut(&current_id) {
            fiber.state = FiberState::BlockedReceive(channel_id);
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
        let channel = self
            .channels
            .get_mut(&channel_id)
            .ok_or("Invalid channel")?;

        channel.closed = true;

        // Wake all blocked receivers with (null, false)
        while let Some(receiver_id) = channel.receivers.pop_front() {
            if let Some(receiver) = self.fibers.get_mut(&receiver_id) {
                receiver.stack.push(Value::Null);
                receiver.stack.push(Value::Bool(false)); // ok = false
                receiver.state = FiberState::Ready;
                self.ready_queue.push_back(receiver_id);
            }
        }

        // Wake all blocked senders with error
        while let Some((sender_id, _)) = channel.senders.pop_front() {
            if let Some(sender) = self.fibers.get_mut(&sender_id) {
                sender.state = FiberState::Failed("send on closed channel".to_string());
            }
        }

        Ok(())
    }

    /// Try to receive from any of the given channels (select)
    pub fn try_select(&mut self, channel_ids: &[ChannelId]) -> Option<(usize, Value)> {
        for (index, &channel_id) in channel_ids.iter().enumerate() {
            if let Some(channel) = self.channels.get_mut(&channel_id) {
                // Check buffer
                if let Some(value) = channel.buffer.pop_front() {
                    return Some((index, value));
                }
                // Check waiting senders
                if let Some((sender_id, value)) = channel.senders.pop_front() {
                    if let Some(sender) = self.fibers.get_mut(&sender_id) {
                        sender.state = FiberState::Ready;
                        self.ready_queue.push_back(sender_id);
                    }
                    return Some((index, value));
                }
            }
        }
        None
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
