# Lira Memory Model Specification

## Document Information

| Property | Value |
|----------|-------|
| **Document ID** | 13-memory-model |
| **Version** | 1.0.0-draft |
| **Status** | Draft Specification |
| **Prerequisites** | 12-vm-runtime |

---

## Table of Contents

1. [Overview](#1-overview)
2. [Reference Counting](#2-reference-counting)
3. [Cycle Detection](#3-cycle-detection)
4. [Heap Organization](#4-heap-organization)
5. [Object Lifecycle](#5-object-lifecycle)
6. [Memory Safety](#6-memory-safety)
7. [Optimization Techniques](#7-optimization-techniques)

---

## 1. Overview

### 1.1 Memory Management Strategy

Lira uses **Automatic Reference Counting (ARC)** with **cycle detection** for memory management:

```
┌─────────────────────────────────────────────────────────────────┐
│                  MEMORY MANAGEMENT OVERVIEW                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │              PRIMARY: Reference Counting                 │    │
│  │  - Immediate reclamation when refcount → 0              │    │
│  │  - Deterministic destruction                            │    │
│  │  - Low overhead for common cases                        │    │
│  └─────────────────────────────────────────────────────────┘    │
│                           │                                      │
│                           ▼                                      │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │              SECONDARY: Cycle Detection                  │    │
│  │  - Periodic scanning for unreachable cycles            │    │
│  │  - Trial deletion algorithm                             │    │
│  │  - Only for potentially cyclic objects                  │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 Design Goals

1. **Deterministic**: Objects freed immediately when unreferenced
2. **Efficient**: Minimal overhead for non-cyclic structures
3. **Safe**: No dangling pointers or use-after-free
4. **Concurrent**: Compatible with fiber scheduling

### 1.3 Memory Regions

```
┌─────────────────────────────────────────────────────────────────┐
│                      VM MEMORY LAYOUT                            │
├─────────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    CODE SEGMENT                            │  │
│  │  - Read-only bytecode                                     │  │
│  │  - Constant pool                                          │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   STACK SEGMENT                            │  │
│  │  - Per-fiber value stacks                                 │  │
│  │  - Call frames                                            │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    HEAP SEGMENT                            │  │
│  │  - Dynamically allocated objects                          │  │
│  │  - Managed by reference counting                          │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                 INTERNED STRINGS                           │  │
│  │  - Deduplicated string literals                           │  │
│  │  - Never freed (permanent)                                │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. Reference Counting

### 2.1 Reference Count Storage

Every heap object has a reference count in its header:

```rust
#[repr(C)]
struct ObjectHeader {
    /// Reference count (63 bits) + GC flags (1 bit)
    refcount_flags: u64,

    /// Type information
    type_info: *const TypeInfo,
}

impl ObjectHeader {
    const REFCOUNT_SHIFT: u32 = 1;
    const FLAG_MASK: u64 = 0x1;

    /// Get current reference count
    #[inline]
    pub fn refcount(&self) -> u64 {
        self.refcount_flags >> Self::REFCOUNT_SHIFT
    }

    /// Increment reference count
    #[inline]
    pub fn inc_ref(&mut self) {
        // Overflow check in debug mode
        debug_assert!(self.refcount() < u64::MAX >> 1);
        self.refcount_flags += 1 << Self::REFCOUNT_SHIFT;
    }

    /// Decrement reference count, returns true if now zero
    #[inline]
    pub fn dec_ref(&mut self) -> bool {
        debug_assert!(self.refcount() > 0);
        self.refcount_flags -= 1 << Self::REFCOUNT_SHIFT;
        self.refcount() == 0
    }
}
```

### 2.2 Reference Count Operations

```rust
impl VM {
    /// Increment reference count for a value
    #[inline]
    pub fn inc_ref(&mut self, value: Value) {
        if value.is_pointer() && !value.is_null() {
            let obj = unsafe { &mut *value.as_pointer::<ObjectHeader>() };
            obj.inc_ref();
        }
    }

    /// Decrement reference count and free if zero
    pub fn dec_ref(&mut self, value: Value) -> Result<(), VMError> {
        if !value.is_pointer() || value.is_null() {
            return Ok(());
        }

        let obj = unsafe { &mut *value.as_pointer::<ObjectHeader>() };

        if obj.dec_ref() {
            // Reference count reached zero - free the object
            self.free_object(value)?;
        }

        Ok(())
    }

    /// Free an object and decrement references to children
    fn free_object(&mut self, value: Value) -> Result<(), VMError> {
        let obj_ptr = value.as_pointer::<ObjectHeader>();
        let obj = unsafe { &*obj_ptr };
        let type_info = unsafe { &*obj.type_info };

        // Call destructor if present
        if let Some(destructor) = type_info.destructor {
            destructor(self, value);
        }

        // Decrement references to child objects
        self.trace_children(value, |child| {
            self.dec_ref(child)
        })?;

        // Return memory to heap
        self.heap.free(obj_ptr as *mut u8);

        Ok(())
    }

    /// Trace all reference fields of an object
    fn trace_children<F>(&mut self, value: Value, mut visitor: F) -> Result<(), VMError>
    where
        F: FnMut(Value) -> Result<(), VMError>
    {
        let obj = unsafe { &*value.as_pointer::<ObjectHeader>() };
        let type_info = unsafe { &*obj.type_info };

        // Get reference field offsets from type info
        let offsets = unsafe {
            slice::from_raw_parts(
                type_info.ref_field_offsets,
                type_info.ref_field_count as usize
            )
        };

        let obj_data = unsafe {
            (value.as_pointer::<u8>()).add(size_of::<ObjectHeader>())
        };

        for &offset in offsets {
            let field_ptr = unsafe { obj_data.add(offset as usize) as *const Value };
            let field_value = unsafe { *field_ptr };
            visitor(field_value)?;
        }

        Ok(())
    }
}
```

### 2.3 Assignment Operations

Reference counting requires careful handling of assignments:

```rust
impl VM {
    /// Assign value to a slot (local, field, or array element)
    pub fn assign(&mut self, slot: &mut Value, new_value: Value) -> Result<(), VMError> {
        // Order matters: increment new first, then decrement old
        // This handles self-assignment correctly
        self.inc_ref(new_value);

        let old_value = *slot;
        *slot = new_value;

        self.dec_ref(old_value)?;

        Ok(())
    }

    /// Move value without changing reference counts
    #[inline]
    pub fn move_value(&mut self, slot: &mut Value) -> Value {
        let value = *slot;
        *slot = Value::null();
        value
    }
}
```

### 2.4 Compiler-Generated Reference Counting

The compiler inserts reference count operations:

```li
// Source code
fn example(obj: MyClass) {
    let local = obj          // inc_ref(obj)
    process(local)           // inc_ref(local) before call
    // dec_ref(local) at end of scope
}
// dec_ref(obj) after return
```

Bytecode:
```
; Function entry - obj already has +1 ref from caller
LOAD_0              ; Load obj
INC_REF             ; Increment for local
STORE_1             ; Store in local

LOAD_1              ; Load local
INC_REF             ; Increment for call arg
INVOKE process

; Before return
LOAD_1
DEC_REF             ; local goes out of scope

RETURN_VOID
```

---

## 3. Cycle Detection

### 3.1 The Problem

Reference counting alone cannot handle cycles:

```
┌─────────────────────────────────────────────────────────────────┐
│                    REFERENCE CYCLE                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│     ┌─────────────┐         ┌─────────────┐                     │
│     │  Object A   │────────▶│  Object B   │                     │
│     │  refcount=1 │         │  refcount=1 │                     │
│     └─────────────┘         └─────────────┘                     │
│           ▲                        │                            │
│           │                        │                            │
│           └────────────────────────┘                            │
│                                                                  │
│  Both objects have refcount=1 but are unreachable!              │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Trial Deletion Algorithm

Lira uses the "trial deletion" algorithm (Lins' algorithm):

```rust
/// Cycle detection state
struct CycleCollector {
    /// Objects potentially in cycles
    candidates: Vec<*mut ObjectHeader>,

    /// Deferred decrements
    deferred: Vec<*mut ObjectHeader>,

    /// Currently scanning
    scanning: bool,

    /// Collection threshold
    threshold: usize,
}

impl CycleCollector {
    pub fn new(threshold: usize) -> Self {
        Self {
            candidates: Vec::new(),
            deferred: Vec::new(),
            scanning: false,
            threshold,
        }
    }

    /// Mark object as potential cycle member
    pub fn add_candidate(&mut self, obj: *mut ObjectHeader) {
        if !self.scanning {
            self.candidates.push(obj);
        }
    }

    /// Run cycle collection if needed
    pub fn maybe_collect(&mut self, vm: &mut VM) {
        if self.candidates.len() >= self.threshold {
            self.collect(vm);
        }
    }

    /// Full cycle collection
    pub fn collect(&mut self, vm: &mut VM) {
        self.scanning = true;

        // Phase 1: Trial decrement
        self.trial_decrement(vm);

        // Phase 2: Find roots
        let roots = self.find_roots();

        // Phase 3: Restore live objects
        self.restore(&roots, vm);

        // Phase 4: Free garbage cycles
        self.free_garbage(vm);

        self.candidates.clear();
        self.scanning = false;
    }
}
```

### 3.3 Phase 1: Trial Decrement

Simulate decrements to find potential garbage:

```rust
impl CycleCollector {
    fn trial_decrement(&mut self, vm: &mut VM) {
        for &obj_ptr in &self.candidates {
            let obj = unsafe { &mut *obj_ptr };

            // Mark as being traced
            obj.set_color(GCColor::Gray);

            // Trial decrement children
            vm.trace_children_raw(obj_ptr, |child_ptr| {
                if !child_ptr.is_null() {
                    let child = unsafe { &mut *child_ptr };

                    // Decrement trial count
                    child.trial_dec();

                    // If not already candidate, add it
                    if child.color() == GCColor::White {
                        child.set_color(GCColor::Gray);
                        self.candidates.push(child_ptr);
                    }
                }
            });
        }
    }
}
```

### 3.4 Phase 2: Find Roots

Objects with external references are roots:

```rust
impl CycleCollector {
    fn find_roots(&self) -> Vec<*mut ObjectHeader> {
        let mut roots = Vec::new();

        for &obj_ptr in &self.candidates {
            let obj = unsafe { &*obj_ptr };

            // Object has external references if:
            // actual_refcount > trial_decremented_count
            if obj.refcount() > obj.trial_count() {
                roots.push(obj_ptr);
            }
        }

        roots
    }
}
```

### 3.5 Phase 3: Restore Live Objects

Mark all objects reachable from roots as live:

```rust
impl CycleCollector {
    fn restore(&self, roots: &[*mut ObjectHeader], vm: &mut VM) {
        let mut worklist: VecDeque<*mut ObjectHeader> = roots.iter().copied().collect();

        while let Some(obj_ptr) = worklist.pop_front() {
            let obj = unsafe { &mut *obj_ptr };

            // Already restored?
            if obj.color() == GCColor::Black {
                continue;
            }

            // Mark as live
            obj.set_color(GCColor::Black);

            // Restore trial count
            obj.restore_refcount();

            // Process children
            vm.trace_children_raw(obj_ptr, |child_ptr| {
                if !child_ptr.is_null() {
                    let child = unsafe { &*child_ptr };
                    if child.color() == GCColor::Gray {
                        worklist.push_back(child_ptr);
                    }
                }
            });
        }
    }
}
```

### 3.6 Phase 4: Free Garbage

Objects still gray after restoration are garbage:

```rust
impl CycleCollector {
    fn free_garbage(&mut self, vm: &mut VM) {
        // Collect garbage objects
        let garbage: Vec<_> = self.candidates
            .iter()
            .filter(|&&obj_ptr| {
                let obj = unsafe { &*obj_ptr };
                obj.color() == GCColor::Gray
            })
            .copied()
            .collect();

        // Free in reverse allocation order (children before parents)
        for obj_ptr in garbage.into_iter().rev() {
            let obj = unsafe { &mut *obj_ptr };

            // Clear references to avoid double-free
            vm.clear_children_raw(obj_ptr);

            // Free memory
            vm.heap.free(obj_ptr as *mut u8);
        }
    }
}
```

### 3.7 When to Trigger Collection

```rust
impl VM {
    /// Called when refcount decremented
    fn on_dec_ref(&mut self, obj: *mut ObjectHeader) {
        let obj_ref = unsafe { &*obj };

        // Potential cycle if:
        // 1. Object has reference fields
        // 2. Refcount didn't reach zero
        if obj_ref.type_info().ref_field_count > 0 && obj_ref.refcount() > 0 {
            self.cycle_collector.add_candidate(obj);
        }

        // Check if collection needed
        self.cycle_collector.maybe_collect(self);
    }
}
```

---

## 4. Heap Organization

### 4.1 Heap Structure

```rust
struct Heap {
    /// Memory blocks by size class
    size_classes: [SizeClass; NUM_SIZE_CLASSES],

    /// Large object space
    large_objects: LargeObjectSpace,

    /// Total allocated bytes
    allocated: usize,

    /// Maximum heap size
    max_size: usize,

    /// Statistics
    stats: HeapStats,
}

/// Size classes for small objects
const SIZE_CLASSES: [usize; NUM_SIZE_CLASSES] = [
    16, 32, 48, 64, 80, 96, 112, 128,
    192, 256, 384, 512, 768, 1024,
    1536, 2048, 3072, 4096,
];

const NUM_SIZE_CLASSES: usize = 18;
const LARGE_OBJECT_THRESHOLD: usize = 4096;
```

### 4.2 Size Class Allocator

```rust
struct SizeClass {
    /// Object size for this class
    object_size: usize,

    /// Free list head
    free_list: *mut FreeBlock,

    /// Allocated blocks
    blocks: Vec<*mut Block>,

    /// Block size
    block_size: usize,
}

#[repr(C)]
struct FreeBlock {
    next: *mut FreeBlock,
}

#[repr(C)]
struct Block {
    /// Size class index
    size_class: u8,
    /// Number of allocated objects
    allocated: u16,
    /// Total capacity
    capacity: u16,
    /// Bitmap for allocated slots
    bitmap: [u64; 8],
    /// Object data follows
    data: [u8; 0],
}

impl SizeClass {
    pub fn alloc(&mut self) -> *mut u8 {
        // Try free list first
        if !self.free_list.is_null() {
            let block = self.free_list;
            self.free_list = unsafe { (*block).next };
            return block as *mut u8;
        }

        // Allocate from current block or get new block
        self.alloc_from_block()
    }

    pub fn free(&mut self, ptr: *mut u8) {
        // Add to free list
        let block = ptr as *mut FreeBlock;
        unsafe {
            (*block).next = self.free_list;
        }
        self.free_list = block;
    }

    fn alloc_from_block(&mut self) -> *mut u8 {
        // Get current block or allocate new one
        let block = self.current_block();

        // Find free slot in bitmap
        for (i, &bits) in unsafe { (*block).bitmap.iter().enumerate() } {
            if bits != !0u64 {
                let slot = bits.trailing_ones() as usize;
                let index = i * 64 + slot;

                if index < unsafe { (*block).capacity as usize } {
                    // Mark as allocated
                    unsafe {
                        (*block).bitmap[i] |= 1 << slot;
                        (*block).allocated += 1;
                    }

                    // Calculate pointer
                    let offset = index * self.object_size;
                    return unsafe { (*block).data.as_mut_ptr().add(offset) };
                }
            }
        }

        // Block full - allocate new one
        self.alloc_new_block();
        self.alloc_from_block()
    }
}
```

### 4.3 Large Object Space

```rust
struct LargeObjectSpace {
    /// Allocated large objects
    objects: HashMap<*mut u8, LargeObject>,

    /// Total allocated size
    allocated: usize,
}

struct LargeObject {
    ptr: *mut u8,
    size: usize,
}

impl LargeObjectSpace {
    pub fn alloc(&mut self, size: usize) -> *mut u8 {
        // Align to page boundary
        let aligned_size = (size + 4095) & !4095;

        // Use syscall for large allocation (anonymous private mapping)
        // sys_mmap(addr, size, prot, flags, fd, offset)
        let ptr = unsafe {
            syscall6(
                SYS_MMAP,
                0,                                    // addr: kernel chooses
                aligned_size,                         // size
                (PROT_READ | PROT_WRITE) as usize,   // prot
                (MAP_PRIVATE | MAP_ANONYMOUS) as usize, // flags
                (-1_isize) as usize,                 // fd: -1 for anonymous
                0,                                    // offset
            ) as *mut u8
        };

        if ptr.is_null() || (ptr as isize) < 0 {
            panic!("Out of memory");
        }

        self.objects.insert(ptr, LargeObject { ptr, size: aligned_size });
        self.allocated += aligned_size;

        ptr
    }

    pub fn free(&mut self, ptr: *mut u8) {
        if let Some(obj) = self.objects.remove(&ptr) {
            unsafe {
                syscall2(SYS_MUNMAP, ptr as usize, obj.size);
            }
            self.allocated -= obj.size;
        }
    }
}
```

### 4.4 Heap Allocation

```rust
impl Heap {
    pub fn alloc(&mut self, size: usize, type_info: &TypeInfo) -> *mut u8 {
        // Include header size
        let total_size = size_of::<ObjectHeader>() + size;

        // Check heap limit
        if self.allocated + total_size > self.max_size {
            // Try to collect cycles first
            self.cycle_collector.collect();

            if self.allocated + total_size > self.max_size {
                panic!("Out of memory");
            }
        }

        let ptr = if total_size <= LARGE_OBJECT_THRESHOLD {
            // Use size class allocator
            let class_idx = self.find_size_class(total_size);
            self.size_classes[class_idx].alloc()
        } else {
            // Use large object space
            self.large_objects.alloc(total_size)
        };

        // Initialize header
        let header = ptr as *mut ObjectHeader;
        unsafe {
            (*header).refcount_flags = 1 << 1; // refcount = 1
            (*header).type_info = type_info;
        }

        self.allocated += total_size;
        self.stats.allocations += 1;

        ptr
    }

    pub fn free(&mut self, ptr: *mut u8) {
        let header = ptr as *mut ObjectHeader;
        let type_info = unsafe { (*header).type_info };
        let size = size_of::<ObjectHeader>() + unsafe { (*type_info).instance_size as usize };

        if size <= LARGE_OBJECT_THRESHOLD {
            let class_idx = self.find_size_class(size);
            self.size_classes[class_idx].free(ptr);
        } else {
            self.large_objects.free(ptr);
        }

        self.allocated -= size;
        self.stats.frees += 1;
    }

    fn find_size_class(&self, size: usize) -> usize {
        SIZE_CLASSES.iter()
            .position(|&s| s >= size)
            .unwrap_or(NUM_SIZE_CLASSES - 1)
    }
}
```

---

## 5. Object Lifecycle

### 5.1 Object States

```
┌─────────────────────────────────────────────────────────────────┐
│                    OBJECT LIFECYCLE                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────┐                                                   │
│  │ Allocate │                                                   │
│  └────┬─────┘                                                   │
│       ▼                                                         │
│  ┌──────────┐     inc_ref     ┌──────────┐                     │
│  │  Active  │◀───────────────▶│  Active  │                     │
│  │ refcount │     dec_ref     │ refcount │                     │
│  │   = 1    │◀───────────────▶│   > 1    │                     │
│  └────┬─────┘                 └──────────┘                     │
│       │ dec_ref (refcount → 0)                                  │
│       ▼                                                         │
│  ┌──────────┐                                                   │
│  │  Dying   │  Call destructor                                 │
│  └────┬─────┘                                                   │
│       ▼                                                         │
│  ┌──────────┐                                                   │
│  │  Free    │  Return memory to heap                           │
│  └──────────┘                                                   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 5.2 Constructor Semantics

```rust
impl VM {
    /// Allocate and initialize object
    pub fn new_object(&mut self, type_idx: usize, args: &[Value]) -> Result<Value, VMError> {
        let type_info = self.types.get(type_idx);

        // Allocate memory
        let ptr = self.heap.alloc(type_info.instance_size as usize, type_info);

        // Zero-initialize fields
        let data = unsafe {
            slice::from_raw_parts_mut(
                (ptr as *mut u8).add(size_of::<ObjectHeader>()),
                type_info.instance_size as usize
            )
        };
        data.fill(0);

        let value = Value::from_pointer(ptr);

        // Call constructor if present
        if let Some(ctor_idx) = type_info.constructor {
            // Push object as first argument
            self.stack.push(value)?;

            // Push other arguments
            for &arg in args {
                self.inc_ref(arg);
                self.stack.push(arg)?;
            }

            // Call constructor
            self.invoke(ctor_idx)?;

            // Constructor doesn't return value, object already on stack
        }

        Ok(value)
    }
}
```

### 5.3 Destructor Semantics

```rust
/// Destructor function type
type Destructor = fn(&mut VM, Value);

impl VM {
    /// Free object, calling destructor
    fn free_object(&mut self, value: Value) -> Result<(), VMError> {
        let obj_ptr = value.as_pointer::<ObjectHeader>();
        let type_info = unsafe { &*(*obj_ptr).type_info };

        // Call destructor if present
        if let Some(destructor) = type_info.destructor {
            destructor(self, value);
        }

        // Decrement references to children
        self.trace_children(value, |child| {
            self.dec_ref(child)
        })?;

        // Free memory
        self.heap.free(obj_ptr as *mut u8);

        Ok(())
    }
}
```

### 5.4 Weak References

```rust
#[repr(C)]
struct WeakRef {
    header: ObjectHeader,
    /// Target object (not counted in refcount)
    target: Value,
    /// Link to next weak ref for same target
    next: *mut WeakRef,
}

impl VM {
    /// Create weak reference
    pub fn create_weak_ref(&mut self, target: Value) -> Value {
        if target.is_null() {
            return Value::null();
        }

        let weak = self.heap.alloc(size_of::<WeakRef>(), &WEAK_REF_TYPE);
        let weak_ref = unsafe { &mut *weak.cast::<WeakRef>() };

        weak_ref.target = target;

        // Add to target's weak reference list
        let target_obj = unsafe { &mut *target.as_pointer::<ObjectHeader>() };
        weak_ref.next = target_obj.weak_refs;
        target_obj.weak_refs = weak as *mut WeakRef;

        Value::from_pointer(weak)
    }

    /// Dereference weak reference
    pub fn weak_deref(&self, weak: Value) -> Value {
        if weak.is_null() {
            return Value::null();
        }

        let weak_ref = unsafe { &*weak.as_pointer::<WeakRef>() };
        weak_ref.target // May be null if target was freed
    }

    /// Called when object is about to be freed
    fn clear_weak_refs(&mut self, obj: *mut ObjectHeader) {
        let header = unsafe { &mut *obj };
        let mut weak = header.weak_refs;

        while !weak.is_null() {
            let weak_ref = unsafe { &mut *weak };
            weak_ref.target = Value::null();
            weak = weak_ref.next;
        }

        header.weak_refs = ptr::null_mut();
    }
}
```

---

## 6. Memory Safety

### 6.1 Bounds Checking

```rust
impl ArrayObject {
    #[inline]
    pub fn get_checked(&self, index: usize) -> Result<Value, VMError> {
        if index >= self.len as usize {
            return Err(VMError::IndexOutOfBounds {
                index,
                length: self.len as usize,
            });
        }
        Ok(unsafe { *self.data.as_ptr().add(index) })
    }

    #[inline]
    pub fn set_checked(&mut self, index: usize, value: Value) -> Result<(), VMError> {
        if index >= self.len as usize {
            return Err(VMError::IndexOutOfBounds {
                index,
                length: self.len as usize,
            });
        }
        unsafe { *self.data.as_mut_ptr().add(index) = value };
        Ok(())
    }
}
```

### 6.2 Null Pointer Checking

```rust
impl VM {
    #[inline]
    fn get_field_safe(&self, obj: Value, field_idx: usize) -> Result<Value, VMError> {
        if obj.is_null() {
            return Err(VMError::NullPointer);
        }

        let obj_ptr = obj.as_pointer::<ObjectHeader>();
        let type_info = unsafe { &*(*obj_ptr).type_info };

        if field_idx >= type_info.field_count as usize {
            return Err(VMError::InvalidField);
        }

        let field_offset = unsafe { *type_info.field_offsets.add(field_idx) } as usize;
        let field_ptr = unsafe {
            (obj_ptr as *const u8)
                .add(size_of::<ObjectHeader>())
                .add(field_offset) as *const Value
        };

        Ok(unsafe { *field_ptr })
    }
}
```

### 6.3 Type Safety

```rust
impl VM {
    fn checkcast(&self, value: Value, target_type: *const TypeInfo) -> Result<Value, VMError> {
        if value.is_null() {
            return Ok(value); // null is compatible with any type
        }

        let obj = unsafe { &*value.as_pointer::<ObjectHeader>() };
        let value_type = obj.type_info;

        if !self.is_subtype(value_type, target_type) {
            return Err(VMError::TypeMismatch {
                expected: unsafe { (*target_type).tag },
                found: unsafe { (*value_type).tag },
            });
        }

        Ok(value)
    }

    fn is_subtype(&self, sub: *const TypeInfo, super_: *const TypeInfo) -> bool {
        let mut current = sub;

        while !current.is_null() {
            if current == super_ {
                return true;
            }

            // Check interfaces
            let type_info = unsafe { &*current };
            for i in 0..type_info.interface_count as usize {
                let interface = unsafe { *type_info.interfaces.add(i) };
                if interface == super_ {
                    return true;
                }
            }

            current = type_info.parent;
        }

        false
    }
}
```

---

## 7. Optimization Techniques

### 7.1 Eliding Reference Counts

The compiler can elide reference count operations in certain cases:

```li
// Source
fn process(items: List<Item>) {
    for item in items {
        // item is borrowed, no inc/dec needed
        print(item.name)
    }
}
```

Safe elision conditions:
1. **Temporary values**: Values that don't escape current expression
2. **Loop iterators**: Values bound to loop variables with known scope
3. **Known-live parents**: Child references while parent is live

### 7.2 Deferred Reference Counting

Batch reference count updates:

```rust
struct DeferredRefCounts {
    increments: Vec<*mut ObjectHeader>,
    decrements: Vec<*mut ObjectHeader>,
}

impl DeferredRefCounts {
    pub fn flush(&mut self, vm: &mut VM) {
        // Process increments first (prevents premature free)
        for obj in self.increments.drain(..) {
            unsafe { (*obj).inc_ref() };
        }

        // Then decrements
        for obj in self.decrements.drain(..) {
            if unsafe { (*obj).dec_ref() } {
                vm.free_object_raw(obj);
            }
        }
    }
}
```

### 7.3 Immortal Objects

Some objects never need reference counting:

```rust
impl ObjectHeader {
    const IMMORTAL_REFCOUNT: u64 = u64::MAX >> 1;

    pub fn make_immortal(&mut self) {
        self.refcount_flags = Self::IMMORTAL_REFCOUNT << 1;
    }

    pub fn is_immortal(&self) -> bool {
        self.refcount() >= Self::IMMORTAL_REFCOUNT
    }
}

// Interned strings are immortal
impl StringPool {
    pub fn intern(&mut self, vm: &mut VM, s: &str) -> Value {
        // ... create string ...
        let obj = unsafe { &mut *string.as_pointer::<ObjectHeader>() };
        obj.make_immortal();
        // ...
    }
}
```

### 7.4 Thread-Local Heaps

For multi-threaded execution (future):

```rust
struct ThreadLocalHeap {
    /// Local allocation buffer
    lab: LocalAllocationBuffer,

    /// Local free list
    local_free: [*mut FreeBlock; NUM_SIZE_CLASSES],

    /// Shared heap reference
    shared: Arc<Mutex<Heap>>,
}

impl ThreadLocalHeap {
    pub fn alloc(&mut self, size: usize) -> *mut u8 {
        // Try local first
        if let Some(ptr) = self.lab.try_alloc(size) {
            return ptr;
        }

        // Try local free list
        let class = find_size_class(size);
        if !self.local_free[class].is_null() {
            let ptr = self.local_free[class];
            self.local_free[class] = unsafe { (*ptr).next };
            return ptr as *mut u8;
        }

        // Fall back to shared heap
        let mut heap = self.shared.lock();
        heap.alloc(size)
    }
}
```

---

## Appendix A: Memory Statistics

```rust
struct HeapStats {
    /// Total allocations
    allocations: u64,

    /// Total frees
    frees: u64,

    /// Current allocated bytes
    allocated_bytes: usize,

    /// Peak allocated bytes
    peak_bytes: usize,

    /// Cycle collections performed
    cycle_collections: u64,

    /// Objects freed by cycle collection
    cycle_freed: u64,
}

impl VM {
    pub fn memory_stats(&self) -> HeapStats {
        self.heap.stats.clone()
    }

    pub fn print_memory_stats(&self) {
        let stats = self.memory_stats();
        println!("=== Memory Statistics ===");
        println!("Allocations:        {}", stats.allocations);
        println!("Frees:              {}", stats.frees);
        println!("Currently allocated: {} bytes", stats.allocated_bytes);
        println!("Peak allocation:    {} bytes", stats.peak_bytes);
        println!("Cycle collections:  {}", stats.cycle_collections);
        println!("Cycles freed:       {}", stats.cycle_freed);
    }
}
```

---

## Appendix B: Debugging Memory Issues

### B.1 Leak Detection

```rust
impl VM {
    #[cfg(debug_assertions)]
    pub fn check_for_leaks(&self) {
        let mut leaks = Vec::new();

        // Check all allocated objects
        for block in &self.heap.size_classes {
            for obj in block.iter_allocated() {
                let header = unsafe { &*obj.cast::<ObjectHeader>() };
                if header.refcount() == 0 {
                    leaks.push(obj);
                }
            }
        }

        if !leaks.is_empty() {
            eprintln!("WARNING: {} potential memory leaks detected", leaks.len());
            for leak in &leaks {
                let header = unsafe { &**leak.cast::<ObjectHeader>() };
                let type_info = unsafe { &*header.type_info };
                eprintln!("  - {:?} ({:?})", leak, type_info.name());
            }
        }
    }
}
```

### B.2 Reference Count Debugging

```rust
#[cfg(debug_assertions)]
impl ObjectHeader {
    pub fn debug_inc_ref(&mut self, location: &'static str) {
        let old = self.refcount();
        self.inc_ref();
        eprintln!(
            "INC_REF: {:p} {} -> {} at {}",
            self, old, self.refcount(), location
        );
    }

    pub fn debug_dec_ref(&mut self, location: &'static str) -> bool {
        let old = self.refcount();
        let result = self.dec_ref();
        eprintln!(
            "DEC_REF: {:p} {} -> {} at {} (zero={})",
            self, old, self.refcount(), location, result
        );
        result
    }
}
```

---

*This document is part of the Lira Language Specification.*
