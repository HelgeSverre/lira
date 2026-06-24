//! Lira Value Types
//!
//! Defines all value types used in the Lira virtual machine.
//!
//! Heap values that can form reference cycles (`Array`, `Object`, `Closure`)
//! are held behind the tracing garbage-collected pointer [`gc::Gc`] so that
//! cyclic structures (e.g. a node whose field points back at itself, or a
//! closure capturing a table that holds the closure) are reclaimed by the
//! mark-and-sweep collector rather than leaking for the life of the process.
//! Acyclic interned strings (`IString`) deliberately stay [`std::rc::Rc`].

// `gc_derive` emits its generated `impl` blocks inside a `const _: () = { .. }`
// scope, which trips rustc's `non_local_definitions` lint. Silence it at module
// scope; revisit if `gc` ever ships a derive that emits top-level impls.
#![allow(non_local_definitions)]

use gc::{Finalize, Gc, GcCell, Trace};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// Interned string type - shared reference-counted string
pub type IString = Rc<String>;

/// Fiber ID type
pub type FiberId = u64;

/// Channel ID type
pub type ChannelId = u64;

/// Value types in the VM
///
/// Cyclic-capable heap variants (`Array`, `Object`, `Closure`) are
/// garbage-collected via [`gc::Gc`]; the tracing collector reclaims reference
/// cycles that plain reference counting cannot. The `#[unsafe_ignore_trace]` on
/// `String` is sound because `IString` (`Rc<String>`) never holds a `Gc` and so
/// has nothing for the tracer to reach; the remaining scalar variants are
/// `Copy` and derive `Trace` trivially.
#[derive(Debug, Clone, Trace, Finalize)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(#[unsafe_ignore_trace] IString), // Interned string for memory efficiency
    Array(Gc<GcCell<Vec<Value>>>),
    Object(Gc<GcCell<HashMap<String, Value>>>),
    Function(usize),          // Code offset
    Closure(Gc<ClosureData>), // Closure with captured values
    Fiber(FiberId),           // Fiber handle
    Channel(ChannelId),       // Channel handle
}

/// Closure data containing function code and captured values
#[derive(Debug, Clone, Trace, Finalize)]
pub struct ClosureData {
    /// Offset of the function code
    #[unsafe_ignore_trace]
    pub code_offset: usize,
    /// Captured variable values (indexed by capture slot)
    pub captures: Vec<Value>,
}

impl Value {
    /// Return the user-facing type name for this value (e.g. `int`, `string`).
    ///
    /// Used in runtime error messages so users see `Cannot add string and int`
    /// rather than the `Debug` rendering of the underlying variant.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Function(_) => "function",
            Value::Closure(_) => "closure",
            Value::Fiber(_) => "fiber",
            Value::Channel(_) => "channel",
        }
    }

    /// Check if value is truthy
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(arr) => !arr.borrow().is_empty(),
            Value::Object(_) => true,
            Value::Function(_) => true,
            Value::Closure(_) => true,
            Value::Fiber(_) => true,
            Value::Channel(_) => true,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => write!(f, "{}", &**s),
            Value::Array(arr) => {
                let elements: Vec<String> = arr.borrow().iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", elements.join(", "))
            }
            Value::Object(obj) => {
                let fields: Vec<String> = obj
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{{{}}}", fields.join(", "))
            }
            Value::Function(offset) => write!(f, "<function@{}>", offset),
            Value::Closure(c) => write!(f, "<closure@{}>", c.code_offset),
            Value::Fiber(id) => write!(f, "<fiber#{}>", id),
            Value::Channel(id) => write!(f, "<channel#{}>", id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_name_is_user_facing_not_debug() {
        // These are the names used in runtime error messages; they must be the
        // readable forms, not the Debug rendering (`Int(..)`, `String(..)`).
        assert_eq!(Value::Null.type_name(), "null");
        assert_eq!(Value::Bool(true).type_name(), "bool");
        assert_eq!(Value::Int(3).type_name(), "int");
        assert_eq!(Value::Float(1.5).type_name(), "float");
        assert_eq!(
            Value::String(Rc::new("x".to_string())).type_name(),
            "string"
        );
    }

    #[test]
    fn display_renders_without_debug_wrapper() {
        // Display backs the value-rendering error sites; `3` not `Int(3)`.
        assert_eq!(Value::Int(3).to_string(), "3");
        assert_eq!(Value::String(Rc::new("hi".to_string())).to_string(), "hi");
    }
}

