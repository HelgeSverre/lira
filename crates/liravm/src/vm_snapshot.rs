//! Value-carrying snapshot DTOs for the fiber scheduler.
//!
//! These are the shared, serialization-agnostic contract a debugger/playground
//! consumes (via [`crate::vm::VM::scheduler_snapshot`]). They live apart from
//! the interpreter so the inspection surface is decoupled from VM internals.

use crate::debug::RichValue;
use crate::fiber::FiberState;
use crate::value::{ChannelId, FiberId};

/// Detailed, value-carrying snapshot of the fiber scheduler.
///
/// Captures every fiber's and channel's live contents as [`RichValue`]s so a
/// debugger/playground can render the full concurrent state. Fibers and
/// channels are ordered by id for deterministic output (the scheduler stores
/// them in `HashMap`s).
#[derive(Debug, Clone)]
pub struct SchedulerSnapshot {
    pub fibers: Vec<FiberStateSnapshot>,
    pub channels: Vec<ChannelStateSnapshot>,
    pub current_fiber_id: Option<FiberId>,
    pub ready_queue: Vec<FiberId>,
}

/// Detailed snapshot of a single fiber, including its operand stack and locals.
#[derive(Debug, Clone)]
pub struct FiberStateSnapshot {
    pub id: FiberId,
    pub state: FiberState,
    pub ip: usize,
    pub stack: Vec<RichValue>,
    pub locals: Vec<RichValue>,
    pub call_stack: Vec<FiberFrameSnapshot>,
    pub result: Option<RichValue>,
}

/// A single call frame within a fiber's snapshot.
#[derive(Debug, Clone)]
pub struct FiberFrameSnapshot {
    pub return_addr: usize,
    pub locals_base: usize,
    /// Resolved function name, when the bytecode debug info can recover one.
    pub function_name: Option<String>,
}

/// Detailed snapshot of a channel, including buffered values and waiters.
#[derive(Debug, Clone)]
pub struct ChannelStateSnapshot {
    pub id: ChannelId,
    pub buffer: Vec<RichValue>,
    pub capacity: usize,
    pub receivers: Vec<FiberId>,
    pub senders: Vec<(FiberId, RichValue)>,
    pub closed: bool,
}
