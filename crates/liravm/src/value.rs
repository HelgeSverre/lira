//! Lira Value Types
//!
//! Defines all value types used in the Lira virtual machine.
//!
//! Heap values that can form reference cycles (`Array`, `Tuple`, `Struct`,
//! `Object`, `Closure`)
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
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

/// Interned string type - shared reference-counted string
pub type IString = Rc<String>;

/// Fiber ID type
pub type FiberId = u64;

/// Channel ID type
pub type ChannelId = u64;

/// Heap payload for a tuple.
///
/// Tuple elements are immutable at the language and bytecode boundaries. The
/// private initializer cursor permits only the compiler's ordered `NewTuple`
/// followed by `TupleSet(0..len)` construction sequence; copied, deserialized,
/// and manually-created tuples are sealed immediately.
#[derive(Debug, Clone, Trace, Finalize)]
pub struct TupleData {
    pub(crate) elements: Vec<Value>,
    #[unsafe_ignore_trace]
    pub(crate) next_initializer: Option<usize>,
}

/// Intrinsic operations that can be used as interface witnesses without a
/// synthetic bytecode function. The enum is deliberately small: only the
/// collection operations whose receiver ABI is already part of the VM are
/// represented here.
#[derive(Debug, Clone, PartialEq, Eq, Trace, Finalize)]
pub enum InterfaceIntrinsic {
    StringLen,
    ArrayLen,
    ArrayPush,
    ArrayPop,
}

/// A bound method in an interface witness table.
#[derive(Debug, Clone, Trace, Finalize)]
pub enum InterfaceMethod {
    /// A function value. Interface invocation prepends the witness receiver.
    Value(Value),
    Intrinsic(InterfaceIntrinsic),
}

/// Runtime representation of an interface value. The receiver is retained
/// separately from the witness table so interface calls never need to expose
/// or mutate user fields used to store methods.
#[derive(Debug, Clone, Trace, Finalize)]
pub struct InterfaceData {
    pub receiver: Value,
    pub methods: HashMap<String, InterfaceMethod>,
}

impl TupleData {
    /// Construct an already-sealed tuple payload.
    pub fn sealed(elements: Vec<Value>) -> Self {
        Self {
            elements,
            next_initializer: None,
        }
    }

    pub(crate) fn initializing(elements: Vec<Value>) -> Self {
        let next_initializer = (!elements.is_empty()).then_some(0);
        Self {
            elements,
            next_initializer,
        }
    }
}

/// Value types in the VM
///
/// Cyclic-capable heap variants (`Array`, `Tuple`, `Struct`, `Object`, `Closure`) are
/// garbage-collected via [`gc::Gc`]; the tracing collector reclaims reference
/// cycles that plain reference counting cannot. `String` is skipped by the
/// tracer because `IString` (`Rc<String>`) never holds a `Gc` and so has
/// nothing for the tracer to reach; the remaining scalar variants are `Copy`
/// and trace trivially.
///
/// `Trace` and `Finalize` are implemented by hand instead of derived: the
/// derive emits a `Drop` impl that calls `gc::finalizer_safe()` (a
/// thread-local read) on every single drop. On call-heavy workloads that
/// overhead profiled at ~11% of VM runtime, and it buys nothing here —
/// `Finalize::finalize` is a no-op for `Value` (the `Gc` field finalizers are
/// empty; their real cleanup runs in `Gc::drop`), so the generated `Drop` was
/// pure overhead. Field drops (Gc unroot, Rc decrement) still run as usual.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(IString), // Interned string for memory efficiency
    Array(Gc<GcCell<Vec<Value>>>),
    /// A fixed-size, value-semantic aggregate. Unlike arrays, tuple containers
    /// are recursively copied at `CopyValue` boundaries.
    Tuple(Gc<GcCell<TupleData>>),
    /// A value-semantic aggregate. The GC cell permits nested/self-referential
    /// data without making copies of the VM's enum layout; `CopyValue` performs
    /// a recursive copy of only these nodes.
    Struct(Gc<GcCell<HashMap<String, Value>>>),
    Object(Gc<GcCell<HashMap<String, Value>>>),
    Interface(Gc<InterfaceData>),
    Function(usize),          // Code offset
    Closure(Gc<ClosureData>), // Closure with captured values
    Fiber(FiberId),           // Fiber handle
    Channel(ChannelId),       // Channel handle
}

