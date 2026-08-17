//! Regression coverage for generic methods declared directly in a struct body.

use lirac::{analyze, check, compile};

const INLINE_BOX: &str = r#"
struct Box<T> {
    value: T

    fn map<U>(self, callback: fn(T) -> U) -> Box<U> {
        return Box { value: callback(self.value) }
    }
}
"#;

#[test]
fn inline_generic_method_is_checked_and_compiles() {
    let source = format!(
        r#"
{INLINE_BOX}
let number = Box {{ value: 21 }}
let doubled: Box<int> = number.map(|value: int| value * 2)
println(doubled.value)
"#
    );

    check(&source).expect("inline generic method should type-check");
    let bytecode = compile(&source).expect("inline generic method should compile");
    let (status, output) = liravm::run_with_capture(&bytecode).expect("VM should execute");
    assert_eq!(status, 0);
    assert_eq!(output, ["42"]);
}

#[test]
fn inline_generic_method_rejects_wrong_callback_and_type_argument_arity() {
    let wrong_callback = format!(
        r#"
{INLINE_BOX}
let number = Box {{ value: 21 }}
let invalid = number.map(|value: string| value)
"#
    );
    let callback_diagnostics = analyze(&wrong_callback)
        .expect("wrong callback source should parse")
        .diagnostics;
    assert!(
        callback_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Argument type mismatch")),
        "expected wrong callback diagnostic, got {callback_diagnostics:?}"
    );

    let wrong_arity = format!(
        r#"
{INLINE_BOX}
let number = Box {{ value: 21 }}
let invalid = number.map::<int, string>(|value: int| value)
"#
    );
    let arity_diagnostics = analyze(&wrong_arity)
        .expect("wrong type-arity source should parse")
        .diagnostics;
    assert!(
        arity_diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("Expected 1 type argument, got 2")),
        "expected generic arity diagnostic, got {arity_diagnostics:?}"
    );
}

#[test]
fn inline_method_body_is_checked_in_its_owner_and_generic_scope() {
    let source = r#"
struct Box<T> {
    value: T

    fn broken<U>(self, callback: fn(T) -> U) -> U {
        return callback(self.missing)
    }
}
"#;

    let diagnostics = analyze(source)
        .expect("invalid inline method source should parse")
        .diagnostics;
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("Unknown field or method: missing")),
        "expected inline method-body field diagnostic, got {diagnostics:?}"
    );
}
