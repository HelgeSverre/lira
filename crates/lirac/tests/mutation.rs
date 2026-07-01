//! In-place mutation of compound lvalues: struct fields and array elements.
//!
//! Bug these pin down: codegen's `Assign`/`CompoundAssign` only stored to
//! `Identifier` targets. `obj.field = v` and `arr[i] = v` compiled the value
//! and silently dropped the store — no `SetField`/`ArraySet` was emitted — so
//! every field/element mutation was a no-op. The VM has always supported it
//! (objects and arrays are shared `Gc<GcCell>` refs); only codegen was missing.

fn run(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile_with_imports("test.li", source)?;
    let (_exit_code, output) = liravm::run_with_capture(&bytecode)?;
    Ok(output)
}

#[test]
fn direct_struct_field_assignment() {
    let src = r#"
struct C { n: int }
var c = C { n: 0 }
c.n = 5
println(c.n)
"#;
    assert_eq!(run(src).unwrap(), vec!["5".to_string()]);
}

#[test]
fn method_mutates_self_field() {
    let src = r#"
struct C { n: int }
impl C {
    fn bump(self) {
        self.n = self.n + 1
    }
}
var c = C { n: 0 }
c.bump()
c.bump()
println(c.n)
"#;
    assert_eq!(run(src).unwrap(), vec!["2".to_string()]);
}

#[test]
fn array_element_assignment_primitive() {
    let src = r#"
var a = [1, 2, 3]
a[0] = 99
a[2] = 7
println(a[0])
println(a[1])
println(a[2])
"#;
    assert_eq!(run(src).unwrap(), vec!["99".to_string(), "2".to_string(), "7".to_string()]);
}

#[test]
fn array_of_struct_field_assignment() {
    let src = r#"
struct P { x: int }
var ps = [P { x: 1 }, P { x: 2 }]
ps[0].x = 99
println(ps[0].x)
println(ps[1].x)
"#;
    assert_eq!(run(src).unwrap(), vec!["99".to_string(), "2".to_string()]);
}

#[test]
fn nested_struct_field_assignment() {
    let src = r#"
struct Inner { v: int }
struct Outer { inner: Inner }
var o = Outer { inner: Inner { v: 1 } }
o.inner.v = 7
println(o.inner.v)
"#;
    assert_eq!(run(src).unwrap(), vec!["7".to_string()]);
}

#[test]
fn compound_assign_struct_field() {
    let src = r#"
struct C { n: int }
var c = C { n: 10 }
c.n += 5
c.n *= 2
println(c.n)
"#;
    assert_eq!(run(src).unwrap(), vec!["30".to_string()]);
}

#[test]
fn compound_assign_array_element() {
    let src = r#"
var a = [10, 20, 30]
a[1] += 5
a[2] -= 10
println(a[0])
println(a[1])
println(a[2])
"#;
    assert_eq!(run(src).unwrap(), vec!["10".to_string(), "25".to_string(), "20".to_string()]);
}
