//! Type-checking coverage for named struct and class literals.
//!
//! These tests intentionally start from source text and run the normal lexer,
//! parser, and checker pipeline.  Runtime/codegen behavior is covered by the
//! existing VM integration tests; this file focuses on aggregate validation and
//! its structured diagnostics.

use lirac::{analyze, check};

fn diagnostic_messages(source: &str) -> Vec<lirac::Diagnostic> {
    analyze(source)
        .expect("source should lex and parse")
        .diagnostics
}

#[test]
fn valid_literals_accept_reordered_nested_class_and_generic_fields() {
    let source = r#"
struct Inner {
    value: int
}

struct Outer {
    name: string
    inner: Inner
}

class Base {
    id: int
}

class Child extends Base {
    label: string
}

struct Box<T> {
    value: T
}

let outer = Outer { inner: Inner { value: 7 }, name: "nested" }
let child = Child { label: "child", id: 9 }
let boxed = Box { value: 42 }
"#;

    assert!(
        check(source).is_ok(),
        "valid aggregate literals should check"
    );
}

#[test]
fn unknown_field_has_structured_diagnostic_and_value_span() {
    let source = "struct Point { x: int }\nlet p = Point { y: 1 }\n";
    let diagnostics = diagnostic_messages(source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic
                .message
                .contains("Unknown field: y on type Point")
        })
        .expect("unknown field diagnostic");

    assert_eq!((diagnostic.line, diagnostic.column), (2, 20));
}

#[test]
fn duplicate_field_is_rejected() {
    let diagnostics =
        diagnostic_messages("struct Point { x: int }\nlet p = Point { x: 1, x: 2 }\n");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message == "Duplicate field 'x' in Point literal"));
}

#[test]
fn missing_field_is_rejected() {
    let diagnostics =
        diagnostic_messages("struct Point { x: int, y: int }\nlet p = Point { x: 1 }\n");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message == "Missing field 'y' in Point literal"));
}

#[test]
fn field_value_type_mismatch_points_at_the_value() {
    let source = "struct Point { x: int }\nlet p = Point { x: \"wrong\" }\n";
    let diagnostics = diagnostic_messages(source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic
                .message
                .contains("Type mismatch: expected 'int', got 'string'")
        })
        .expect("field value type mismatch diagnostic");

    assert_eq!((diagnostic.line, diagnostic.column), (2, 20));
}

#[test]
fn unknown_or_non_aggregate_names_are_rejected() {
    let unknown = diagnostic_messages("let p = Missing { x: 1 }\n");
    assert!(unknown
        .iter()
        .any(|diagnostic| diagnostic.message == "Unknown type: Missing"));

    let non_aggregate = diagnostic_messages("type Alias = int\nlet value = Alias { x: 1 }\n");
    assert!(non_aggregate
        .iter()
        .any(|diagnostic| diagnostic.message == "Type 'Alias' is not a struct or class"));
}
