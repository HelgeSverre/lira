//! End-to-end interface witnesses through the normal compiler and VM pipeline.

fn run(source: &str) -> Vec<String> {
    let bytecode = lirac::compile(source).expect("interface source should compile");
    let (status, output) =
        liravm::run_with_capture(&bytecode).expect("interface bytecode should execute");
    assert_eq!(status, 0);
    output
}

#[test]
fn struct_interface_argument_return_and_dispatch() {
    let output = run(r#"
interface Named { fn name() -> string }
struct User { fn name(self) -> string { return "user" } }
fn identity(value: Named) -> Named { return value }
let named: Named = User {}
println(identity(named).name())
"#);
    assert_eq!(output, vec!["user"]);
}

#[test]
fn interface_round_trip_and_structural_is() {
    let output = run(r#"
interface Named { fn name() -> string }
struct User { fn name(self) -> string { return "user" } }
fn erase(value: Named) -> any { return value }
fn restore(value: any) -> Named { return value as Named }
let value: Named = restore(erase(User {}))
println(value is Named)
println(value.name())
"#);
    assert_eq!(output, vec!["true", "user"]);
}

#[test]
fn class_override_string_array_and_primitive_witnesses() {
    let output = run(r#"
interface Speaking { fn speak() -> string }
class Parent { fn speak(self) -> string { return "parent" } }
class Child extends Parent { override fn speak(self) -> string { return "child" } }
fn speak(value: Speaking) -> string { return value.speak() }
println(speak(Child {}))

interface Sized { fn len() -> int }
fn size(value: Sized) -> int { return value.len() }
println(size("four"))
let values = [1]
let sized: Sized = values
values.push(2)
println(sized.len())

interface Valuable { fn value() -> int }
impl int { fn value(self) -> int { return self } }
fn read(value: Valuable) -> int { return value.value() }
println(read(9))

interface HasLen { fn len() -> int }
fn erase(value: any) -> any { return value }
println(erase("abc") is HasLen)
let erased_string: HasLen = erase("abcd") as HasLen
println(erased_string.len())
let erased_array: HasLen = erase([1, 2]) as HasLen
println(erased_array.len())
"#);
    assert_eq!(output, vec!["child", "4", "2", "9", "true", "4", "2"]);
}

#[test]
fn interface_width_defaults_alias_and_value_reference_boundaries() {
    let output = run(r#"
interface Narrow { fn first() -> int }
interface Wide { fn second() -> int fn first() -> int }
struct Both {
    fn first(self) -> int { return 1 }
    fn second(self) -> int { return 2 }
}
fn narrow(value: Narrow) -> int { return value.first() }
fn convert(value: Wide) -> Narrow { return value }
println(narrow(convert(Both {})))

interface Flexible { fn value(amount: int = 7) -> int }
struct FlexibleImpl {
    fn value(self, amount: int = 9) -> int { return amount }
}
fn read(value: Flexible) -> int { return value.value() }
fn read_named(value: Flexible) -> int { return value.value(amount: 3) }
println(read(FlexibleImpl {}))
println(read_named(FlexibleImpl {}))

type Alias = Narrow
fn alias(value: Alias) -> Alias { return value }
println(alias(Both {}).first())

interface Named { fn name() -> string }
struct User {
    label: string
    fn name(self) -> string { return self.label }
}
struct Holder { value: Named }
let user = User { label: "before" }
let holder = Holder { value: user }
user.label = "after"
println(holder.value.name())

class RefUser {
    label: string
    fn name(self) -> string { return self.label }
}
let ref_user = RefUser { label: "before" }
let ref_view: Named = ref_user
ref_user.label = "after"
println(ref_view.name())

interface Factory { fn make() -> Named }
struct FactoryImpl { fn make(self) -> User { return User { label: "factory" } } }
fn build(value: Factory) -> Named { return value.make() }
println(build(FactoryImpl {}).name())
"#);
    assert_eq!(
        output,
        vec!["1", "7", "3", "1", "before", "after", "factory"]
    );
}

#[test]
fn interface_witness_survives_spawn_boundary() {
    let output = run(r#"
interface Named { fn name() -> string }
struct User { fn name(self) -> string { return "spawned" } }
let done: Channel<int> = chan()
fn show_done(value: Named, done: Channel<int>) {
    println(value.name())
    send(done, 1)
}
spawn show_done(User {}, done)
recv(done)
"#);
    assert_eq!(output, vec!["spawned"]);
}

#[test]
fn invalid_interface_cast_is_rejected() {
    let source = r#"
interface Named { fn name() -> string }
let invalid: Named = 1 as Named
"#;
    assert!(lirac::compile(source).is_err());
}

#[test]
fn invalid_erased_interface_cast_fails_at_runtime() {
    let source = r#"
interface Named { fn name() -> string }
fn erase(value: any) -> any { return value }
let invalid: Named = erase(1) as Named
println(invalid.name())
"#;
    let bytecode = lirac::compile(source).expect("Any casts are checker-approved");
    assert!(liravm::run_with_capture(&bytecode).is_err());
}

#[test]
fn erased_interface_membership_is_structural_and_bounded() {
    let output = run(r#"
interface Named { fn name() -> string }
struct User { fn name(self) -> string { return "user" } }
fn erase(value: any) -> any { return value }
fn erase_named(value: Named) -> any { return value }
println(erase(User {}) is Named)
println(erase_named(User {}) is Named)
println(erase(1) is Named)
println(erase("text") is Named)
"#);
    assert_eq!(output, vec!["true", "true", "false", "false"]);
}

#[test]
fn interface_len_on_string_is_a_byte_count() {
    // A string dispatched through an interface `len` must return the byte count
    // (matching `len()`, the docs, and the native backend), not a UTF-8 scalar
    // count. "héllo" is 5 code points but 6 bytes.
    let output = run(r#"
interface Sized { fn len(self) -> int }
fn main() {
    let s: Sized = "héllo"
    println(s.len())
    println(len("héllo"))
}
"#);
    assert_eq!(output, vec!["6", "6"]);
}