impl Finalize for Value {}

unsafe impl Trace for Value {
    unsafe fn trace(&self) {
        match self {
            Value::Array(a) => a.trace(),
            Value::Tuple(t) => t.trace(),
            Value::Struct(s) => s.trace(),
            Value::Object(o) => o.trace(),
            Value::Interface(i) => i.trace(),
            Value::Closure(c) => c.trace(),
            _ => {}
        }
    }

    unsafe fn root(&self) {
        match self {
            Value::Array(a) => a.root(),
            Value::Tuple(t) => t.root(),
            Value::Struct(s) => s.root(),
            Value::Object(o) => o.root(),
            Value::Interface(i) => i.root(),
            Value::Closure(c) => c.root(),
            _ => {}
        }
    }

    unsafe fn unroot(&self) {
        match self {
            Value::Array(a) => a.unroot(),
            Value::Tuple(t) => t.unroot(),
            Value::Struct(s) => s.unroot(),
            Value::Object(o) => o.unroot(),
            Value::Interface(i) => i.unroot(),
            Value::Closure(c) => c.unroot(),
            _ => {}
        }
    }

    fn finalize_glue(&self) {
        Finalize::finalize(self);
        match self {
            Value::Array(a) => a.finalize_glue(),
            Value::Tuple(t) => t.finalize_glue(),
            Value::Struct(s) => s.finalize_glue(),
            Value::Object(o) => o.finalize_glue(),
            Value::Interface(i) => i.finalize_glue(),
            Value::Closure(c) => c.finalize_glue(),
            _ => {}
        }
    }
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
            Value::Tuple(_) => "tuple",
            Value::Struct(_) => "struct",
            Value::Object(_) => "object",
            Value::Interface(_) => "interface",
            Value::Function(_) => "function",
            Value::Closure(_) => "closure",
            Value::Fiber(_) => "fiber",
            Value::Channel(_) => "channel",
        }
    }

    /// Shallow value label for instruction tracing and debugger tables.
    ///
    /// Unlike [`Display`](fmt::Display), this never walks aggregate children,
    /// so enabling VM tracing cannot duplicate or render an entire user-owned
    /// collection on every instruction.
    pub(crate) fn debug_summary(&self) -> String {
        const STRING_PREVIEW_BYTES: usize = 256;

        match self {
            Self::Null => "null".to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::String(value) if value.len() <= STRING_PREVIEW_BYTES => (**value).clone(),
            Self::String(value) => {
                let mut end = STRING_PREVIEW_BYTES;
                while end > 0 && !value.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}<truncated>", &value[..end])
            }
            Self::Array(values) => format!("[{} elements]", values.borrow().len()),
            Self::Tuple(values) => format!("({} elements)", values.borrow().elements.len()),
            Self::Object(fields) | Self::Struct(fields) => {
                format!("{{{} fields}}", fields.borrow().len())
            }
            Self::Interface(interface) => {
                format!("<interface {} methods>", interface.methods.len())
            }
            Self::Function(offset) => format!("<function@{offset}>"),
            Self::Closure(closure) => format!("<closure@{}>", closure.code_offset),
            Self::Fiber(id) => format!("<fiber#{id}>"),
            Self::Channel(id) => format!("<channel#{id}>"),
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
            Value::Tuple(tuple) => !tuple.borrow().elements.is_empty(),
            Value::Struct(_) => true,
            Value::Object(_) => true,
            Value::Interface(_) => true,
            Value::Function(_) => true,
            Value::Closure(_) => true,
            Value::Fiber(_) => true,
            Value::Channel(_) => true,
        }
    }
}

