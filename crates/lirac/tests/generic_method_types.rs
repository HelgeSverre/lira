//! Public-API coverage for generic methods on generic user-defined types.
//!
//! These tests deliberately start with Lira source and exercise checking,
//! compilation, and VM execution.  In particular, `map<U>` must keep the
//! receiver's `T` distinct from its result type `U`.

use lirac::{analyze, check, compile_with_imports};

const BOX_METHODS: &str = r#"
struct Box<T> {
    value: T
}

impl<T> Box<T> {
    fn get(self) -> T {
        return self.value
    }

    fn map<U>(self, f: fn(T) -> U) -> Box<U> {
        return Box { value: f(self.value) }
    }
}
"#;

fn diagnostics(source: &str) -> Vec<lirac::Diagnostic> {
    analyze(source)
        .expect("source should lex and parse")
        .diagnostics
}

fn assert_diagnostic_contains(source: &str, text: &str) {
    let diagnostics = diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(text)),
        "expected a diagnostic containing {text:?}, got {diagnostics:?}"
    );
}

#[test]
fn generic_method_types_check_in_nested_and_optional_annotations() {
    let source = format!(
        r#"
{BOX_METHODS}
let boxed: Box<int> = Box {{ value: 42 }}
let nested: [Box<int>] = [Box {{ value: 7 }}]
let optional: Box<int>? = Box {{ value: 9 }}
let text: Box<string> = Box {{ value: "lira" }}
let mapped: Box<int> = text.map(|value: string| 1)
"#
    );

    let analysis = analyze(&source).expect("generic method source should parse");
    assert!(
        analysis.diagnostics.is_empty(),
        "generic method annotations should check: {:?}",
        analysis.diagnostics
    );
    assert!(analysis
        .sema
        .generic_type_instances
        .contains_key("Box<int>"));
    assert!(analysis
        .sema
        .generic_type_instances
        .contains_key("Box<string>"));
}

#[test]
fn generic_method_map_can_change_t_to_a_different_u() {
    let source = format!(
        r#"
{BOX_METHODS}
let boxed: Box<int> = Box {{ value: 41 }}
let mapped: Box<string> = boxed.map(|value: int| "mapped")
"#
    );

    check(&source).expect("map<U> should permit U to differ from receiver T");
}

#[test]
fn generic_method_type_mismatches_are_rejected() {
    let wrong_assignment = format!(
        r#"
{BOX_METHODS}
let boxed: Box<int> = Box {{ value: 42 }}
let wrong: Box<string> = boxed
"#
    );
    assert_diagnostic_contains(
        &wrong_assignment,
        "Type mismatch: expected 'Box<string>', got 'Box<int>'",
    );

    let wrong_callback = format!(
        r#"
{BOX_METHODS}
let boxed: Box<int> = Box {{ value: 42 }}
let wrong = boxed.map(|value: string| value)
"#
    );
    assert_diagnostic_contains(&wrong_callback, "Argument type mismatch");

    let wrong_result = format!(
        r#"
{BOX_METHODS}
let boxed: Box<int> = Box {{ value: 42 }}
let wrong: Box<int> = boxed.map(|value: int| "text")
"#
    );
    assert_diagnostic_contains(
        &wrong_result,
        "Type mismatch: expected 'Box<int>', got 'Box<string>'",
    );
}

#[test]
fn wrong_user_generic_arity_is_diagnosed() {
    let source = format!(
        r#"
{BOX_METHODS}
let wrong: Box<int, string> = Box {{ value: 42 }}
"#
    );
    assert_diagnostic_contains(&source, "Type 'Box' expects 1 type argument");
}

#[test]
fn generic_method_source_compiles_and_executes() {
    let source = format!(
        r#"
{BOX_METHODS}
fn main() {{
    let boxed: Box<int> = Box {{ value: 21 }}
    let doubled: Box<int> = boxed.map(|value: int| value * 2)
    println(doubled.get())

    let text: Box<string> = Box {{ value: "source" }}
    let length: Box<int> = text.map(|value: string| len(value))
    println(length.get())

    let nested: [Box<int>] = [Box {{ value: 7 }}]
    println(nested[0].get())
}}
main()
"#
    );
    let bytecode = compile_with_imports("generic_method_types.li", &source)
        .expect("generic method source should compile");
    let (status, output) = liravm::run_with_capture(&bytecode).expect("VM should execute");
    assert_eq!(status, 0);
    assert_eq!(output, ["42", "6", "7"]);
}
