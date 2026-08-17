//! End-to-end core `assert(bool)` behavior and misuse diagnostics.

use lirac::analyze;

#[test]
fn true_assertion_continues_and_false_assertion_stops_at_its_source_span() {
    let passing =
        lirac::compile("assert(1 < 2)\nprintln(\"ok\")\n").expect("valid core assertion compiles");
    let (status, output) =
        liravm::run_with_capture(&passing).expect("true assertion executes normally");
    assert_eq!(status, 0);
    assert_eq!(output, ["ok"]);

    let failing = lirac::compile("println(\"before\")\nassert(false)\nprintln(\"after\")\n")
        .expect("false core assertion compiles");
    let (output, error) = liravm::run_with_capture_structured(&failing)
        .expect_err("false assertion must stop execution");
    assert_eq!(output, ["before"]);
    assert_eq!(error.message, "assertion failed");
    assert_eq!(error.line, Some(2));
    assert_eq!(error.column, Some(1));
}

#[test]
fn core_assert_reports_arity_and_argument_type_misuse() {
    let source = "assert()\nassert(1)\nassert(false, true)\nassert(true)\n";
    let diagnostics = analyze(source).expect("source parses").diagnostics;

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 1
            && diagnostic.column == 1
            && diagnostic.message == "Expected at least 1 arguments, got 0"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 2
            && diagnostic.column == 8
            && diagnostic.message.contains("expected 'bool', got 'int'")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 3
            && diagnostic.column == 1
            && diagnostic.message == "Expected at most 1 arguments, got 2"
    }));
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.line != 4),
        "valid neighboring assertion was rejected: {diagnostics:?}"
    );
}

#[test]
fn core_assert_validates_its_named_condition() {
    lirac::check("assert(condition: true)\n")
        .expect("the specified core assert parameter name must be accepted");

    let diagnostics = analyze(
        "assert(value: true)\nassert(condition: true, condition: false)\nassert(condition: true)\n",
    )
    .expect("source parses")
    .diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 1 && diagnostic.message == "Unknown named argument 'value'"
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.line == 2
            && diagnostic
                .message
                .contains("Duplicate value for parameter 'condition'")
    }));
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.line != 3),
        "valid neighboring named assertion was rejected: {diagnostics:?}"
    );
}
