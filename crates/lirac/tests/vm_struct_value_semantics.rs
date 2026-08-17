//! Real-source regressions for VM struct value semantics.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode =
        lirac::compile_with_imports(name, source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let (code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|e| panic!("run {name}: {e}"));
    assert_eq!(code, 0, "{name} exited with status {code}");
    output
}

#[test]
fn assignment_and_nested_structs_are_independent() {
    assert_eq!(
        run_source(
            "struct_assignment",
            r#"
struct Inner { value: int }
struct Outer { inner: Inner }
let first = Outer { inner: Inner { value: 1 } }
let second = first
second.inner.value = 9
println(first.inner.value)
println(second.inner.value)
"#,
        ),
        vec!["1", "9"]
    );
}

#[test]
fn function_argument_and_return_copy_structs() {
    assert_eq!(
        run_source(
            "struct_function_boundary",
            r#"
struct Point { x: int }
fn change(point: Point) -> Point {
    point.x = 8
    return point
}

let original = Point { x: 2 }
let changed = change(original)
println(original.x)
println(changed.x)
"#,
        ),
        vec!["2", "8"]
    );
}

#[test]
fn implicit_struct_return_copies_locals_and_direct_tail_return_remains_safe() {
    let bytecode = lirac::compile(
        r#"
struct Point { x: int }
fn make() -> Point {
    Point { x: 3 }
}
fn forward() -> Point {
    return make()
}
"#,
    )
    .expect("compile direct struct tail return");
    let program = liravm::bytecode::load(&bytecode).expect("load direct struct tail return");
    assert!(
        program
            .code
            .contains(&(lira_core::opcode::Opcode::TailCall as u8)),
        "direct call return should retain TailCall after return-boundary lowering"
    );

    assert_eq!(
        run_source(
            "struct_return_boundaries",
            r#"
struct Point { x: int }
fn make() -> Point {
    let local = Point { x: 3 }
    local
}

fn forward() -> Point {
    return make()
}

let first = make()
let second = forward()
first.x = 8
second.x = 9
println(first.x)
println(second.x)
"#,
        ),
        vec!["8", "9"]
    );
}

#[test]
fn mutable_struct_receiver_updates_the_receiver_without_aliasing_assignment() {
    assert_eq!(
        run_source(
            "struct_mut_receiver",
            r#"
struct Counter { value: int fn bump(this mut) { this.value = this.value + 1 } }
let first = Counter { value: 1 }
let second = first
first.bump()
println(first.value)
println(second.value)
"#,
        ),
        vec!["2", "1"]
    );
}

#[test]
fn struct_collection_insertion_and_extraction_copy() {
    assert_eq!(
        run_source(
            "struct_collections",
            r#"
struct Point { x: int }
let point = Point { x: 3 }
let points = [point]
let extracted = points[0]
extracted.x = 7
println(point.x)
println(points[0].x)
println(extracted.x)
"#,
        ),
        vec!["3", "3", "7"]
    );
}

#[test]
fn channel_and_select_payloads_copy_structs() {
    assert_eq!(
        run_source(
            "struct_channel_select",
            r#"
struct Point { x: int }
let ch: Channel<Point> = chan(1)
let point = Point { x: 6 }
send(ch, point)
select {
    received = <-ch => {
        received.x = 10
        println(received.x)
    }
    _ => println("default")
}
println(point.x)
"#,
        ),
        vec!["10", "6"]
    );
}

#[test]
fn closure_capture_and_pattern_binding_copy_structs() {
    assert_eq!(
        run_source(
            "struct_capture_pattern",
            r#"
struct Point { x: int }
fn make() -> int {
    let point = Point { x: 4 }
    let get = || point.x
    point.x = 6
    return get()
}
let source = Point { x: 5 }
match source {
    Point { x } => println(x)
}
println(make())
"#,
        ),
        vec!["5", "4"]
    );
}

#[test]
fn classes_remain_reference_like_and_dispatch_virtual() {
    assert_eq!(
        run_source(
            "class_reference_boundary",
            r#"
class Base { value: int fn speak(self) -> int { return 1 } }
class Child extends Base { override fn speak(self) -> int { return 2 } }
let first = Child { value: 0 }
let second = first
second.value = 9
println(first.value)
println(first.speak())
"#,
        ),
        vec!["9", "2"]
    );
}

#[test]
fn struct_cycles_render_bounded_and_do_not_break_collection() {
    assert_eq!(
        run_source(
            "struct_cycle_boundary",
            r#"
struct Node { next: Node? }
var node = Node { next: null }
node.next = node
println(node)
collect()
println(node)
"#,
        ),
        // Struct assignment recursively copies the value, so assigning a
        // struct to its own field terminates as a finite copied snapshot.
        vec!["{next: {next: null}}", "{next: {next: null}}"]
    );
}

#[test]
fn repeated_nested_struct_copies_remain_collectable() {
    assert_eq!(
        run_source(
            "nested_struct_copy_stress",
            r#"
struct Leaf { value: int }
struct Middle { leaf: Leaf }
struct Root { middle: Middle }
var root = Root { middle: Middle { leaf: Leaf { value: 11 } } }
var i = 0
while i < 100 {
    let copy = root
    i = i + 1
}
collect()
println(root.middle.leaf.value)
"#,
        ),
        vec!["11"]
    );
}
