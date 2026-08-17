//! Equality compatibility regressions from real Lira source.

use lirac::{analyze, check, compile};

#[test]
fn compatible_equality_operands_check_and_execute() {
    let source = r#"
let maybe: int? = null
println(true == false)
println(1 == 1.0)
println("left" != "right")
println(maybe == null)
println(null == maybe)
"#;
    check(source).expect("compatible equality source checks");
    let bytecode = compile(source).expect("compatible equality source compiles");
    let (status, output) = liravm::run_with_capture(&bytecode).expect("equality source executes");
    assert_eq!(status, 0);
    assert_eq!(output, ["false", "true", "true", "true", "true"]);
}

#[test]
fn unrelated_equality_operands_are_rejected_at_the_operator_expression() {
    let source = r#"
let first = true == 1
let second = false != 0.0
let third = "1" == 1
let valid = true == false
"#;
    let diagnostics = analyze(source).expect("source parses").diagnostics;
    let expected = [
        "Cannot compare values of type 'bool' and 'int' for equality",
        "Cannot compare values of type 'bool' and 'float' for equality",
        "Cannot compare values of type 'string' and 'int' for equality",
    ];
    for (index, message) in expected.iter().enumerate() {
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.message == *message && diagnostic.line == index + 2
            }),
            "missing {message:?}: {diagnostics:?}"
        );
    }
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.line != 5),
        "neighboring valid equality must not be rejected: {diagnostics:?}"
    );
}
