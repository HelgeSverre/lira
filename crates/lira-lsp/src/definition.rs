//! Go-to-definition support for Lira
//!
//! Scope-aware: resolves the binding under the cursor to a `SymbolId` (or a
//! struct member) via the type checker's semantic tables and jumps to its
//! declaration, so shadowed locals land on the correct `let`. Falls back to a
//! textual search for symbols the semantic tables don't track (struct/enum/
//! trait names in type position, imported symbols).

use crate::sema_refs;
use crate::utils::{self, get_regex};
use tower_lsp::lsp_types::*;

/// Find the definition of the symbol at the given position.
pub fn find_definition(uri: &Url, content: &str, position: Position) -> Option<Location> {
    // Prefer scope-aware resolution: jump to the binding's actual declaration
    // rather than the first textual match. Only value bindings and struct
    // members are tracked here; type names fall through to the regex search.
    if let Some(range) = semantic_definition(content, position) {
        return Some(Location {
            uri: uri.clone(),
            range,
        });
    }

    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at the cursor
    let word_info = utils::get_word_at_position(line, col)?;

    // Fall back to a textual search for symbols not tracked semantically.
    find_symbol_definition(uri, content, &word_info.text)
}

/// Resolve the declaration range under the cursor via the semantic tables.
/// Returns `None` when the buffer fails to parse or the cursor is not on a
/// tracked binding/member (the caller then falls back to the textual search).
fn semantic_definition(content: &str, position: Position) -> Option<Range> {
    let analysis = lirac::analyze(content).ok()?;

    // A value binding (variable, parameter, function name) resolved to a
    // scope-correct `SymbolId`.
    if let Some(sym_id) = sema_refs::resolve_symbol_at(&analysis, content, position) {
        if let Some(range) = sema_refs::decl_range(&analysis, content, sym_id) {
            return Some(range);
        }
    }

    // A struct field or method, scoped by its owner type.
    if let Some(key) = sema_refs::resolve_member_at(&analysis, content, position) {
        if let Some(range) = sema_refs::member_decl_range(&analysis, content, &key) {
            return Some(range);
        }
    }

    None
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

    // ---- Scope-aware go-to-definition (via the semantic tables) ----

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn test_goto_def_shadowed_inner_binding_jumps_to_inner() {
        // Two distinct `x` bindings: outer (line 0) and inner inside `f` (line
        // 2). Go-to-definition from the inner use `return x` must land on the
        // INNER `let x = 2`, never the outer one. A regex search returns the
        // first textual match (outer), so this only passes when scope-aware.
        let content = "let x = 1\nfn f() -> int {\n    let x = 2\n    return x\n}\nlet y = x\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // Cursor on `return x` (line 3, col 11).
        let loc = find_definition(&uri, content, pos(3, 11)).expect("resolves");
        assert_eq!(loc.range.start.line, 2, "inner decl, got {:?}", loc.range);
    }

    #[test]
    fn test_goto_def_outer_use_jumps_to_outer() {
        let content = "let x = 1\nfn f() -> int {\n    let x = 2\n    return x\n}\nlet y = x\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // Cursor on the outer use `let y = x` (line 5, col 8).
        let loc = find_definition(&uri, content, pos(5, 8)).expect("resolves");
        assert_eq!(loc.range.start.line, 0, "outer decl, got {:?}", loc.range);
    }

    #[test]
    fn test_goto_def_parameter_use_jumps_to_param() {
        // A use of a parameter jumps to the parameter, not to a same-named
        // top-level binding.
        let content = "let a = 99\nfn add(a: int, b: int) -> int {\n    return a + b\n}\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // Cursor on `return a` (line 2, col 11).
        let loc = find_definition(&uri, content, pos(2, 11)).expect("resolves");
        assert_eq!(loc.range.start.line, 1, "param decl, got {:?}", loc.range);
    }

    #[test]
    fn test_goto_def_type_name_falls_back_to_struct_decl() {
        // Struct/enum/trait names in type position aren't value bindings, so the
        // regex fallback still resolves them.
        let content =
            "struct Point {\n    x: int,\n}\nfn main() {\n    let p = Point { x: 1 }\n}\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // Cursor on `Point` in the literal (line 4, col 12).
        let loc = find_definition(&uri, content, pos(4, 12)).expect("resolves");
        assert_eq!(loc.range.start.line, 0, "struct decl, got {:?}", loc.range);
    }
}
