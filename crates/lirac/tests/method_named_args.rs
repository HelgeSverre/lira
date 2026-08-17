//! Method call named/default argument binding through the normal compiler and
//! VM pipeline. The bytecode VM must reorder named arguments by declaration
//! name and fill defaulted gaps, matching the native backend. See
//! `examples/method_named_args.li` for the parity (VM/AOT/JIT) coverage.

fn run(source: &str) -> Vec<String> {
    let bytecode = lirac::compile(source).expect("method source should compile");
    let (status, output) =
        liravm::run_with_capture(&bytecode).expect("method bytecode should execute");
    assert_eq!(status, 0);
    output
}

#[test]
fn static_method_named_arguments_reorder_by_name() {
    let output = run(r#"
struct Pair {
    a: int
    b: int
}
impl Pair {
    fn make(a: int, b: int) -> string { return "a=" + a + " b=" + b }
}
fn main() {
    println(Pair.make(b: 1, a: 2))
}
"#);
    assert_eq!(output, vec!["a=2 b=1"]);
}

#[test]
fn static_method_default_arguments_are_filled() {
    let output = run(r#"
struct Box { v: int }
impl Box {
    fn wrap(value: int, label: string = "box") -> string { return label + ":" + value }
}
fn main() {
    println(Box.wrap(7))
}
"#);
    assert_eq!(output, vec!["box:7"]);
}

#[test]
fn instance_method_named_arguments_drop_self_and_reorder() {
    let output = run(r#"
struct Point {
    x: int
    y: int
}
impl Point {
    fn shift(self, dx: int, dy: int = 1, scale: int = 1) -> string {
        return "(" + (dx * scale) + "," + (self.y + dy) + ")"
    }
}
fn main() {
    let p = Point { x: 1, y: 2 }
    println(p.shift(scale: 3, dx: 4))
    println(p.shift(dx: 2, dy: 9))
    println(p.shift(dx: 5))
}
"#);
    assert_eq!(output, vec!["(12,3)", "(2,11)", "(5,3)"]);
}

#[test]
fn class_instance_method_named_and_default_arguments() {
    let output = run(r#"
class Base {
    fn compute(self, a: int, b: int = 10) -> string { return "b" + a + ":" + b }
}
class Child extends Base {
    override fn compute(self, a: int, b: int = 20) -> string { return "c" + a + ":" + b }
}
fn main() {
    let c = Child {}
    println(c.compute(b: 2, a: 1))
    println(c.compute(a: 9))
}
"#);
    assert_eq!(output, vec!["c1:2", "c9:20"]);
}

#[test]
fn expression_receiver_method_named_arguments_reorder() {
    let output = run(r#"
struct S { v: int }
impl S { fn get(self, x: int, y: int) -> string { return "x=" + x + " y=" + y } }
fn make_s() -> S { return S { v: 5 } }
fn main() {
    println(make_s().get(y: 1, x: 2))
}
"#);
    assert_eq!(output, vec!["x=2 y=1"]);
}

#[test]
fn super_method_named_and_default_arguments() {
    let output = run(r#"
class Base {
    fn shift(self, amount: int = 1) -> string { return "base:" + amount }
}
class Child extends Base {
    fn go(self) -> string { return super.shift() }
    fn go2(self) -> string { return super.shift(amount: 9) }
}
fn main() {
    let c = Child {}
    println(c.go())
    println(c.go2())
}
"#);
    assert_eq!(output, vec!["base:1", "base:9"]);
}
