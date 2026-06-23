//! Tests for the structured diagnostics API.
//!
//! `check_with_imports_diagnostics` surfaces the checker's structured errors
//! with precise spans, so consumers (the LSP) no longer have to re-parse a
//! flattened "line:col: message" string to recover positions.

#[test]
fn undefined_variable_diagnostic_carries_span_and_clean_message() {
    // `x` is at column 9 of line 1: "let y = x"
    let diags = lirac::check_with_imports_diagnostics("test.li", "let y = x");

    assert_eq!(diags.len(), 1, "expected one diagnostic, got: {:?}", diags);
    assert_eq!(diags[0].line, 1);
    assert_eq!(diags[0].column, 9);
    assert_eq!(diags[0].message, "Undefined variable: x");
}

#[test]
fn valid_program_produces_no_diagnostics() {
    let diags = lirac::check_with_imports_diagnostics("test.li", "let y = 1\nprintln(y)");
    assert!(
        diags.is_empty(),
        "expected no diagnostics, got: {:?}",
        diags
    );
}
