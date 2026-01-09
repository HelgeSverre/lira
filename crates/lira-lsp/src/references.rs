//! Find references support for Lira
//!
//! Finds all references to a symbol across the document.

use crate::utils::{self, get_regex};
use tower_lsp::lsp_types::*;

/// Find all references to the symbol at the given position
pub fn find_references(
    uri: &Url,
    content: &str,
    position: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return vec![];
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at the cursor
    let word_info = match utils::get_word_at_position(line, col) {
        Some(w) => w,
        None => return vec![],
    };

    // Find all occurrences of this word as an identifier
    find_all_references(uri, content, &word_info.text, include_declaration)
}

fn find_all_references(
    uri: &Url,
    content: &str,
    symbol: &str,
    include_declaration: bool,
) -> Vec<Location> {
    let mut references = Vec::new();

    // Pattern to match the symbol as a whole word (identifier)
    let pattern = format!(r"\b{}\b", regex::escape(symbol));
    let re = match get_regex(&pattern) {
        Some(r) => r,
        None => return references,
    };

    // Patterns for definitions (to optionally exclude them)
    let def_patterns = [
        format!(r"fn\s+{}\s*[<(]", regex::escape(symbol)),
        format!(r"struct\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"class\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"enum\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"trait\s+{}\s*[<{{]", regex::escape(symbol)),
        format!(r"type\s+{}\s*[<=]", regex::escape(symbol)),
        format!(r"(?:let|var|const)\s+{}\s*[=:]", regex::escape(symbol)),
    ];

    let def_regexes: Vec<_> = def_patterns
        .iter()
        .filter_map(|p| get_regex(p))
        .collect();

    for (line_idx, line) in content.lines().enumerate() {
        // Check if this line is a definition
        let is_definition = def_regexes.iter().any(|r| r.is_match(line));

        // Skip definitions if not including them
        if is_definition && !include_declaration {
            continue;
        }

        // Find all matches in the line
        for m in re.find_iter(line) {
            // Skip if it's inside a string or comment
            if utils::is_in_string_or_comment(line, m.start()) {
                continue;
            }

            // Convert byte positions to character positions for UTF-8
            let char_start = utils::byte_offset_to_char_col(line, m.start());
            let char_end = utils::byte_offset_to_char_col(line, m.end());

            references.push(Location {
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

    references
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_function_references() {
        let content = r#"
fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    let x = add(1, 2)
    let y = add(3, 4)
    println(add(x, y))
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let refs = find_all_references(&uri, content, "add", true);

        // Should find: definition + 3 usages = 4
        assert_eq!(refs.len(), 4);
    }

    #[test]
    fn test_find_references_exclude_declaration() {
        let content = r#"
fn foo() {
    bar()
}

fn bar() {
    foo()
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let refs = find_all_references(&uri, content, "foo", false);

        // Should find only the usage in bar(), not the definition
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_skip_string_content() {
        let content = r#"
fn test() {
    println("test is a word")
    test()
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let refs = find_all_references(&uri, content, "test", true);

        // Should find definition and call, but NOT the string content
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_skip_comments() {
        let content = r#"
fn foo() {
    // call foo here
    bar()
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let refs = find_all_references(&uri, content, "foo", true);

        // Should find only definition, not the comment
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_unicode_references() {
        let content = r#"
fn main() {
    let 变量 = 5
    println(变量)
    let x = 变量 + 1
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let refs = find_all_references(&uri, content, "变量", true);

        // Should find: definition + 2 usages = 3
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn test_skip_escaped_string() {
        let content = r#"
fn test() {
    println("escaped \"test\" quote")
    test()
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let refs = find_all_references(&uri, content, "test", true);

        // Should find definition and call, but NOT the string content
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_skip_char_literal() {
        let content = r#"
fn main() {
    let c = 'a'
    let a = 5
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let refs = find_all_references(&uri, content, "a", true);

        // Should find only the variable 'a', not the char literal
        assert_eq!(refs.len(), 1);
    }
}
