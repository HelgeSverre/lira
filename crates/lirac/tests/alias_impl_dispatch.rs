//! Alias-owner dispatch must use the underlying runtime type for the VM.

fn run(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile_with_imports("alias_impl.li", source)?;
    let (status, output) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM exited with status {status}"));
    }
    Ok(output)
}

#[test]
fn primitive_alias_instance_impl_dispatches() {
    let source = r#"
type Integer = int
impl Integer {
    fn bump(self) -> int { return self + 1 }
}
let x: Integer = 41
println(x.bump())
"#;
    assert_eq!(run(source).expect("alias impl should execute"), ["42"]);
}

#[test]
fn primitive_alias_static_impl_dispatches() {
    let source = r#"
type Integer = int
impl Integer {
    fn answer() -> int { return 42 }
}
println(Integer.answer())
"#;
    assert_eq!(
        run(source).expect("alias static impl should execute"),
        ["42"]
    );
}

#[test]
fn alias_chain_instance_impl_dispatches() {
    let source = r#"
type BaseInteger = int
type Integer = BaseInteger
impl Integer {
    fn bump(self) -> int { return self + 1 }
}
let x: Integer = 41
println(x.bump())
"#;
    assert_eq!(
        run(source).expect("alias chain impl should execute"),
        ["42"]
    );
}

#[test]
fn aggregate_alias_impl_dispatches() {
    let source = r#"
struct Point { x: int }
type Position = Point
impl Position {
    fn value(self) -> int { return self.x }
}
let p: Position = Point { x: 42 }
println(p.value())
"#;
    assert_eq!(
        run(source).expect("aggregate alias impl should execute"),
        ["42"]
    );
}

#[test]
fn array_alias_impl_dispatches() {
    let source = r#"
type Integer = int
type Ints = [Integer]
impl Ints {
    fn first_plus(self) -> int { return self[0] + 1 }
}
let xs: Ints = [41]
println(xs.first_plus())
"#;
    assert_eq!(
        run(source).expect("array alias impl should execute"),
        ["42"]
    );
}

#[test]
fn alias_to_class_impl_is_rejected() {
    let source = r#"
class Counter { value: int }
type Alias = Counter
impl Alias { fn invalid(self) -> int { return self.value } }
"#;
    let error = lirac::compile_with_imports("alias_impl.li", source)
        .expect_err("alias-to-class impl must be rejected");
    assert!(error.contains("impl"));
    assert!(error.contains("classes"));
}

#[test]
fn duplicate_alias_impl_is_rejected() {
    let source = r#"
type Integer = int
impl int { fn bump(self) -> int { return self + 1 } }
impl Integer { fn bump(self) -> int { return self + 2 } }
"#;
    let error = lirac::compile_with_imports("alias_impl.li", source)
        .expect_err("canonical owner collision must be rejected");
    assert!(error.contains("duplicate impl method"));
}

#[test]
fn missing_alias_method_is_rejected_specifically() {
    let source = r#"
type Integer = int
let x: Integer = 41
println(x.missing())
"#;
    let error = lirac::compile_with_imports("alias_impl.li", source)
        .expect_err("missing method must remain a checker error");
    assert!(error.contains("Unknown method"));
    assert!(error.contains("missing"));
}
