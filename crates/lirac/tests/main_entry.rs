//! Entry-point semantics: `main()` must run exactly once.
//!
//! Lira supports two idioms:
//!   1. Script style — top-level statements execute in order; there is no `main`.
//!   2. Program style — a `fn main()` is defined and auto-invoked as the entry point.
//!
//! The bug these tests pin down: codegen emitted every top-level statement
//! (including an explicit `main()` call, which many examples write out of
//! Rust/C habit) AND then unconditionally auto-invoked `main()` — so a file
//! doing both ran `main` twice, silently doubling all output. The auto-invoke
//! must be suppressed when the top level already calls `main`.

fn run(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile_with_imports("test.li", source)?;
    let (_exit_code, output) = liravm::run_with_capture(&bytecode)?;
    Ok(output)
}

#[test]
fn main_defined_and_explicitly_called_runs_once() {
    // The Rust/C idiom: define main, then call it. Must NOT double.
    let source = r#"
fn main() {
    println("hello")
}
main()
"#;
    let output = run(source).expect("program should compile and run");
    assert_eq!(output, vec!["hello".to_string()]);
}

#[test]
fn main_defined_not_called_is_auto_invoked() {
    // Program style: main is the entry point even without an explicit call.
    let source = r#"
fn main() {
    println("hello")
}
"#;
    let output = run(source).expect("program should compile and run");
    assert_eq!(output, vec!["hello".to_string()]);
}

#[test]
fn no_main_runs_top_level_once() {
    // Script style: no main at all.
    let source = r#"println("hello")"#;
    let output = run(source).expect("program should compile and run");
    assert_eq!(output, vec!["hello".to_string()]);
}

#[test]
fn top_level_preamble_then_explicit_main_runs_each_once() {
    // A top-level statement before an explicit main() call: the preamble runs,
    // then main runs once (not twice).
    let source = r#"
fn main() {
    println("in main")
}
println("preamble")
main()
"#;
    let output = run(source).expect("program should compile and run");
    assert_eq!(output, vec!["preamble".to_string(), "in main".to_string()]);
}