/// Tests proving the tracing cycle collector reclaims reference cycles that
/// plain reference counting would leak.
///
/// `Value`'s cyclic variants are `Gc<GcCell<..>>` / `Gc<..>`. A small probe type
/// whose [`Finalize`] bumps a thread-local counter lets us observe exactly when
/// the collector reclaims a cyclic structure: rust-gc invokes `Finalize` on each
/// unreachable node during the sweep (and, crucially, skips finalization
/// entirely when nothing is unreachable). The `Node` cycles below mirror the
/// `Gc<GcCell<..>>` shape the real `Value` heap uses, and the final test ties a
/// probe directly to a genuine `Value::Object` self-cycle.
#[cfg(test)]
mod cycle_collector_tests {
    use super::*;
    use gc::{Finalize, Gc, GcCell, Trace};
    use std::cell::Cell;

    thread_local! {
        /// Number of [`Probe`] finalizations observed on this thread.
        static FINALIZE_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    fn finalize_count() -> usize {
        FINALIZE_COUNT.with(|c| c.get())
    }

    fn reset_finalize_count() {
        FINALIZE_COUNT.with(|c| c.set(0));
    }

    /// A marker whose `Finalize` records that the collector reclaimed it. It
    /// holds no `Gc`, so its trace is empty.
    struct Probe;

    impl Finalize for Probe {
        fn finalize(&self) {
            FINALIZE_COUNT.with(|c| c.set(c.get() + 1));
        }
    }

    // SAFETY: `Probe` owns no `Gc` pointers, so an empty trace is correct: there
    // is nothing for the collector to reach through it.
    unsafe impl Trace for Probe {
        gc::unsafe_empty_trace!();
    }

    /// A node whose `next` edge can point back at another node, forming a cycle.
    /// Each node carries a [`Probe`] so its reclamation is observable.
    #[derive(Trace, Finalize)]
    struct Node {
        next: GcCell<Option<Gc<Node>>>,
        probe: Probe,
    }

    impl Node {
        fn new() -> Gc<Node> {
            Gc::new(Node {
                next: GcCell::new(None),
                probe: Probe,
            })
        }
    }

    /// Tests must run serially: the finalize counter and rust-gc's collector are
    /// thread-local, and `cargo test` runs tests on multiple threads. A mutex
    /// serializes the GC-observing tests so their counters don't interleave.
    fn gc_test_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        // Recover from a poisoned lock: a failing assertion in another GC test
        // should not cascade into spurious failures here.
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// (a) A cycle with no external roots is reclaimed by the collector.
    ///
    /// Two nodes reference each other (`a -> b -> a`). Reference counting alone
    /// can never free this: each node keeps the other's strong count above zero.
    /// After dropping the stack roots and forcing a collection, both probes must
    /// have finalized.
    #[test]
    fn dropped_cycle_is_reclaimed() {
        let _guard = gc_test_lock();
        // Clear any cyclic garbage left rooted-then-dropped by earlier tests so
        // it doesn't finalize during *our* collection and skew the count.
        gc::force_collect();
        reset_finalize_count();

        let a = Node::new();
        let b = Node::new();
        *a.next.borrow_mut() = Some(b.clone());
        *b.next.borrow_mut() = Some(a.clone());

        // Sanity: the cycle is intact and self-referential.
        assert!(a.next.borrow().is_some());
        assert!(b.next.borrow().is_some());

        // Drop the only roots into the cycle. Under pure Rc this leaks forever.
        drop(a);
        drop(b);

        assert_eq!(
            finalize_count(),
            0,
            "nothing should be finalized before collection runs"
        );

        gc::force_collect();

        assert_eq!(
            finalize_count(),
            2,
            "both nodes in the unreachable cycle must be reclaimed"
        );
    }

    /// (b) A cycle still reachable from a root is NOT reclaimed; once the root is
    /// dropped, a subsequent collection does reclaim it.
    #[test]
    fn reachable_cycle_survives() {
        let _guard = gc_test_lock();
        gc::force_collect();
        reset_finalize_count();

        let a = Node::new();
        let b = Node::new();
        *a.next.borrow_mut() = Some(b.clone());
        *b.next.borrow_mut() = Some(a.clone());

        // Keep `a` rooted on the stack across the collection.
        gc::force_collect();

        assert_eq!(
            finalize_count(),
            0,
            "a cycle reachable from a live root must not be collected"
        );
        // The structure is still fully readable through the surviving root.
        let reached_self = {
            let next = a.next.borrow();
            let b_ref = next.as_ref().expect("a -> b edge intact");
            let b_has_edge = b_ref.next.borrow().is_some();
            b_has_edge
        };
        assert!(reached_self, "b -> a edge must still be intact");

        // Now drop the root and collect: the cycle becomes unreachable.
        drop(a);
        drop(b);
        gc::force_collect();

        assert_eq!(
            finalize_count(),
            2,
            "after the last root is dropped the cycle must be reclaimed"
        );
    }

    /// A genuine `Value::Object` self-cycle (`obj["self"] == obj`) is reclaimed.
    ///
    /// This exercises the real `Value` heap type rather than a stand-in. `Value`
    /// is a closed enum with no slot for a `Finalize` probe, so reclamation is
    /// observed through rust-gc's live-heap accounting (`gc::stats`): building a
    /// self-referential object grows `bytes_allocated`, and collecting it after
    /// the roots are dropped shrinks it back. Under pure `Rc` the bytes would
    /// never be released.
    #[test]
    fn value_object_self_cycle_is_reclaimed() {
        let _guard = gc_test_lock();
        // Settle the heap so the baseline excludes earlier tests' garbage.
        gc::force_collect();
        let baseline = gc::stats().bytes_allocated;

        {
            // Build a real self-referential Value::Object: obj["self"] = obj.
            let obj: Gc<GcCell<HashMap<String, Value>>> = Gc::new(GcCell::new(HashMap::new()));
            obj.borrow_mut()
                .insert("self".to_string(), Value::Object(obj.clone()));
            let value_cycle = Value::Object(obj);

            // Navigating obj["self"] yields the object itself (cycle is real).
            if let Value::Object(map) = &value_cycle {
                let borrowed = map.borrow();
                assert!(
                    matches!(borrowed.get("self"), Some(Value::Object(_))),
                    "obj[\"self\"] must be the object itself"
                );
            } else {
                panic!("expected Value::Object");
            }

            assert!(
                gc::stats().bytes_allocated > baseline,
                "the live self-cycle must occupy GC heap"
            );
            // value_cycle and the clone inside the map drop here: the cycle is
            // now unreachable but, being a cycle, still allocated.
        }

        // Collecting reclaims the unreachable Value::Object cycle.
        gc::force_collect();
        assert_eq!(
            gc::stats().bytes_allocated,
            baseline,
            "the dropped Value::Object cycle must be reclaimed by the collector"
        );
    }

    /// A `Value::Array` / `Value::Closure` mutual cycle is reclaimed: a closure
    /// captures an array, and the array holds the closure. This is the
    /// "closure capturing a table that holds the closure" leak from the issue.
    #[test]
    fn value_array_closure_cycle_is_reclaimed() {
        let _guard = gc_test_lock();
        gc::force_collect();
        let baseline = gc::stats().bytes_allocated;

        {
            let arr: Gc<GcCell<Vec<Value>>> = Gc::new(GcCell::new(Vec::new()));
            let closure = Gc::new(ClosureData {
                code_offset: 0,
                captures: vec![Value::Array(arr.clone())],
            });
            // Array holds the closure -> closure captures the array -> cycle.
            arr.borrow_mut().push(Value::Closure(closure.clone()));

            let _roots = (Value::Array(arr), Value::Closure(closure));
            assert!(
                gc::stats().bytes_allocated > baseline,
                "the live array/closure cycle must occupy GC heap"
            );
        }

        gc::force_collect();
        assert_eq!(
            gc::stats().bytes_allocated,
            baseline,
            "the dropped array/closure cycle must be reclaimed"
        );
    }

    /// The Rust mirror of `examples/cycle_stress.li`: building cyclic garbage in
    /// a loop while periodically collecting keeps live memory bounded. Without a
    /// cycle collector each self-referential array would leak and the heap would
    /// grow without bound (`per_cycle * N`); this test proves the high-water mark
    /// stays far below that projection.
    #[test]
    fn looping_cycles_stay_bounded_with_collection() {
        const ITERS: usize = 5_000;
        let _guard = gc_test_lock();

        // Measure the heap cost of a single self-referential-array cycle so we
        // can project what an unbounded leak would cost over ITERS iterations.
        gc::force_collect();
        let baseline = gc::stats().bytes_allocated;
        let one_cycle = {
            let arr: Gc<GcCell<Vec<Value>>> = Gc::new(GcCell::new(vec![Value::Int(0)]));
            arr.borrow_mut().push(Value::Array(arr.clone()));
            let live = gc::stats().bytes_allocated;
            // Keep `arr` alive until after we read the size.
            assert!(arr.borrow().len() == 2);
            live - baseline
        };
        gc::force_collect();
        assert!(one_cycle > 0, "a live cycle must occupy GC heap");
        // If the cycles leaked, the heap would grow by at least this much.
        let unbounded_projection = one_cycle * ITERS;

        // Steady-state loop: build cyclic garbage every iteration, collect
        // periodically. Track the live-heap high-water mark above baseline.
        let mut peak_over_baseline = 0usize;
        for i in 0..ITERS {
            // Self-cycle: arr -> arr.
            let arr: Gc<GcCell<Vec<Value>>> = Gc::new(GcCell::new(vec![Value::Int(i as i64)]));
            arr.borrow_mut().push(Value::Array(arr.clone()));

            // Mutual cycle: a -> b -> a.
            let a: Gc<GcCell<Vec<Value>>> = Gc::new(GcCell::new(Vec::new()));
            let b: Gc<GcCell<Vec<Value>>> = Gc::new(GcCell::new(Vec::new()));
            a.borrow_mut().push(Value::Array(b.clone()));
            b.borrow_mut().push(Value::Array(a.clone()));

            if i % 500 == 0 {
                gc::force_collect();
            }
            let over = gc::stats().bytes_allocated.saturating_sub(baseline);
            peak_over_baseline = peak_over_baseline.max(over);
        }
        gc::force_collect();

        // After the final collection the heap is back to baseline: nothing leaked.
        assert_eq!(
            gc::stats().bytes_allocated,
            baseline,
            "steady-state heap returns to baseline after collection"
        );
        // The in-flight high-water mark reflects at most the garbage built
        // between two collections, never all ITERS iterations' worth. It must be
        // a small fraction of the unbounded-leak projection.
        assert!(
            peak_over_baseline * 4 < unbounded_projection,
            "live memory stayed bounded ({peak_over_baseline} bytes peak) well \
             under the unbounded-leak projection ({unbounded_projection} bytes)"
        );
    }
}