impl Value {
    /// Copy a value at a language value boundary.
    ///
    /// Tuple containers and struct fields are copied recursively, while all
    /// reference-valued fields (including arrays and objects that may contain
    /// tuples or structs) retain their handles. Memo tables preserve cycles in
    /// malformed or manually constructed self-referential aggregates.
    pub fn semantic_copy(&self) -> Self {
        self.semantic_copy_with_allocations().0
    }

    /// Copy a value and report the number of newly allocated tuple/struct nodes.
    /// The VM uses this count to drive deterministic cycle collection even for
    /// a single boundary copy containing many nested structs.
    pub fn semantic_copy_with_allocations(&self) -> (Self, u64) {
        fn copy(
            value: &Value,
            struct_memo: &mut HashMap<usize, Gc<GcCell<HashMap<String, Value>>>>,
            tuple_memo: &mut HashMap<usize, Gc<GcCell<TupleData>>>,
            allocations: &mut u64,
        ) -> Value {
            match value {
                Value::Struct(source) => {
                    let identity = Gc::as_ptr(source) as usize;
                    if let Some(existing) = struct_memo.get(&identity) {
                        return Value::Struct(existing.clone());
                    }

                    let destination = Gc::new(GcCell::new(HashMap::new()));
                    struct_memo.insert(identity, destination.clone());
                    *allocations = allocations.wrapping_add(1);
                    let fields: Vec<(String, Value)> = source
                        .borrow()
                        .iter()
                        .map(|(name, field)| (name.clone(), field.clone()))
                        .collect();
                    let copied: Vec<(String, Value)> = fields
                        .iter()
                        .map(|(name, field)| {
                            (
                                name.clone(),
                                copy(field, struct_memo, tuple_memo, allocations),
                            )
                        })
                        .collect();
                    destination.borrow_mut().extend(copied);
                    Value::Struct(destination)
                }
                Value::Tuple(source) => {
                    let identity = Gc::as_ptr(source) as usize;
                    if let Some(existing) = tuple_memo.get(&identity) {
                        return Value::Tuple(existing.clone());
                    }

                    let destination = Gc::new(GcCell::new(TupleData::sealed(Vec::new())));
                    tuple_memo.insert(identity, destination.clone());
                    *allocations = allocations.wrapping_add(1);
                    let elements = source.borrow().elements.clone();
                    let copied = elements
                        .iter()
                        .map(|element| copy(element, struct_memo, tuple_memo, allocations))
                        .collect();
                    destination.borrow_mut().elements = copied;
                    Value::Tuple(destination)
                }
                _ => value.clone(),
            }
        }

        let mut allocations = 0;
        let copied = copy(
            self,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut allocations,
        );
        (copied, allocations)
    }
}

/// Keep value rendering bounded even for a malformed or deliberately
/// self-referential aggregate. This mirrors the native renderer's depth bound
/// while preserving the existing VM spellings for recursive containers.
const DISPLAY_DEPTH_LIMIT: usize = 8;

