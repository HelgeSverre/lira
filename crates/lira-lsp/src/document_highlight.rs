//! Document highlight support for Lira
//!
//! Highlights all occurrences of the symbol under cursor.

use crate::utils::{self, get_regex};
use tower_lsp::lsp_types::*;

/// Get document highlights for the symbol at position
pub fn get_document_highlights(content: &str, position: Position) -> Vec<DocumentHighlight> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return vec![];
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at cursor
    let word_info = match utils::get_word_at_position(line, col) {
        Some(w) => w,
        None => return vec![],
    };

    // Find all occurrences
    find_all_highlights(content, &word_info.text)
}

fn find_all_highlights(content: &str, symbol: &str) -> Vec<DocumentHighlight> {
    let mut highlights = Vec::new();

    // Pattern to match the symbol as a whole word
    let pattern = format!(r"\b{}\b", regex::escape(symbol));
    let re = match get_regex(&pattern) {
        Some(r) => r,
        None => return highlights,
    };

    // Patterns for definitions (to mark as Write kind)
    let def_patterns = [
        format!(r"fn\s+{}\s*[<(]", regex::escape(symbol)),
        format!(r"struct\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"class\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"enum\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"trait\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"type\s+{}\s*[<=]", regex::escape(symbol)),
        format!(r"(?:let|var|const)\s+{}\s*[=:]", regex::escape(symbol)),
    ];

    let def_regexes: Vec<_> = def_patterns.iter().filter_map(|p| get_regex(p)).collect();

    for (line_idx, line) in content.lines().enumerate() {
        // Check if this line is a definition
        let is_definition = def_regexes.iter().any(|r| r.is_match(line));

        // Find all matches in the line
        for m in re.find_iter(line) {
            // Skip if in string or comment
            if utils::is_in_string_or_comment(line, m.start()) {
                continue;
            }

            // Convert byte positions to character positions
            let char_start = utils::byte_offset_to_char_col(line, m.start());
            let char_end = utils::byte_offset_to_char_col(line, m.end());

            // Check if this specific occurrence is an assignment target
            // Look at what comes after the symbol (skip whitespace)
            let after_match = &line[m.end()..];
            let is_assignment_target = after_match.trim_start().starts_with('=')
                && !after_match.trim_start().starts_with("==");

            // Determine highlight kind
            let kind = if is_definition || is_assignment_target {
                Some(DocumentHighlightKind::WRITE)
            } else {
                Some(DocumentHighlightKind::READ)
            };

            highlights.push(DocumentHighlight {
                range: Range {
                    start: Position {
                        line: line_idx as u32,
                        character: char_start as u32,
                    },
                    end: Position {
                        line: line_idx as u32,
                        character: char_end as u32,
                    },
                },
                kind,
            });
        }
    }

    highlights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_variable() {
        let content = r#"fn main() {
    let x = 5
    println(x)
    let y = x + 1
}"#;
        let highlights = get_document_highlights(
            content,
            Position {
                line: 1,
                character: 8,
            },
        );

        assert_eq!(highlights.len(), 3);
        // Definition should be WRITE
        assert_eq!(highlights[0].kind, Some(DocumentHighlightKind::WRITE));
        // Usages should be READ
        assert_eq!(highlights[1].kind, Some(DocumentHighlightKind::READ));
        assert_eq!(highlights[2].kind, Some(DocumentHighlightKind::READ));
    }

    #[test]
    fn test_highlight_function() {
        let content = r#"fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    add(1, 2)
    add(3, 4)
}"#;
        let highlights = get_document_highlights(
            content,
            Position {
                line: 5,
                character: 4,
            },
        );

        assert_eq!(highlights.len(), 3);
        // Definition
        assert_eq!(highlights[0].kind, Some(DocumentHighlightKind::WRITE));
        // Calls
        assert_eq!(highlights[1].kind, Some(DocumentHighlightKind::READ));
        assert_eq!(highlights[2].kind, Some(DocumentHighlightKind::READ));
    }

    #[test]
    fn test_highlight_assignment() {
        let content = r#"fn main() {
    var x = 0
    x = 5
    println(x)
}"#;
        let highlights = get_document_highlights(
            content,
            Position {
                line: 1,
                character: 8,
            },
        );

        assert_eq!(highlights.len(), 3);
        // Definition - WRITE
        assert_eq!(highlights[0].kind, Some(DocumentHighlightKind::WRITE));
        // Assignment - WRITE
        assert_eq!(highlights[1].kind, Some(DocumentHighlightKind::WRITE));
        // Read
        assert_eq!(highlights[2].kind, Some(DocumentHighlightKind::READ));
    }

    #[test]
    fn test_skip_strings() {
        let content = r#"fn main() {
    let foo = "foo"
    println(foo)
}"#;
        let highlights = get_document_highlights(
            content,
            Position {
                line: 1,
                character: 8,
            },
        );

        // Should find 2: definition and println usage, not the string content
        assert_eq!(highlights.len(), 2);
    }

    #[test]
    fn test_unicode() {
        let content = r#"fn main() {
    let 变量 = 5
    println(变量)
}"#;
        let highlights = get_document_highlights(
            content,
            Position {
                line: 1,
                character: 8,
            },
        );

        assert_eq!(highlights.len(), 2);
    }
}
