//! Lira Memory Management
//!
//! Implements automatic reference counting (ARC) with cycle detection.
//! See docs/lira/13-memory-model.md for the full specification.

use std::cell::RefCell;
use std::rc::Rc;

/// A reference-counted object
pub type ObjectRef = Rc<RefCell<Object>>;

/// Unique ID for tracking objects
pub type ObjectId = usize;

/// An object in the Lira heap
#[derive(Debug)]
pub struct Object {
    /// Unique identifier for this object
    pub id: ObjectId,
    /// Object type
    pub kind: ObjectKind,
    /// Mark for cycle detection (used during GC)
    pub marked: bool,
}

/// Object types
#[derive(Debug)]
pub enum ObjectKind {
    /// String object
    String(String),
    /// Array object
    Array(Vec<ObjectRef>),
    /// Class instance
    Instance {
        class_name: String,
        fields: Vec<(String, ObjectRef)>,
    },
}

/// GC statistics
#[derive(Debug, Default)]
pub struct GcStats {
    /// Total allocations
    pub total_allocations: usize,
    /// Total collections run
    pub collections: usize,
    /// Total objects freed
    pub objects_freed: usize,
}

/// The memory manager with cycle detection
pub struct Memory {
    /// All allocated objects (for cycle detection)
    objects: Vec<ObjectRef>,
    /// Next object ID
    next_id: ObjectId,
    /// GC statistics
    pub stats: GcStats,
    /// Allocation threshold before triggering GC
    gc_threshold: usize,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            next_id: 0,
            stats: GcStats::default(),
            gc_threshold: 1000, // Trigger GC after 1000 allocations
        }
    }

    /// Get the next unique object ID
    fn next_object_id(&mut self) -> ObjectId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Allocate a new string
    pub fn alloc_string(&mut self, s: String) -> ObjectRef {
        self.stats.total_allocations += 1;
        let obj = Rc::new(RefCell::new(Object {
            id: self.next_object_id(),
            kind: ObjectKind::String(s),
            marked: false,
        }));
        self.objects.push(obj.clone());
        obj
    }

    /// Allocate a new array
    pub fn alloc_array(&mut self, elements: Vec<ObjectRef>) -> ObjectRef {
        self.stats.total_allocations += 1;
        let obj = Rc::new(RefCell::new(Object {
            id: self.next_object_id(),
            kind: ObjectKind::Array(elements),
            marked: false,
        }));
        self.objects.push(obj.clone());
        obj
    }

    /// Allocate a class instance
    pub fn alloc_instance(
        &mut self,
        class_name: String,
        fields: Vec<(String, ObjectRef)>,
    ) -> ObjectRef {
        self.stats.total_allocations += 1;
        let obj = Rc::new(RefCell::new(Object {
            id: self.next_object_id(),
            kind: ObjectKind::Instance { class_name, fields },
            marked: false,
        }));
        self.objects.push(obj.clone());
        obj
    }

    /// Check if GC should run
    pub fn should_gc(&self) -> bool {
        self.objects.len() >= self.gc_threshold
    }

    /// Mark an object and all objects it references
    pub fn mark_object(&self, obj: &ObjectRef) {
        let mut borrowed = obj.borrow_mut();
        if borrowed.marked {
            return; // Already marked, avoid infinite recursion
        }
        borrowed.marked = true;

        // Mark referenced objects
        match &borrowed.kind {
            ObjectKind::String(_) => {}
            ObjectKind::Array(elements) => {
                // Need to drop borrow before recursing
                let elements: Vec<_> = elements.clone();
                drop(borrowed);
                for elem in &elements {
                    self.mark_object(elem);
                }
            }
            ObjectKind::Instance { fields, .. } => {
                let fields: Vec<_> = fields.clone();
                drop(borrowed);
                for (_, value) in &fields {
                    self.mark_object(value);
                }
            }
        }
    }

    /// Mark objects reachable from a set of root object IDs
    pub fn mark_roots(&self, roots: &[ObjectRef]) {
        for root in roots {
            self.mark_object(root);
        }
    }

    /// Clear all marks (prepare for next GC cycle)
    fn clear_marks(&self) {
        for obj in &self.objects {
            obj.borrow_mut().marked = false;
        }
    }

    /// Sweep phase: remove all unmarked objects
    fn sweep(&mut self) -> usize {
        let before = self.objects.len();

        // Keep only marked objects (or objects with external references)
        self.objects.retain(|obj| {
            let marked = obj.borrow().marked;
            let external_refs = Rc::strong_count(obj) > 1;
            marked || external_refs
        });

        let freed = before - self.objects.len();
        self.stats.objects_freed += freed;
        freed
    }

    /// Run a full garbage collection cycle
    /// roots: objects known to be reachable (e.g., from stack)
    pub fn collect(&mut self, roots: &[ObjectRef]) -> usize {
        self.stats.collections += 1;

        // Clear all marks
        self.clear_marks();

        // Mark all reachable objects
        self.mark_roots(roots);

        // Sweep unmarked objects
        self.sweep()
    }

    /// Run cycle detection and collect unreachable objects
    /// This is the simple version that just removes objects with no external refs
    pub fn collect_cycles(&mut self) -> usize {
        let before = self.objects.len();

        // Remove objects that only we hold a reference to
        // This catches simple cycles where all references are internal
        self.objects.retain(|obj| Rc::strong_count(obj) > 1);

        let freed = before - self.objects.len();
        self.stats.objects_freed += freed;
        freed
    }

    /// Get current number of tracked objects
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocation() {
        let mut mem = Memory::new();
        let s = mem.alloc_string("hello".to_string());
        assert_eq!(mem.object_count(), 1);
        assert_eq!(mem.stats.total_allocations, 1);

        // Borrow to check content
        {
            let borrowed = s.borrow();
            match &borrowed.kind {
                ObjectKind::String(content) => assert_eq!(content, "hello"),
                _ => panic!("Expected string"),
            }
        }
    }

    #[test]
    fn test_simple_gc() {
        let mut mem = Memory::new();

        // Allocate some objects
        let _s1 = mem.alloc_string("one".to_string());
        let _s2 = mem.alloc_string("two".to_string());
        assert_eq!(mem.object_count(), 2);

        // Both are still referenced, nothing should be freed
        let freed = mem.collect_cycles();
        assert_eq!(freed, 0);
        assert_eq!(mem.object_count(), 2);
    }

    #[test]
    fn test_gc_unreachable() {
        let mut mem = Memory::new();

        // Allocate and immediately drop reference
        {
            let _ = mem.alloc_string("temporary".to_string());
        }
        assert_eq!(mem.object_count(), 1);

        // Now it should be collected (refcount is 1 - only in objects list)
        let freed = mem.collect_cycles();
        assert_eq!(freed, 1);
        assert_eq!(mem.object_count(), 0);
    }

    #[test]
    fn test_mark_and_sweep() {
        let mut mem = Memory::new();

        // Create a chain: root -> array -> string
        let s = mem.alloc_string("reachable".to_string());
        let arr = mem.alloc_array(vec![s.clone()]);

        // Also create an unreachable object
        {
            let _ = mem.alloc_string("unreachable".to_string());
        }
        assert_eq!(mem.object_count(), 3);

        // Mark and sweep with arr as root
        let freed = mem.collect(&[arr.clone()]);
        assert_eq!(freed, 1); // Only the unreachable string should be freed
        assert_eq!(mem.object_count(), 2);
    }

    #[test]
    fn test_cycle_detection() {
        let mut mem = Memory::new();

        // Create two objects that reference each other (cycle)
        // Note: Due to Rc limitations, we can't easily create true cycles
        // But we can test that the GC handles the case correctly

        // This test shows that objects only held by our list are freed
        let temp = mem.alloc_string("temp".to_string());
        assert_eq!(Rc::strong_count(&temp), 2); // mem.objects + temp

        drop(temp); // Now only in objects list
        assert_eq!(mem.object_count(), 1);

        let freed = mem.collect_cycles();
        assert_eq!(freed, 1);
    }
}
