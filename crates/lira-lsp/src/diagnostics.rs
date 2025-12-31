//! Diagnostics generation for Lira
//!
//! Runs the Lira compiler pipeline and extracts errors for LSP diagnostics.

use regex::Regex;
use tower_lsp::lsp_types::*;

/// Check a document and return diagnostics
pub fn check_document(uri: &Url, content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Get the file path for module resolution
    let file_path = uri.path();

    // Run the type checker
    let result = lirac::check_with_imports(file_path, content);

    if let Err(error) = result {
        // Parse error message to extract diagnostics
        diagnostics.extend(parse_error_message(&error, content));
    }

    diagnostics
}

/// Parse an error message from lirac into LSP diagnostics
fn parse_error_message(error: &str, content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Try to extract line/column from error messages
    // Common formats:
    // - "Error at line X, column Y: message"
    // - "Line X: message"
    // - "[X:Y] message"

    let line_col_regex =
        Regex::new(r"(?:Error at |)line\s*(\d+)(?:,?\s*column\s*(\d+))?:?\s*(.*)").unwrap();
    let bracket_regex = Regex::new(r"\[(\d+):(\d+)\]\s*(.*)").unwrap();

    // Count total lines for range validation
    let line_count = content.lines().count();

    for line in error.lines() {
        let (line_num, col_num, message) = if let Some(caps) = line_col_regex.captures(line) {
            let line_num = caps
                .get(1)
                .map(|m| m.as_str().parse::<u32>().unwrap_or(1))
                .unwrap_or(1);
            let col_num = caps
                .get(2)
                .map(|m| m.as_str().parse::<u32>().unwrap_or(0))
                .unwrap_or(0);
            let message = caps.get(3).map(|m| m.as_str()).unwrap_or(line);
            (line_num, col_num, message.to_string())
        } else if let Some(caps) = bracket_regex.captures(line) {
            let line_num = caps
                .get(1)
                .map(|m| m.as_str().parse::<u32>().unwrap_or(1))
                .unwrap_or(1);
            let col_num = caps
                .get(2)
                .map(|m| m.as_str().parse::<u32>().unwrap_or(0))
                .unwrap_or(0);
            let message = caps.get(3).map(|m| m.as_str()).unwrap_or(line);
            (line_num, col_num, message.to_string())
        } else if !line.trim().is_empty() {
            // No position info - put at start of file
            (1, 0, line.to_string())
        } else {
            continue;
        };

        // Convert to 0-indexed and validate
        let line_idx = line_num.saturating_sub(1);
        let safe_line = if (line_idx as usize) < line_count {
            line_idx
        } else {
            (line_count.saturating_sub(1)) as u32
        };

        // Get the line length for end position
        let line_text = content.lines().nth(safe_line as usize).unwrap_or("");
        let end_col = line_text.len() as u32;

        diagnostics.push(Diagnostic {
            range: Range {
                start: Position {
                    line: safe_line,
                    character: col_num.saturating_sub(1),
                },
                end: Position {
                    line: safe_line,
                    character: end_col,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("lira".to_string()),
            message,
            ..Default::default()
        });
    }

    // If we couldn't parse any diagnostics, create one at the start
    if diagnostics.is_empty() && !error.trim().is_empty() {
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("lira".to_string()),
            message: error.to_string(),
            ..Default::default()
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_line_error() {
        let error = "Error at line 5, column 10: undefined variable 'foo'";
        let content = "line1\nline2\nline3\nline4\nlet x = foo\nline6";
        let diagnostics = parse_error_message(error, content);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start.line, 4); // 0-indexed
        assert_eq!(diagnostics[0].range.start.character, 9); // 0-indexed
    }

    #[test]
    fn test_parse_bracket_error() {
        let error = "[3:1] unexpected token";
        let content = "line1\nline2\nlet x =";
        let diagnostics = parse_error_message(error, content);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start.line, 2);
    }
}
