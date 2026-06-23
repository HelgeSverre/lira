//! Tests that codegen consumes the checker's SemanticTables for method
//! dispatch, rather than re-inferring receiver types with syntactic heuristics.
//!
//! Methods on primitives (`impl int`, `impl [int]`, ...) are compiled to mangled
//! free functions and must be dispatched statically — there is no object field
//! to fall back on, so a method call whose receiver type cannot be recovered
//! fails at runtime with "Cannot get field from Int(..)".
//!
//! The legacy heuristic could only recover a receiver's type from a few syntactic
//! shapes (literals, struct literals, `Type.method()` calls, a `typename_*`
//! naming convention). A value returned from a plain function defeats all of them.
//! With SemanticTables wired in, codegen looks the receiver's type up by NodeId
//! and dispatches correctly.

fn run(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile_with_imports("test.li", source)?;
    let (_exit_code, output) = liravm::run_with_capture(&bytecode)?;
    Ok(output)
}

#[test]
fn primitive_method_on_function_result_dispatches() {
    // `x` is bound to the result of `produce()`. The legacy heuristic cannot
    // infer its type (the function name is not a `int_*` constructor), so the
    // `x.double()` call would fall through to a dynamic field load and fail.
    let source = r#"
impl int {
    fn double(self) -> int {
        return self * 2
    }
}

fn produce() -> int {
    return 21
}

let x = produce()
println(x.double())
"#;

    let output = run(source).expect("program should compile and run");
    assert_eq!(output, vec!["42".to_string()]);
}
