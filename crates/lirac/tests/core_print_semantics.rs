//! End-to-end core `print`/`println` semantics and diagnostics.

use lirac::analyze;

#[test]
fn print_is_newline_free_and_println_terminates_the_logical_line() {
    let bytecode = lirac::compile(
        r#"
print("a")
print("b")
println("c")
print("left\n")
println("right")
"#,
    )
    .expect("core output source compiles");
    let program = liravm::bytecode::load(&bytecode).expect("compiled bytecode loads");
    let mut vm = liravm::vm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);

    assert_eq!(vm.run().expect("compiled output source executes"), 0);
    assert_eq!(vm.get_output_string(), "abc\nleft\nright\n");
    assert_eq!(vm.get_output(), &["abc", "left", "right"]);
}

#[test]
fn print_and_println_require_exactly_one_argument() {
    let source = r#"print()
println(1, 2)
print("valid neighbor")
"#;
    let diagnostics = analyze(source).expect("source parses").diagnostics;

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 1
            && diagnostic.column == 1
            && diagnostic.message == "Expected at least 1 arguments, got 0"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 2
            && diagnostic.column == 1
            && diagnostic.message == "Expected at most 1 arguments, got 2"
    }));
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.line != 3),
        "valid neighboring print call was rejected: {diagnostics:?}"
    );
}

#[test]
fn print_and_println_validate_named_arguments() {
    let valid = lirac::compile(
        r#"
print(value: "named")
println(value: " output")
"#,
    )
    .expect("the specified core parameter name must be accepted");
    let (status, output) =
        liravm::run_with_capture(&valid).expect("named core output calls must execute");
    assert_eq!(status, 0);
    assert_eq!(output, ["named output"]);

    let diagnostics = analyze(
        r#"
print(wrong: "x")
println(value: "x", value: "y")
println(value: "valid neighbor")
"#,
    )
    .expect("source parses")
    .diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 2 && diagnostic.message == "Unknown named argument 'wrong'"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 3
            && diagnostic
                .message
                .contains("Duplicate value for parameter 'value'")
    }));
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.line != 4),
        "valid neighboring named call was rejected: {diagnostics:?}"
    );
}
