//! Code actions support for Lira
//!
//! Provides quick fixes and refactoring suggestions:
//! - Convert between `let` and `var` (mutability toggle)
//! - Add documentation comments to declarations
//! - Organize imports (sort and deduplicate)
//! - Generate impl block for structs
//! - Extract variable from expression

use regex::Regex;
use tower_lsp::lsp_types::*;

/// Get available code actions for the given range
pub fn get_code_actions(
    uri: &Url,
    content: &str,
    range: Range,
    _diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Get the line at the cursor
    let line_idx = range.start.line as usize;
    if line_idx >= lines.len() {
        return actions;
    }
    let line = lines[line_idx];

    // Check for mutability toggle (let <-> var)
    if let Some(action) = mutability_toggle_action(uri, line, line_idx) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Check for adding documentation
    if let Some(action) = add_doc_comment_action(uri, content, line, line_idx) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Check for generating impl block
    if let Some(action) = generate_impl_action(uri, content, line, line_idx) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Check for organize imports
    if let Some(action) = organize_imports_action(uri, content) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Check for extract variable
    if range.start != range.end {
        if let Some(action) = extract_variable_action(uri, content, range) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }

    actions
}

/// Toggle between `let` and `var`
fn mutability_toggle_action(uri: &Url, line: &str, line_idx: usize) -> Option<CodeAction> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];

    // Check for let declaration
    if trimmed.starts_with("let ") {
        let new_line = format!("{}var {}", indent, &trimmed[4..]);
        return Some(create_replace_line_action(
            uri,
            "Convert to var (mutable)",
            line_idx,
            line,
            &new_line,
            CodeActionKind::REFACTOR_REWRITE,
        ));
    }

    // Check for var declaration
    if trimmed.starts_with("var ") {
        let new_line = format!("{}let {}", indent, &trimmed[4..]);
        return Some(create_replace_line_action(
            uri,
            "Convert to let (immutable)",
            line_idx,
            line,
            &new_line,
            CodeActionKind::REFACTOR_REWRITE,
        ));
    }

    None
}

