//! Source-driven native coverage for structural interfaces and witness dispatch.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

fn scratch_dir(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-interfaces-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn run(source: &str, label: &str) -> Result<(String, String, String), String> {
    let dir = scratch_dir(label);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let bytecode = lirac::compile_with_imports(path.to_str().ok_or("non-UTF-8 path")?, source)?;
        let (vm_status, vm_lines) = liravm::run_with_capture(&bytecode)?;
        if vm_status != 0 {
            return Err(format!("VM exited with {vm_status}"));
        }
        let aot = common::run_aot(&path, source).map_err(|error| error.to_string())?;
        aot.assert_complete_output()?;
        if !aot.status.success() {
            return Err(format!(
                "AOT exited with {}: {}",
                aot.status,
                aot.stderr_text()
            ));
        }
        let (jit_status, jit_output) =
            common::run_jit_capture(path.to_str().ok_or("non-UTF-8 path")?, source)?;
        if jit_status != 0 {
            return Err(format!("JIT exited with {jit_status}"));
        }
        Ok((
            format!("{}\n", vm_lines.join("\n")),
            String::from_utf8(aot.stdout).map_err(|error| error.to_string())?,
            String::from_utf8(jit_output).map_err(|error| error.to_string())?,
        ))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn assert_all(source: &str, label: &str, expected: &str) {
    let (vm, aot, jit) = run(source, label).expect("interface source runs on VM/AOT/JIT");
    assert_eq!(vm, expected, "VM output");
    assert_eq!(aot, expected, "AOT output");
    assert_eq!(jit, expected, "JIT output");
}

fn assert_native(source: &str, label: &str, expected: &str) {
    let dir = scratch_dir(label);
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let path = dir.join("program.li");
    std::fs::write(&path, source).expect("write source");
    let aot = common::run_aot(&path, source).expect("AOT interface source runs");
    aot.assert_complete_output().expect("complete AOT output");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(String::from_utf8_lossy(&aot.stdout), expected);
    let (status, output) =
        common::run_jit_capture(path.to_str().expect("UTF-8 source path"), source)
            .expect("JIT interface source runs");
    assert_eq!(status, 0);
    assert_eq!(String::from_utf8_lossy(&output), expected);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn struct_interface_dispatch_works_for_local_argument_and_return() {
    assert_all(
        r#"
interface Label { fn label() -> string }
struct Item { value: string }
impl Item { fn label(self) -> string { return self.value } }
fn from_arg(value: Label) -> string { return value.label() }
fn from_return() -> Label { return Item { value: "returned" } }
let item = Item { value: "local" }
let view: Label = item
println(item.label())
println(from_arg(item))
println(from_return().label())
println(view.label())
"#,
        "local-arg-return",
        "local\nlocal\nreturned\nlocal\n",
    );
}

#[test]
fn class_override_dispatches_through_ancestor_interface() {
    assert_all(
        r#"
interface Speaker { fn speak() -> string }
class Base : Speaker { fn speak(self) -> string { return "base" } }
class Child extends Base { override fn speak(self) -> string { return "child" } }
fn say(value: Speaker) -> string { return value.speak() }
let child = Child {}
let ancestor: Base = child
println(say(ancestor))
println(ancestor.speak())
"#,
        "class-ancestor-interface",
        "child\nchild\n",
    );
}

#[test]
fn string_and_array_satisfy_has_len_structurally() {
    assert_all(
        r#"
interface HasLen { fn len() -> int }
fn size(value: HasLen) -> int { return value.len() }
let values = [1, 2, 3]
println(size("hello"))
println(size(values))
"#,
        "has-len",
        "5\n3\n",
    );
}

#[test]
fn intrinsic_interface_results_adapt_to_any_without_abi_mismatch() {
    assert_all(
        r#"
interface ErasedLen { fn len() -> any }
interface ErasedPop { fn pop() -> any }
interface Named { fn name() -> string }
interface NamedPop { fn pop() -> Named? }
struct User { label: string }
impl User { fn name(self) -> string { return self.label } }
let text: ErasedLen = "abc"
let numbers = [7]
let sized_numbers: ErasedLen = numbers
let popped_numbers: ErasedPop = numbers
let users = [User { label: "ok" }]
let popped_users: NamedPop = users
println(text.len())
println(sized_numbers.len())
println(popped_numbers.pop())
println(popped_numbers.pop())
println((popped_users.pop() ?? User { label: "none" }).name())
"#,
        "intrinsic-erased-results",
        "3\n1\n7\nnull\nok\n",
    );
}

#[test]
fn custom_interface_impl_for_int_dispatches() {
    assert_all(
        r#"
interface Bump { fn bump() -> int }
impl int { fn bump(self) -> int { return self + 1 } }
fn apply(value: Bump) -> int { return value.bump() }
println(apply(41))
println((7 as Bump).bump())
"#,
        "int-impl",
        "42\n8\n",
    );
}

#[test]
fn interface_method_aliases_keep_their_concrete_native_abi() {
    assert_all(
        r#"
type Integer = int
type Number = Integer
interface Convert { fn apply(value: Number) -> Integer }
struct Offset { amount: int }
impl Offset {
    fn apply(self, value: Number) -> Integer { return value + self.amount }
}
let converter: Convert = Offset { amount: 6 }
println(converter.apply(4))
"#,
        "interface-method-alias-abi",
        "10\n",
    );
}

#[test]
fn wide_interface_adapts_to_narrow_interface() {
    assert_all(
        r#"
interface Narrow { fn first() -> int }
interface Wide { fn second() -> int fn first() -> int }
struct Pair { value: int }
impl Pair { fn first(self) -> int { return self.value } fn second(self) -> int { return 99 } }
fn read(value: Narrow) -> int { return value.first() }
let wide: Wide = Pair { value: 7 }
let narrow: Narrow = wide
println(read(narrow))
"#,
        "wide-narrow",
        "7\n",
    );
}

#[test]
fn interface_any_round_trip_preserves_same_and_narrow_views() {
    assert_all(
        r#"
interface Narrow { fn first() -> int }
interface Wide { fn second() -> int fn first() -> int }
struct Pair { value: int }
impl Pair { fn first(self) -> int { return self.value } fn second(self) -> int { return 2 } }
let wide: Wide = Pair { value: 5 }
let erased: any = wide
let same: Wide = erased as Wide
let narrow: Narrow = erased as Narrow
println(same.first() + same.second())
println(narrow.first())
"#,
        "interface-any-roundtrip",
        "7\n5\n",
    );
}

#[test]
fn interface_alias_nested_field_and_covariant_interface_return_are_adapted() {
    assert_all(
        r#"
interface Named { fn name() -> string }
type NamedAlias = Named
struct User { value: string }
impl User { fn name(self) -> string { return self.value } }
interface Factory { fn make() -> Named }
struct UserFactory {}
impl UserFactory { fn make(self) -> User { return User { value: "made" } } }
struct Holder { value: Named }
let aliased: NamedAlias = User { value: "alias" }
let holder = Holder { value: User { value: "nested" } }
let factory: Factory = UserFactory {}
println(aliased.name())
println(holder.value.name())
println(factory.make().name())
"#,
        "alias-nested-return",
        "alias\nnested\nmade\n",
    );
}

#[test]
fn raw_any_uses_structural_interface_membership_and_casts() {
    assert_all(
        r#"
interface HasLen { fn len() -> int }
let text: any = "hello"
let numbers: any = [1, 2, 3]
let unrelated: any = 7
println(text is HasLen)
println(numbers is HasLen)
println(unrelated is HasLen)
let text_view: HasLen = text as HasLen
let numbers_view: HasLen = numbers as HasLen
println(text_view.len())
println(numbers_view.len())
"#,
        "raw-any-membership",
        "true\ntrue\nfalse\n5\n3\n",
    );
}

#[test]
fn raw_any_struct_and_concrete_class_recover_interface_witnesses() {
    assert_all(
        r#"
interface Mutate { fn mutate() -> int }
interface Speaker { fn speak() -> string }
struct Point { x: int }
impl Point {
    fn mutate(self) -> int {
        self.x = self.x + 1
        return self.x
    }
}
class Base : Speaker { fn speak(self) -> string { return "base" } }
class Child extends Base { override fn speak(self) -> string { return "child" } }
let point = Point { x: 1 }
let erased_point: any = point
let point_view: Mutate = erased_point as Mutate
let child = Child {}
let parent: Base = child
let erased_child: any = parent
let speaker: Speaker = erased_child as Speaker
println(erased_point is Mutate)
println(point_view.mutate())
println(point.x)
println(erased_child is Speaker)
println(speaker.speak())
"#,
        "raw-any-object-witnesses",
        "true\n2\n1\ntrue\nchild\n",
    );
}

#[test]
fn interface_values_inside_erased_arrays_keep_their_witnesses() {
    assert_all(
        r#"
interface Named { fn name() -> string }
struct User { label: string }
impl User { fn name(self) -> string { return self.label } }
let values: [Named] = [User { label: "nested" }]
let erased: any = values
let first: any = erased[0]
println(first is Named)
let view: Named = first as Named
println(view.name())
"#,
        "nested-interface-any-descriptor",
        "true\nnested\n",
    );
}

#[test]
fn native_raw_any_scalar_recovers_an_unambiguous_custom_witness() {
    // The VM's unboxed `any` representation does not retain custom primitive
    // implementation identity yet. Native scalar recovery is nevertheless
    // deterministic when exactly one integer-family conformer exists.
    assert_native(
        r#"
interface Bump { fn bump() -> int }
impl int { fn bump(self) -> int { return self + 1 } }
let erased: any = 41
println(erased is Bump)
let view: Bump = erased as Bump
println(view.bump())
"#,
        "raw-any-scalar-witness",
        "true\n42\n",
    );
}

#[test]
fn raw_any_array_cast_selects_the_exact_element_type_witness() {
    // The bytecode VM currently erases an array's element type when it flows
    // through `any`, so this remains native-only until that runtime metadata is
    // preserved. Native Any descriptors must still select the exact witness.
    assert_native(
        r#"
interface Marker { fn marker() -> string }
impl [int] { fn marker(self) -> string { return "int" } }
impl [string] { fn marker(self) -> string { return "string" } }
let ints: any = [1, 2]
let strings: any = ["a", "b"]
let int_view: Marker = ints as Marker
let string_view: Marker = strings as Marker
println(int_view.marker())
println(string_view.marker())
"#,
        "raw-any-array-exact-witness",
        "int\nstring\n",
    );
}

#[test]
fn struct_copy_and_array_class_references_keep_their_boundaries() {
    assert_all(
        r#"
interface Mutate { fn mutate() -> int }
struct Point { x: int }
struct Holder { values: [int] }
class Box {
    x: int
    fn mutate(self) -> int {
        self.x = self.x + 1
        return self.x
    }
}
impl Point {
    fn mutate(self) -> int {
        self.x = self.x + 1
        return self.x
    }
}
impl Holder {
    fn mutate(self) -> int {
        self.values[0] = self.values[0] + 1
        return self.values[0]
    }
}
let point = Point { x: 1 }
let point_view: Mutate = point
println(point_view.mutate())
println(point.x)
var holder = Holder { values: [3] }
let holder_view: Mutate = holder
println(holder_view.mutate())
println(holder.values[0])
let box = Box { x: 5 }
let box_view: Mutate = box
println(box_view.mutate())
println(box.x)
"#,
        "copy-reference-boundaries",
        "2\n1\n4\n4\n6\n6\n",
    );
}

#[test]
fn interface_call_uses_interface_default_not_implementation_default() {
    assert_all(
        r#"
interface Greeter { fn greet(prefix: string = "interface") -> string }
struct User {}
impl User { fn greet(self, prefix: string = "implementation") -> string { return prefix } }
let greeter: Greeter = User {}
println(greeter.greet())
println(greeter.greet(prefix: "explicit"))
"#,
        "interface-default",
        "interface\nexplicit\n",
    );
}

#[test]
fn native_interface_calls_copy_erased_any_wrappers_for_direct_and_spawned_dispatch() {
    let source = r#"
interface Grow { fn grow(value: any) -> int }
interface GrowAsync { fn grow(value: any, output: Channel<int>) -> void }
struct Worker {}
impl Worker {
    fn grow(self, value: any) -> int {
        push(value, "widened")
        return len(value)
    }
}
struct AsyncWorker {}
impl AsyncWorker {
    fn grow(self, value: any, output: Channel<int>) {
        push(value, "widened")
        send(output, len(value))
    }
}
let direct_value: any = [1]
let direct: Grow = Worker {}
println(direct.grow(direct_value))
println(len(direct_value))
let spawned_value: any = [2]
let output: Channel<int> = chan(1)
let async_worker: GrowAsync = AsyncWorker {}
spawn async_worker.grow(spawned_value, output)
println(recv(output))
println(len(spawned_value))
"#;
    let (vm, aot, jit) = run(source, "interface-any-call-boundary")
        .expect("interface Any call boundary runs on VM/AOT/JIT");
    // The bytecode VM erases the array element descriptor inside `any`, so a
    // widening push mutates the shared tagged array. Native code retains the
    // descriptor and copies the Any wrapper at a call boundary, matching its
    // ordinary-function call semantics without mutating the caller wrapper.
    assert_eq!(vm, "2\n2\n2\n2\n");
    assert_eq!(aot, "2\n1\n2\n1\n");
    assert_eq!(jit, "2\n1\n2\n1\n");
}

#[test]
fn bounded_interface_allocation_and_collect_churn_completes() {
    assert_all(
        r#"
interface Value { fn value() -> int }
struct Item { n: int }
impl Item { fn value(self) -> int { return self.n } }
var total = 0
var i = 0
while i < 8 {
    let erased: any = (Item { n: i } as Value)
    let current: Value = erased as Value
    total = total + current.value()
    collect()
    i = i + 1
}
println(total)
"#,
        "allocation-churn",
        "28\n",
    );
}

#[test]
fn spawned_function_accepts_and_dispatches_interface_argument() {
    assert_all(
        r#"
interface Job { fn run() -> int }
struct Work { n: int }
impl Work { fn run(self) -> int { return self.n } }
fn worker(job: Job, output: Channel<int>) { send(output, job.run()) }
let output: Channel<int> = chan(1)
spawn worker(Work { n: 9 }, output)
println(recv(output))
"#,
        "spawn-interface",
        "9\n",
    );
}

#[test]
fn spawned_interface_method_dispatches_on_the_child_fiber() {
    assert_all(
        r#"
interface Job { fn run(output: Channel<int>) -> void }
struct Work { n: int }
impl Work {
    fn run(self, output: Channel<int>) {
        send(output, self.n)
    }
}
let output: Channel<int> = chan(1)
let job: Job = Work { n: 11 }
spawn job.run(output)
println(recv(output))
"#,
        "spawn-interface-method",
        "11\n",
    );
}

// This case checks native runtime membership after interface values are erased;
// it remains native-only while the VM's exact interface descriptor support is
// being brought to parity.
#[test]
fn native_only_interface_any_is_checks_retain_exact_descriptors() {
    let source = r#"
interface Narrow { fn first() -> int }
interface Wide { fn first() -> int fn second() -> int }
struct Pair { value: int }
impl Pair { fn first(self) -> int { return self.value } fn second(self) -> int { return 2 } }
let wide: Wide = Pair { value: 5 }
let erased: any = wide
println(erased is Wide)
println(erased is Narrow)
println(erased is string)
"#;
    assert_native(source, "native-interface-is", "true\ntrue\nfalse\n");
}
