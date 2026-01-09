//! Go-to-definition support for Lira
//!
//! Finds symbol definitions within the document.

use crate::utils::{self, get_regex};
use tower_lsp::lsp_types::*;

/// Find the definition of the symbol at the given position
pub fn find_definition(uri: &Url, content: &str, position: Position) -> Option<Location> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at the cursor
    let word_info = utils::get_word_at_position(line, col)?;

    // Search for the definition
    find_symbol_definition(uri, content, &word_info.text)
}

fn find_symbol_definition(uri: &Url, content: &str, symbol: &str) -> Option<Location> {
    // Patterns for different definition types
    let patterns = [
        // Function: fn name(
        format!(r"fn\s+{}\s*[<(]", regex::escape(symbol)),
        // Struct: struct Name
        format!(r"struct\s+{}\s*[<{{]", regex::escape(symbol)),
        // Class: class Name
        format!(r"class\s+{}\s*[<{{]", regex::escape(symbol)),
        // Enum: enum Name
        format!(r"enum\s+{}\s*[<{{]", regex::escape(symbol)),
        // Trait: trait Name
        format!(r"trait\s+{}\s*[<{{]", regex::escape(symbol)),
        // Type alias: type Name
        format!(r"type\s+{}\s*[<=]", regex::escape(symbol)),
        // Variable: let/var/const name
        format!(r"(?:let|var|const)\s+{}\s*[=:]", regex::escape(symbol)),
        // Parameter: name: Type
        format!(r"\(\s*.*?\s*{}\s*:", regex::escape(symbol)),
    ];

    for pattern in patterns {
        if let Some(location) = find_pattern_in_content(uri, content, &pattern, symbol) {
            return Some(location);
        }
    }

    None
}

fn find_pattern_in_content(
    uri: &Url,
    content: &str,
    pattern: &str,
    symbol: &str,
) -> Option<Location> {
    let re = get_regex(pattern)?;

    for (line_idx, line) in content.lines().enumerate() {
        if let Some(m) = re.find(line) {
            // Skip if in string or comment
            if utils::is_in_string_or_comment(line, m.start()) {
                continue;
            }

            // Find the exact position of the symbol name within the match
            if let Some(symbol_pos) = line[m.start()..].find(symbol) {
                let char_offset = m.start() + symbol_pos;
                // Convert byte offset to character offset for UTF-8
                let char_start = utils::byte_offset_to_char_col(line, char_offset);
                let char_end = char_start + symbol.chars().count();

                return Some(Location {
                    uri: uri.clone(),
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
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_function_definition() {
        let content = r#"
fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    add(1, 2)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let result = find_symbol_definition(&uri, content, "add");

        assert!(result.is_some());
        let location = result.unwrap();
        assert_eq!(location.range.start.line, 1);
    }

    #[test]
    fn test_find_struct_definition() {
        let content = r#"
struct Point {
    x: float,
    y: float,
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 }
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let result = find_symbol_definition(&uri, content, "Point");

        assert!(result.is_some());
        let location = result.unwrap();
        assert_eq!(location.range.start.line, 1);
    }

    #[test]
    fn test_find_variable_definition() {
        let content = r#"
fn main() {
    let count = 0
    count + 1
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let result = find_symbol_definition(&uri, content, "count");

        assert!(result.is_some());
        let location = result.unwrap();
        assert_eq!(location.range.start.line, 2);
    }

    #[test]
    fn test_find_unicode_variable() {
        let content = r#"
fn main() {
    let 变量 = 5
    println(变量)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let result = find_symbol_definition(&uri, content, "变量");

        assert!(result.is_some());
    }

    #[test]
    fn test_skip_definition_in_string() {
        let content = r#"
fn main() {
    println("fn foo() { }")
}

fn foo() {}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let result = find_symbol_definition(&uri, content, "foo");

        assert!(result.is_some());
        // Should find the real definition, not the one in string
        assert_eq!(result.unwrap().range.start.line, 5);
    }
}
