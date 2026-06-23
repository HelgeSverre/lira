//! Diagnostics generation for Lira
//!
//! Runs the Lira compiler pipeline and maps its structured diagnostics onto
//! LSP diagnostics. The compiler reports each error with a precise span, so we
//! no longer reconstruct positions by regex-parsing a flattened error string.

use tower_lsp::lsp_types::*;

/// Check a document and return diagnostics
pub fn check_document(uri: &Url, content: &str) -> Vec<Diagnostic> {
    let file_path = uri.path();
    lirac::check_with_imports_diagnostics(file_path, content)
        .iter()
        .map(|diag| to_lsp_diagnostic(diag, content))
        .collect()
}

/// Convert a compiler [`lirac::Diagnostic`] (1-indexed, position-only) into an
/// LSP [`Diagnostic`]. The error range runs from the reported column to the end
/// of that line; positions of 0 fall back to the start of the file.
fn to_lsp_diagnostic(diag: &lirac::Diagnostic, content: &str) -> Diagnostic {
    let line_count = content.lines().count();

    // Compiler positions are 1-indexed; LSP positions are 0-indexed.
    let line_idx = diag.line.saturating_sub(1);
    let safe_line = if line_idx < line_count {
        line_idx
    } else {
        line_count.saturating_sub(1)
    } as u32;

    let start_char = diag.column.saturating_sub(1) as u32;
    let line_text = content.lines().nth(safe_line as usize).unwrap_or("");
    let end_char = line_text.len() as u32;

    Diagnostic {
        range: Range {
            start: Position {
                line: safe_line,
                character: start_char,
            },
            end: Position {
                line: safe_line,
                character: end_char.max(start_char),
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("lira".to_string()),
        message: diag.message.clone(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(content: &str) -> Vec<Diagnostic> {
        let uri = Url::parse("file:///test.li").unwrap();
        check_document(&uri, content)
    }

    #[test]
    fn undefined_variable_is_reported_at_its_span() {
        // `x` is at line 1, column 9 (1-indexed) of "let y = x".
        let diagnostics = check("let y = x");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start.line, 0);
        assert_eq!(diagnostics[0].range.start.character, 8);
        assert_eq!(diagnostics[0].message, "Undefined variable: x");
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn valid_program_has_no_diagnostics() {
        assert!(check("let y = 1\nprintln(y)").is_empty());
    }

    #[test]
    fn error_on_later_line_maps_to_correct_line() {
        let diagnostics = check("let a = 1\nlet b = missing");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start.line, 1);
        assert_eq!(diagnostics[0].message, "Undefined variable: missing");
    }
}