/// Add documentation comment to a declaration
fn add_doc_comment_action(
    uri: &Url,
    content: &str,
    line: &str,
    line_idx: usize,
) -> Option<CodeAction> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];

    // Check if this is a documentable declaration
    let decl_type = if trimmed.starts_with("fn ") {
        Some("function")
    } else if trimmed.starts_with("struct ") {
        Some("struct")
    } else if trimmed.starts_with("enum ") {
        Some("enum")
    } else if trimmed.starts_with("trait ") {
        Some("trait")
    } else if trimmed.starts_with("class ") {
        Some("class")
    } else if trimmed.starts_with("const ") {
        Some("constant")
    } else {
        None
    }?;

    // Check if there's already a doc comment
    if line_idx > 0 {
        let prev_line = content.lines().nth(line_idx - 1).unwrap_or("");
        if prev_line.trim_start().starts_with("///") {
            return None;
        }
    }

    // Extract the name for the placeholder
    let name = extract_declaration_name(trimmed)?;

    let doc_comment = format!("{}/// TODO: Document this {}\n", indent, decl_type);

    let edit = TextEdit {
        range: Range {
            start: Position {
                line: line_idx as u32,
                character: 0,
            },
            end: Position {
                line: line_idx as u32,
                character: 0,
            },
        },
        new_text: doc_comment,
    };

    Some(CodeAction {
        title: format!("Add documentation for `{}`", name),
        kind: Some(CodeActionKind::REFACTOR),
        edit: Some(WorkspaceEdit {
            changes: Some([(uri.clone(), vec![edit])].into_iter().collect()),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Generate an impl block for a struct
fn generate_impl_action(
    uri: &Url,
    content: &str,
    line: &str,
    line_idx: usize,
) -> Option<CodeAction> {
    let trimmed = line.trim_start();

    // Only for struct declarations
    if !trimmed.starts_with("struct ") {
        return None;
    }

    let struct_name = extract_declaration_name(trimmed)?;

    // Check if there's already an impl block for this struct
    let impl_pattern = format!(r"impl\s+{}\s*\{{", regex::escape(&struct_name));
    let impl_re = Regex::new(&impl_pattern).ok()?;
    if impl_re.is_match(content) {
        return None;
    }

    // Find the end of the struct definition
    let lines: Vec<&str> = content.lines().collect();
    let mut brace_depth = 0;
    let mut struct_end_line = line_idx;

    for (idx, l) in lines.iter().enumerate().skip(line_idx) {
        for ch in l.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        struct_end_line = idx;
                        break;
                    }
                }
                _ => {}
            }
        }
        if brace_depth == 0 && struct_end_line != line_idx {
            break;
        }
    }

    let impl_block = format!(
        "\n\nimpl {} {{\n    // TODO: Add methods\n}}\n",
        struct_name
    );

    let insert_line = struct_end_line + 1;
    let edit = TextEdit {
        range: Range {
            start: Position {
                line: insert_line as u32,
                character: 0,
            },
            end: Position {
                line: insert_line as u32,
                character: 0,
            },
        },
        new_text: impl_block,
    };

    Some(CodeAction {
        title: format!("Generate impl block for `{}`", struct_name),
        kind: Some(CodeActionKind::REFACTOR),
        edit: Some(WorkspaceEdit {
            changes: Some([(uri.clone(), vec![edit])].into_iter().collect()),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Organize imports (sort and deduplicate)
fn organize_imports_action(uri: &Url, content: &str) -> Option<CodeAction> {
    let lines: Vec<&str> = content.lines().collect();

    // Find all import lines and their positions
    let mut import_groups: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut current_group_start: Option<usize> = None;
    let mut current_imports: Vec<String> = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") || trimmed.starts_with("use ") {
            if current_group_start.is_none() {
                current_group_start = Some(idx);
            }
            current_imports.push(trimmed.to_string());
        } else if !trimmed.is_empty() {
            // Non-import, non-empty line ends the group
            if let Some(start) = current_group_start {
                if !current_imports.is_empty() {
                    import_groups.push((start, idx, current_imports.clone()));
                    current_imports.clear();
                }
                current_group_start = None;
            }
        }
    }

    // Handle final group
    if let Some(start) = current_group_start {
        if !current_imports.is_empty() {
            import_groups.push((start, lines.len(), current_imports));
        }
    }

    if import_groups.is_empty() {
        return None;
    }

    // Check if any group needs sorting
    let mut needs_action = false;
    for (_, _, imports) in &import_groups {
        let mut sorted = imports.clone();
        sorted.sort();
        sorted.dedup();
        if sorted != *imports {
            needs_action = true;
            break;
        }
    }

    if !needs_action {
        return None;
    }

    // Create edits for each group
    let mut edits = Vec::new();
    for (start, end, imports) in import_groups {
        let mut sorted = imports.clone();
        sorted.sort();
        sorted.dedup();

        let new_text = sorted.join("\n") + "\n";
        edits.push(TextEdit {
            range: Range {
                start: Position {
                    line: start as u32,
                    character: 0,
                },
                end: Position {
                    line: end as u32,
                    character: 0,
                },
            },
            new_text,
        });
    }

    Some(CodeAction {
        title: "Organize imports".to_string(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        edit: Some(WorkspaceEdit {
            changes: Some([(uri.clone(), edits)].into_iter().collect()),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Extract selected expression to a variable
fn extract_variable_action(uri: &Url, content: &str, range: Range) -> Option<CodeAction> {
    let lines: Vec<&str> = content.lines().collect();

    // Only handle single-line selections for now
    if range.start.line != range.end.line {
        return None;
    }

    let line_idx = range.start.line as usize;
    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let start_char = range.start.character as usize;
    let end_char = range.end.character as usize;

    if end_char > line.len() || start_char >= end_char {
        return None;
    }

    let selected_text = &line[start_char..end_char];

    // Don't extract if it's just whitespace or already a simple identifier
    if selected_text.trim().is_empty() {
        return None;
    }

    // Simple check: skip if it looks like a simple variable name
    let is_simple_ident = selected_text
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_');
    if is_simple_ident {
        return None;
    }

    let indent = &line[..line.len() - line.trim_start().len()];
    let var_name = "extracted";

    // Create the variable declaration
    let var_decl = format!("{}let {} = {}\n", indent, var_name, selected_text);

    // Replace selected text with variable name
    let new_line = format!("{}{}{}", &line[..start_char], var_name, &line[end_char..]);

    let edits = vec![
        // Insert variable declaration
        TextEdit {
            range: Range {
                start: Position {
                    line: line_idx as u32,
                    character: 0,
                },
                end: Position {
                    line: line_idx as u32,
                    character: 0,
                },
            },
            new_text: var_decl,
        },
        // Replace selected expression with variable
        TextEdit {
            range: Range {
                start: Position {
                    line: line_idx as u32,
                    character: 0,
                },
                end: Position {
                    line: line_idx as u32,
                    character: line.len() as u32,
                },
            },
            new_text: new_line,
        },
    ];

    Some(CodeAction {
        title: "Extract to variable".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(WorkspaceEdit {
            changes: Some([(uri.clone(), edits)].into_iter().collect()),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Helper: Extract declaration name from a line
fn extract_declaration_name(line: &str) -> Option<String> {
    let re = Regex::new(r"(?:fn|struct|enum|trait|class|const)\s+([a-zA-Z_][a-zA-Z0-9_]*)").ok()?;
    re.captures(line)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Helper: Create a code action that replaces a line
fn create_replace_line_action(
    uri: &Url,
    title: &str,
    line_idx: usize,
    old_line: &str,
    new_line: &str,
    kind: CodeActionKind,
) -> CodeAction {
    let edit = TextEdit {
        range: Range {
            start: Position {
                line: line_idx as u32,
                character: 0,
            },
            end: Position {
                line: line_idx as u32,
                character: old_line.len() as u32,
            },
        },
        new_text: new_line.to_string(),
    };

    CodeAction {
        title: title.to_string(),
        kind: Some(kind),
        edit: Some(WorkspaceEdit {
            changes: Some([(uri.clone(), vec![edit])].into_iter().collect()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uri() -> Url {
        Url::parse("file:///test.li").unwrap()
    }

    #[test]
    fn test_mutability_toggle_let_to_var() {
        let line = "    let x = 5";
        let action = mutability_toggle_action(&test_uri(), line, 0).unwrap();
        assert_eq!(action.title, "Convert to var (mutable)");
    }

    #[test]
    fn test_mutability_toggle_var_to_let() {
        let line = "    var x = 5";
        let action = mutability_toggle_action(&test_uri(), line, 0).unwrap();
        assert_eq!(action.title, "Convert to let (immutable)");
    }

    #[test]
    fn test_add_doc_comment() {
        let content = "fn hello() {\n    println(\"hello\")\n}";
        let action = add_doc_comment_action(&test_uri(), content, "fn hello() {", 0).unwrap();
        assert!(action.title.contains("Add documentation"));
    }

    #[test]
    fn test_no_doc_if_already_present() {
        let content = "/// Docs\nfn hello() {\n}";
        let action = add_doc_comment_action(&test_uri(), content, "fn hello() {", 1);
        assert!(action.is_none());
    }

    #[test]
    fn test_generate_impl() {
        let content = "struct Point {\n    x: int,\n    y: int,\n}";
        let action = generate_impl_action(&test_uri(), content, "struct Point {", 0).unwrap();
        assert!(action.title.contains("Generate impl"));
    }

    #[test]
    fn test_no_impl_if_exists() {
        let content = "struct Point { x: int }\n\nimpl Point {\n}";
        let action = generate_impl_action(&test_uri(), content, "struct Point { x: int }", 0);
        assert!(action.is_none());
    }

    #[test]
    fn test_organize_imports_unsorted() {
        let content = "import std.io\nimport std.fs\nimport std.collections\n\nfn main() {}";
        let action = organize_imports_action(&test_uri(), content).unwrap();
        assert_eq!(action.title, "Organize imports");
    }

    #[test]
    fn test_organize_imports_already_sorted() {
        let content = "import std.collections\nimport std.fs\nimport std.io\n\nfn main() {}";
        let action = organize_imports_action(&test_uri(), content);
        assert!(action.is_none());
    }

    #[test]
    fn test_extract_declaration_name() {
        assert_eq!(
            extract_declaration_name("fn hello() {"),
            Some("hello".to_string())
        );
        assert_eq!(
            extract_declaration_name("struct Point {"),
            Some("Point".to_string())
        );
        assert_eq!(
            extract_declaration_name("enum Color {"),
            Some("Color".to_string())
        );
    }
}
