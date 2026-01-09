//! Rename support for Lira
//!
//! Provides symbol renaming across the document.

use crate::references;
use crate::utils;
use std::collections::HashMap;
use tower_lsp::lsp_types::*;

/// Prepare rename - check if symbol at position can be renamed
pub fn prepare_rename(content: &str, position: Position) -> Option<PrepareRenameResponse> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at cursor
    let word_info = utils::get_word_at_position(line, col)?;

    // Check if this is a renameable symbol (not a keyword)
    if utils::is_keyword(&word_info.text) {
        return None;
    }

    Some(PrepareRenameResponse::Range(Range {
        start: Position {
            line: position.line,
            character: word_info.start_col as u32,
        },
        end: Position {
            line: position.line,
            character: word_info.end_col as u32,
        },
    }))
}

/// Perform rename - return workspace edit with all changes
pub fn rename(
    uri: &Url,
    content: &str,
    position: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at cursor
    let word_info = utils::get_word_at_position(line, col)?;

    // Check if this is a renameable symbol
    if utils::is_keyword(&word_info.text) {
        return None;
    }

    // Validate new name
    if !is_valid_identifier(new_name) {
        return None;
    }

    // Find all references to this symbol
    let refs = references::find_references(uri, content, position, true);

    if refs.is_empty() {
        return None;
    }

    // Build text edits
    let edits: Vec<TextEdit> = refs
        .into_iter()
        .map(|loc| TextEdit {
            range: loc.range,
            new_text: new_name.to_string(),
        })
        .collect();

    // Group edits by URI
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

/// Check if a string is a valid Lira identifier
fn is_valid_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();

    // First character must be letter or underscore
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }

    // Rest must be alphanumeric or underscore
    for c in chars {
        if !c.is_alphanumeric() && c != '_' {
            return false;
        }
    }

    // Cannot be a keyword
    !utils::is_keyword(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_word_at_position() {
        let line = "let foo = 42";
        let word = utils::get_word_at_position(line, 4).unwrap();
        assert_eq!(word.text, "foo");
        assert_eq!(word.start_col, 4);
        assert_eq!(word.end_col, 7);

        let word = utils::get_word_at_position(line, 0).unwrap();
        assert_eq!(word.text, "let");
    }

    #[test]
    fn test_is_keyword() {
        assert!(utils::is_keyword("fn"));
        assert!(utils::is_keyword("let"));
        assert!(utils::is_keyword("if"));
        assert!(!utils::is_keyword("foo"));
        assert!(!utils::is_keyword("myFunc"));
    }

    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("foo"));
        assert!(is_valid_identifier("_bar"));
        assert!(is_valid_identifier("baz123"));
        assert!(!is_valid_identifier("123abc")); // starts with number
        assert!(!is_valid_identifier("")); // empty
        assert!(!is_valid_identifier("fn")); // keyword
        assert!(!is_valid_identifier("foo-bar")); // contains hyphen
    }

    #[test]
    fn test_prepare_rename_on_keyword() {
        let content = "let x = 42";
        let pos = Position {
            line: 0,
            character: 0,
        }; // on "let"
        assert!(prepare_rename(content, pos).is_none());
    }

    #[test]
    fn test_prepare_rename_on_variable() {
        let content = "let foo = 42";
        let pos = Position {
            line: 0,
            character: 4,
        }; // on "foo"
        let result = prepare_rename(content, pos);
        assert!(result.is_some());
    }

    #[test]
    fn test_rename() {
        let content = r#"fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    let result = add(1, 2)
    println(result)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let pos = Position {
            line: 5,
            character: 17,
        }; // on "add" in call

        let edit = rename(&uri, content, pos, "sum");
        assert!(edit.is_some());

        let edit = edit.unwrap();
        let changes = edit.changes.unwrap();
        let edits = changes.get(&uri).unwrap();

        // Should have 2 edits: definition and call site
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn test_rename_unicode() {
        let content = r#"fn main() {
    let 变量 = 5
    println(变量)
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let pos = Position {
            line: 1,
            character: 8,
        }; // on "变量"

        let edit = rename(&uri, content, pos, "value");
        assert!(edit.is_some());
    }
}