fn fmt_value(
    value: &Value,
    f: &mut fmt::Formatter<'_>,
    active: &mut HashSet<usize>,
    depth: usize,
) -> fmt::Result {
    match value {
        Value::Null => write!(f, "null"),
        Value::Bool(b) => write!(f, "{}", b),
        Value::Int(n) => write!(f, "{}", n),
        Value::Float(fl) => write!(f, "{}", fl),
        Value::String(s) => write!(f, "{}", s),
        Value::Array(arr) => {
            write!(f, "[")?;
            let identity = Gc::as_ptr(arr) as usize;
            if depth >= DISPLAY_DEPTH_LIMIT || !active.insert(identity) {
                return write!(f, "...]");
            }

            let elements = arr.borrow();
            let result = (|| {
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    fmt_value(element, f, active, depth + 1)?;
                }
                write!(f, "]")
            })();
            active.remove(&identity);
            result
        }
        Value::Tuple(tuple) => {
            write!(f, "(")?;
            let identity = Gc::as_ptr(tuple) as usize;
            if depth >= DISPLAY_DEPTH_LIMIT || !active.insert(identity) {
                return write!(f, "...)");
            }

            let tuple = tuple.borrow();
            let elements = &tuple.elements;
            let result = (|| {
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    fmt_value(element, f, active, depth + 1)?;
                }
                if elements.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            })();
            active.remove(&identity);
            result
        }
        Value::Object(obj) => {
            write!(f, "{{")?;
            let identity = Gc::as_ptr(obj) as usize;
            if depth >= DISPLAY_DEPTH_LIMIT || !active.insert(identity) {
                return write!(f, "...}}");
            }

            // HashMap iteration order is not stable. Sorting copied fields
            // keeps print/cast output deterministic and matches native maps.
            let obj = obj.borrow();
            let mut fields: Vec<(&String, &Value)> = obj.iter().collect();
            fields.sort_unstable_by_key(|(name, _)| *name);

            let result = (|| {
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: ", key)?;
                    fmt_value(value, f, active, depth + 1)?;
                }
                write!(f, "}}")
            })();
            active.remove(&identity);
            result
        }
        Value::Interface(interface) => fmt_value(&interface.receiver, f, active, depth),
        Value::Struct(obj) => {
            write!(f, "{{")?;
            let identity = Gc::as_ptr(obj) as usize;
            if depth >= DISPLAY_DEPTH_LIMIT || !active.insert(identity) {
                return write!(f, "...}}");
            }

            let obj = obj.borrow();
            let mut fields: Vec<(&String, &Value)> = obj.iter().collect();
            fields.sort_unstable_by_key(|(name, _)| *name);
            let result = (|| {
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: ", key)?;
                    fmt_value(value, f, active, depth + 1)?;
                }
                write!(f, "}}")
            })();
            active.remove(&identity);
            result
        }
        // User-facing rendering must not expose bytecode offsets or scheduler
        // ids. Those identities are backend implementation details and make
        // otherwise equivalent VM/native programs print different strings.
        // Debug summaries and the structured debug protocol retain the ids.
        Value::Function(_) | Value::Closure(_) => write!(f, "<function>"),
        Value::Fiber(_) => write!(f, "<fiber>"),
        Value::Channel(_) => write!(f, "<channel>"),
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_value(self, f, &mut HashSet::new(), 0)
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
    fn user_facing_opaque_handles_do_not_expose_backend_identities() {
        assert_eq!(Value::Function(37).to_string(), "<function>");
        assert_eq!(
            Value::Closure(Gc::new(ClosureData {
                code_offset: 91,
                captures: vec![Value::Int(1)],
            }))
            .to_string(),
            "<function>"
        );
        assert_eq!(Value::Fiber(12).to_string(), "<fiber>");
        assert_eq!(Value::Channel(18).to_string(), "<channel>");
    }

    #[test]
    fn display_renders_without_debug_wrapper() {
        // Display backs the value-rendering error sites; `3` not `Int(3)`.
        assert_eq!(Value::Int(3).to_string(), "3");
        assert_eq!(Value::String(Rc::new("hi".to_string())).to_string(), "hi");
    }

    #[test]
    fn debug_summary_is_shallow_and_bounds_utf8_strings() {
        let array = Value::Array(Gc::new(GcCell::new(vec![Value::Int(1); 300])));
        assert_eq!(array.debug_summary(), "[300 elements]");

        let text = Value::String(Rc::new("ø".repeat(200)));
        let summary = text.debug_summary();
        assert!(summary.is_char_boundary(summary.len()));
        assert!(summary.ends_with("<truncated>"));
        assert!(summary.len() <= 256 + "<truncated>".len());
    }

    #[test]
    fn display_bounds_self_referential_arrays_and_objects() {
        let array = Gc::new(GcCell::new(Vec::new()));
        array.borrow_mut().push(Value::Int(1));
        array.borrow_mut().push(Value::Array(array.clone()));
        assert_eq!(Value::Array(array).to_string(), "[1, [...]]");

        let object = Gc::new(GcCell::new(HashMap::new()));
        object
            .borrow_mut()
            .insert("self".to_string(), Value::Object(object.clone()));
        assert_eq!(Value::Object(object).to_string(), "{self: {...}}");
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
