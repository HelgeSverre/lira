//! Rename support for Lira
//!
//! Provides symbol renaming across the document.

use crate::sema_refs;
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

/// Perform rename - return a workspace edit covering exactly the binding's
/// declaration and every use, resolved scope-aware via the semantic tables.
///
/// Returns `None` when the new name is not a legal identifier, the buffer does
/// not parse, or the cursor is not on a renameable binding.
pub fn rename(
    uri: &Url,
    content: &str,
    position: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    // Validate the new name up front (legal identifier, not a keyword).
    if !is_valid_identifier(new_name) {
        return None;
    }

    let analysis = lirac::analyze(content).ok()?;

    // Resolve the target as either a variable/param/fn-name binding (a
    // `SymbolId`) or a struct field/method (scoped by owner type).
    let ranges = sema_refs::resolve_symbol_at(&analysis, content, position)
        .map(|sym_id| sema_refs::collect_symbol_ranges(&analysis, content, sym_id, true))
        .or_else(|| {
            sema_refs::resolve_member_at(&analysis, content, position)
                .map(|key| sema_refs::collect_member_ranges(&analysis, content, &key, true))
        })?;

    if ranges.is_empty() {
        return None;
    }

    let edits: Vec<TextEdit> = ranges
        .into_iter()
        .map(|range| TextEdit {
            range,
            new_text: new_name.to_string(),
        })
        .collect();

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

    fn edits_for(uri: &Url, edit: &WorkspaceEdit) -> Vec<TextEdit> {
        edit.changes.as_ref().unwrap().get(uri).unwrap().clone()
    }

    #[test]
    fn test_rename() {
        let content = "fn add(a: int, b: int) -> int {\n    a + b\n}\n\nfn main() {\n    let result = add(1, 2)\n    println(result)\n}\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // On `add` in the call (line 5, col 17).
        let pos = Position {
            line: 5,
            character: 17,
        };

        let edit = rename(&uri, content, pos, "sum").expect("rename produces an edit");
        let edits = edits_for(&uri, &edit);
        // Definition + call site = 2.
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().all(|e| e.new_text == "sum"));
    }

    #[test]
    fn test_rename_only_targeted_binding() {
        // Two distinct `x` bindings. Renaming the inner one must not touch the
        // outer same-named binding.
        let content = "let x = 1\nfn f() -> int {\n    let x = 2\n    return x\n}\nlet y = x\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // On `return x` (line 3, col 11) — the inner binding.
        let pos = Position {
            line: 3,
            character: 11,
        };
        let edit = rename(&uri, content, pos, "z").expect("rename produces an edit");
        let edits = edits_for(&uri, &edit);
        // Inner decl (line 2) + inner use (line 3) = 2.
        assert_eq!(edits.len(), 2);
        // Outer occurrences (lines 0 and 5) are untouched.
        assert!(edits.iter().all(|e| e.range.start.line != 0));
        assert!(edits.iter().all(|e| e.range.start.line != 5));
    }

    #[test]
    fn test_rename_invalid_name_rejected() {
        let content = "let foo = 42\nlet bar = foo\n";
        let uri = Url::parse("file:///test.li").unwrap();
        let pos = Position {
            line: 0,
            character: 4,
        }; // on `foo`
        assert!(rename(&uri, content, pos, "123abc").is_none());
        assert!(rename(&uri, content, pos, "fn").is_none()); // keyword
        assert!(rename(&uri, content, pos, "foo-bar").is_none());
    }

    #[test]
    fn test_rename_keyword_position_yields_none() {
        let content = "let x = 42\n";
        let uri = Url::parse("file:///test.li").unwrap();
        let pos = Position {
            line: 0,
            character: 0,
        }; // on `let`
        assert!(rename(&uri, content, pos, "y").is_none());
    }

    // ---- Struct fields & methods (scope-aware by owner type) ----

    #[test]
    fn test_rename_struct_field_updates_decl_and_accesses_only_on_that_type() {
        // (b) renaming a struct field updates the declaration + all accesses on
        // that type only.
        let content = "struct Point { x: int }\nstruct Box { x: int }\nfn main() {\n    let p = Point { x: 1 }\n    let q = Box { x: 9 }\n    let a = p.x\n    let b = q.x\n}\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // On `p.x` (line 5, col 14).
        let pos = Position {
            line: 5,
            character: 14,
        };
        let edit = rename(&uri, content, pos, "px").expect("rename produces an edit");
        let edits = edits_for(&uri, &edit);
        // Point.x decl (line 0) + Point literal field (line 3) + Point access
        // (line 5) = 3. The literal `Point { x: 1 }` MUST be renamed too.
        assert_eq!(edits.len(), 3, "got {:?}", edits);
        assert!(edits.iter().all(|e| e.new_text == "px"));
        assert!(
            edits.iter().any(|e| e.range.start.line == 3),
            "Point literal field renamed"
        );
        // Box.x decl (line 1), Box literal (line 4), Box access (line 6) untouched.
        assert!(edits.iter().all(|e| e.range.start.line != 1));
        assert!(edits.iter().all(|e| e.range.start.line != 4));
        assert!(edits.iter().all(|e| e.range.start.line != 6));
    }

    #[test]
    fn test_rename_method_updates_impl_decl_and_call_sites_only_on_that_type() {
        // (c) renaming a method updates its impl decl + all call sites on that
        // type. A same-named method on another type is left alone.
        let content = "struct Point { x: int }\nstruct Box { x: int }\nimpl Point {\n    fn get(self) -> int { self.x }\n}\nimpl Box {\n    fn get(self) -> int { self.x }\n}\nfn main() {\n    let p = Point { x: 1 }\n    let q = Box { x: 9 }\n    let a = p.get()\n    let b = q.get()\n}\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // On `p.get()` (line 11, col 14).
        let pos = Position {
            line: 11,
            character: 14,
        };
        let edit = rename(&uri, content, pos, "value").expect("rename produces an edit");
        let edits = edits_for(&uri, &edit);
        // Point::get impl decl (line 3) + the Point call site (line 11) = 2.
        assert_eq!(edits.len(), 2, "got {:?}", edits);
        assert!(edits.iter().all(|e| e.new_text == "value"));
        // Box::get decl (line 6) and the Box call site (line 12) untouched.
        assert!(edits.iter().all(|e| e.range.start.line != 6));
        assert!(edits.iter().all(|e| e.range.start.line != 12));
    }

    #[test]
    fn test_rename_field_from_declaration_site() {
        // Renaming initiated from the field declaration updates the decl + every
        // access on the owner type (including `self.x` inside methods).
        let content = "struct Point { x: int }\nimpl Point {\n    fn get(self) -> int { self.x }\n}\nfn main() {\n    let p = Point { x: 1 }\n    let a = p.x\n}\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // On the field decl `x` (line 0, col 15).
        let pos = Position {
            line: 0,
            character: 15,
        };
        let edit = rename(&uri, content, pos, "coord").expect("rename produces an edit");
        let edits = edits_for(&uri, &edit);
        // decl (0) + self.x (2) + struct-literal field (5) + p.x (6) = 4.
        assert_eq!(edits.len(), 4, "got {:?}", edits);
        assert!(edits.iter().all(|e| e.new_text == "coord"));
        assert!(
            edits.iter().any(|e| e.range.start.line == 5),
            "struct-literal field renamed (else the program won't compile)"
        );
    }

    #[test]
    fn test_rename_unicode() {
        let content = "fn main() {\n    let 变量 = 5\n    println(变量)\n}\n";
        let uri = Url::parse("file:///test.li").unwrap();
        // On the declaration `let 变量` (line 1, col 8).
        let pos = Position {
            line: 1,
            character: 8,
        };
        let edit = rename(&uri, content, pos, "value").expect("rename produces an edit");
        let edits = edits_for(&uri, &edit);
        // Declaration + one use = 2.
        assert_eq!(edits.len(), 2);
    }
}
