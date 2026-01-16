//! Go to type definition support for Lira
//!
//! Navigates to the type definition of a variable or expression.

use crate::utils::{self, get_regex};
use tower_lsp::lsp_types::*;

/// Find the type definition for the symbol at position
pub fn find_type_definition(uri: &Url, content: &str, position: Position) -> Option<Location> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at cursor
    let word_info = utils::get_word_at_position(line, col)?;

    // Find the type of this symbol
    let type_name = find_symbol_type(content, &word_info.text, line_idx)?;

    // Now find the definition of that type
    find_type_location(uri, content, &type_name)
}

/// Find the type of a symbol by searching for its declaration
fn find_symbol_type(content: &str, symbol: &str, usage_line: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();

    // Pattern for variable declarations with explicit type: let/var/const name: Type
    let explicit_type_pattern = format!(
        r"(?:let|var|const)\s+{}\s*:\s*([A-Z][a-zA-Z0-9_<>]*)",
        regex::escape(symbol)
    );

    // Pattern for function parameters: name: Type
    let param_pattern = format!(r"\b{}\s*:\s*([A-Z][a-zA-Z0-9_<>]*)", regex::escape(symbol));

    // Search backward from usage line for declaration
    for line_idx in (0..=usage_line).rev() {
        let line = lines.get(line_idx)?;

        // Check for explicit type annotation
        if let Some(re) = get_regex(&explicit_type_pattern) {
            if let Some(caps) = re.captures(line) {
                if let Some(type_match) = caps.get(1) {
                    return Some(extract_base_type(type_match.as_str()));
                }
            }
        }

        // Check in function parameters
        if let Some(re) = get_regex(&param_pattern) {
            if let Some(caps) = re.captures(line) {
                if let Some(type_match) = caps.get(1) {
                    return Some(extract_base_type(type_match.as_str()));
                }
            }
        }
    }

    // Try to infer type from initializer
    infer_type_from_initializer(content, symbol, usage_line)
}

/// Extract base type from a potentially generic type (List<int> -> List)
fn extract_base_type(type_str: &str) -> String {
    if let Some(idx) = type_str.find('<') {
        type_str[..idx].to_string()
    } else {
        type_str.to_string()
    }
}

/// Try to infer type from initializer expression
fn infer_type_from_initializer(content: &str, symbol: &str, usage_line: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();

    // Pattern for constructor calls: let x = TypeName { ... } or TypeName::new(...)
    let init_pattern = format!(
        r"(?:let|var)\s+{}\s*=\s*([A-Z][a-zA-Z0-9_]*)\s*(?:\{{|::)",
        regex::escape(symbol)
    );

    for line_idx in (0..=usage_line).rev() {
        let line = lines.get(line_idx)?;

        if let Some(re) = get_regex(&init_pattern) {
            if let Some(caps) = re.captures(line) {
                if let Some(type_match) = caps.get(1) {
                    return Some(type_match.as_str().to_string());
                }
            }
        }
    }

    None
}

/// Find the location of a type definition
fn find_type_location(uri: &Url, content: &str, type_name: &str) -> Option<Location> {
    // Skip built-in types
    let builtin_types = [
        "int", "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "float",
        "bool", "string", "char", "void", "List", "Map", "Set", "Option", "Result", "Channel",
    ];

    if builtin_types.contains(&type_name) {
        return None;
    }

    // Use the definition finder to locate the type
    // We create a fake position and look for the type definition directly
    let type_def_patterns = [
        format!(r"struct\s+{}\s*[<{{]", regex::escape(type_name)),
        format!(r"class\s+{}\s*[<{{]", regex::escape(type_name)),
        format!(r"enum\s+{}\s*[<{{]", regex::escape(type_name)),
        format!(r"trait\s+{}\s*[<{{]", regex::escape(type_name)),
        format!(r"type\s+{}\s*[<=]", regex::escape(type_name)),
    ];

    for (line_idx, line) in content.lines().enumerate() {
        for pattern in &type_def_patterns {
            if let Some(re) = get_regex(pattern) {
                if let Some(m) = re.find(line) {
                    // Skip if in string or comment
                    if utils::is_in_string_or_comment(line, m.start()) {
                        continue;
                    }

                    // Find the type name within the match
                    if let Some(name_re) =
                        get_regex(&format!(r"\b{}\b", regex::escape(type_name)))
                    {
                        if let Some(name_match) = name_re.find(line) {
                            let char_start = utils::byte_offset_to_char_col(line, name_match.start());
                            let char_end = utils::byte_offset_to_char_col(line, name_match.end());

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
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explicit_type_annotation() {
        let content = r#"
struct Point {
    x: int,
    y: int,
}

fn main() {
    let p: Point = Point { x: 1, y: 2 }
    println(p)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        // Position on "p" in println(p)
        let loc = find_type_definition(&uri, content, Position { line: 8, character: 12 });

        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert_eq!(loc.range.start.line, 1); // Point struct definition
    }

    #[test]
    fn test_inferred_from_constructor() {
        let content = r#"
struct Point {
    x: int,
    y: int,
}

fn main() {
    let p = Point { x: 1, y: 2 }
    println(p)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let loc = find_type_definition(&uri, content, Position { line: 8, character: 12 });

        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert_eq!(loc.range.start.line, 1);
    }

    #[test]
    fn test_function_parameter() {
        let content = r#"
struct Point {
    x: int,
    y: int,
}

fn print_point(p: Point) {
    println(p.x)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        // Position on "p" in println(p.x)
        let loc = find_type_definition(&uri, content, Position { line: 7, character: 12 });

        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert_eq!(loc.range.start.line, 1);
    }

    #[test]
    fn test_builtin_type_returns_none() {
        let content = r#"
fn main() {
    let x: int = 42
    println(x)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let loc = find_type_definition(&uri, content, Position { line: 3, character: 12 });

        // int is a builtin, should return None
        assert!(loc.is_none());
    }

    #[test]
    fn test_generic_type() {
        let content = r#"
struct Container<T> {
    value: T,
}

fn main() {
    let c: Container<int> = Container { value: 42 }
    println(c)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let loc = find_type_definition(&uri, content, Position { line: 7, character: 12 });

        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert_eq!(loc.range.start.line, 1); // Container struct
    }
}
