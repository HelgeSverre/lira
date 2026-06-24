//! Call hierarchy support for Lira
//!
//! Provides:
//! - Prepare call hierarchy (get function at cursor)
//! - Incoming calls (who calls this function)
//! - Outgoing calls (what functions does this call)

use crate::utils::{self, get_regex};
use tower_lsp::lsp_types::*;

/// Information about a function in the document
#[derive(Debug, Clone)]
struct FunctionInfo {
    name: String,
    line: u32,
    start_char: u32,
    end_char: u32,
    body_start_line: u32,
    body_end_line: u32,
}

/// Prepare call hierarchy item at position
pub fn prepare_call_hierarchy(
    uri: &Url,
    content: &str,
    position: Position,
) -> Option<Vec<CallHierarchyItem>> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at cursor
    let word_info = utils::get_word_at_position(line, col)?;

    // Find if this word is a function
    let functions = find_all_functions(content);
    let func = functions.iter().find(|f| f.name == word_info.text)?;

    Some(vec![CallHierarchyItem {
        name: func.name.clone(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri: uri.clone(),
        range: Range {
            start: Position {
                line: func.line,
                character: 0,
            },
            end: Position {
                line: func.body_end_line,
                character: lines
                    .get(func.body_end_line as usize)
                    .map_or(0, |l| l.chars().count() as u32),
            },
        },
        selection_range: Range {
            start: Position {
                line: func.line,
                character: func.start_char,
            },
            end: Position {
                line: func.line,
                character: func.end_char,
            },
        },
        data: None,
    }])
}

/// Find incoming calls (functions that call the given function)
pub fn incoming_calls(
    uri: &Url,
    content: &str,
    item: &CallHierarchyItem,
) -> Vec<CallHierarchyIncomingCall> {
    let target_name = &item.name;
    let functions = find_all_functions(content);
    let lines: Vec<&str> = content.lines().collect();

    let mut incoming = Vec::new();

    // Pattern to find function calls
    let call_pattern = format!(r"\b{}\s*\(", regex::escape(target_name));
    let call_re = match get_regex(&call_pattern) {
        Some(r) => r,
        None => return incoming,
    };

    for func in &functions {
        // Skip if this is the target function itself
        if func.name == *target_name {
            continue;
        }

        let mut from_ranges = Vec::new();

        // Search within this function's body for calls to target
        for line_idx in func.body_start_line..=func.body_end_line {
            if let Some(line) = lines.get(line_idx as usize) {
                for m in call_re.find_iter(line) {
                    // Skip if in comment or string
                    if utils::is_in_string_or_comment(line, m.start()) {
                        continue;
                    }

                    // Convert byte positions to character positions
                    let char_start = utils::byte_offset_to_char_col(line, m.start());
                    let char_end = utils::byte_offset_to_char_col(line, m.end() - 1); // Exclude the (

                    from_ranges.push(Range {
                        start: Position {
                            line: line_idx,
                            character: char_start as u32,
                        },
                        end: Position {
                            line: line_idx,
                            character: char_end as u32,
                        },
                    });
                }
            }
        }

        if !from_ranges.is_empty() {
            incoming.push(CallHierarchyIncomingCall {
                from: CallHierarchyItem {
                    name: func.name.clone(),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    detail: None,
                    uri: uri.clone(),
                    range: Range {
                        start: Position {
                            line: func.line,
                            character: 0,
                        },
                        end: Position {
                            line: func.body_end_line,
                            character: lines
                                .get(func.body_end_line as usize)
                                .map_or(0, |l| l.chars().count() as u32),
                        },
                    },
                    selection_range: Range {
                        start: Position {
                            line: func.line,
                            character: func.start_char,
                        },
                        end: Position {
                            line: func.line,
                            character: func.end_char,
                        },
                    },
                    data: None,
                },
                from_ranges,
            });
        }
    }

    incoming
}

/// Find outgoing calls (functions called by the given function)
pub fn outgoing_calls(
    uri: &Url,
    content: &str,
    item: &CallHierarchyItem,
) -> Vec<CallHierarchyOutgoingCall> {
    let functions = find_all_functions(content);
    let lines: Vec<&str> = content.lines().collect();

    // Find the source function
    let source_func = match functions.iter().find(|f| f.name == item.name) {
        Some(f) => f,
        None => return vec![],
    };

    let mut outgoing: Vec<CallHierarchyOutgoingCall> = Vec::new();

    // For each function, check if it's called within source_func's body
    for target in &functions {
        // Skip self-recursion detection for now (could be added)
        if target.name == source_func.name {
            continue;
        }

        let call_pattern = format!(r"\b{}\s*\(", regex::escape(&target.name));
        let call_re = match get_regex(&call_pattern) {
            Some(r) => r,
            None => continue,
        };

        let mut from_ranges = Vec::new();

        // Search within source function's body
        for line_idx in source_func.body_start_line..=source_func.body_end_line {
            if let Some(line) = lines.get(line_idx as usize) {
                for m in call_re.find_iter(line) {
                    if utils::is_in_string_or_comment(line, m.start()) {
                        continue;
                    }

                    let char_start = utils::byte_offset_to_char_col(line, m.start());
                    let char_end = utils::byte_offset_to_char_col(line, m.end() - 1);

                    from_ranges.push(Range {
                        start: Position {
                            line: line_idx,
                            character: char_start as u32,
                        },
                        end: Position {
                            line: line_idx,
                            character: char_end as u32,
                        },
                    });
                }
            }
        }

        if !from_ranges.is_empty() {
            outgoing.push(CallHierarchyOutgoingCall {
                to: CallHierarchyItem {
                    name: target.name.clone(),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    detail: None,
                    uri: uri.clone(),
                    range: Range {
                        start: Position {
                            line: target.line,
                            character: 0,
                        },
                        end: Position {
                            line: target.body_end_line,
                            character: lines
                                .get(target.body_end_line as usize)
                                .map_or(0, |l| l.chars().count() as u32),
                        },
                    },
                    selection_range: Range {
                        start: Position {
                            line: target.line,
                            character: target.start_char,
                        },
                        end: Position {
                            line: target.line,
                            character: target.end_char,
                        },
                    },
                    data: None,
                },
                from_ranges,
            });
        }
    }

    outgoing
}

/// Find all function definitions in the content
fn find_all_functions(content: &str) -> Vec<FunctionInfo> {
    let mut functions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Match function declarations: fn name(
    let fn_re = match get_regex(r"fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*[<(]") {
        Some(r) => r,
        None => return functions,
    };

    for (line_idx, line) in lines.iter().enumerate() {
        // Skip if in string or comment
        if let Some(caps) = fn_re.captures(line) {
            let name_match = caps.get(1).unwrap();

            // Check if the fn keyword is in a string/comment
            if let Some(fn_match) = caps.get(0) {
                if utils::is_in_string_or_comment(line, fn_match.start()) {
                    continue;
                }
            }

            let name = name_match.as_str().to_string();

            // Find the function body bounds by tracking braces
            let body_bounds = find_function_body_bounds(&lines, line_idx);

            // Convert byte positions to character positions
            let start_char = utils::byte_offset_to_char_col(line, name_match.start()) as u32;
            let end_char = utils::byte_offset_to_char_col(line, name_match.end()) as u32;

            functions.push(FunctionInfo {
                name,
                line: line_idx as u32,
                start_char,
                end_char,
                body_start_line: body_bounds.0,
                body_end_line: body_bounds.1,
            });
        }
    }

    functions
}

/// Find the start and end lines of a function body
fn find_function_body_bounds(lines: &[&str], fn_line: usize) -> (u32, u32) {
    let mut brace_depth: i32 = 0;
    let mut found_open = false;
    let mut body_start = fn_line as u32;
    let mut body_end = fn_line as u32;
    let mut in_string = false;
    let mut prev_char = '\0';

    for (idx, line) in lines.iter().enumerate().skip(fn_line) {
        for ch in line.chars() {
            // Track string context
            if ch == '"' && prev_char != '\\' {
                in_string = !in_string;
            }

            if !in_string {
                match ch {
                    '{' => {
                        if !found_open {
                            found_open = true;
                            body_start = idx as u32;
                        }
                        brace_depth += 1;
                    }
                    '}' => {
                        brace_depth -= 1;
                        if found_open && brace_depth == 0 {
                            body_end = idx as u32;
                            return (body_start, body_end);
                        }
                    }
                    _ => {}
                }
            }
            prev_char = ch;
        }
    }

    (body_start, body_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_all_functions() {
        let content = r#"
fn add(a: int, b: int) -> int {
    a + b
}

fn multiply(x: int, y: int) -> int {
    x * y
}

fn main() {
    let sum = add(1, 2)
    let product = multiply(3, 4)
}
"#;
        let functions = find_all_functions(content);
        assert_eq!(functions.len(), 3);
        assert_eq!(functions[0].name, "add");
        assert_eq!(functions[1].name, "multiply");
        assert_eq!(functions[2].name, "main");
    }

    #[test]
    fn test_prepare_call_hierarchy() {
        let content = r#"
fn helper() {
    println("helping")
}

fn main() {
    helper()
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        // Position on "helper" in the function definition
        let result = prepare_call_hierarchy(
            &uri,
            content,
            Position {
                line: 1,
                character: 4,
            },
        );
        assert!(result.is_some());
        let items = result.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "helper");
    }

    #[test]
    fn test_incoming_calls() {
        let content = r#"
fn target() {
    println("target")
}

fn caller1() {
    target()
}

fn caller2() {
    target()
    target()
}

fn not_a_caller() {
    println("nope")
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let item = CallHierarchyItem {
            name: "target".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: uri.clone(),
            range: Range::default(),
            selection_range: Range::default(),
            data: None,
        };

        let incoming = incoming_calls(&uri, content, &item);
        assert_eq!(incoming.len(), 2);

        let names: Vec<&str> = incoming.iter().map(|c| c.from.name.as_str()).collect();
        assert!(names.contains(&"caller1"));
        assert!(names.contains(&"caller2"));
    }

    #[test]
    fn test_outgoing_calls() {
        let content = r#"
fn helper1() {
    println("h1")
}

fn helper2() {
    println("h2")
}

fn main() {
    helper1()
    helper2()
    helper1()
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let item = CallHierarchyItem {
            name: "main".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: uri.clone(),
            range: Range::default(),
            selection_range: Range::default(),
            data: None,
        };

        let outgoing = outgoing_calls(&uri, content, &item);
        assert_eq!(outgoing.len(), 2);

        let names: Vec<&str> = outgoing.iter().map(|c| c.to.name.as_str()).collect();
        assert!(names.contains(&"helper1"));
        assert!(names.contains(&"helper2"));
    }

    #[test]
    fn test_skip_calls_in_strings() {
        let content = r#"
fn target() {
    println("target")
}

fn main() {
    println("target()")
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let item = CallHierarchyItem {
            name: "target".to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: uri.clone(),
            range: Range::default(),
            selection_range: Range::default(),
            data: None,
        };

        let incoming = incoming_calls(&uri, content, &item);
        // main doesn't actually call target, just has it in a string
        assert_eq!(incoming.len(), 0);
    }

    #[test]
    fn test_skip_fn_in_string() {
        let content = r#"
fn main() {
    println("fn fake() { }")
}

fn real() {
    println("real")
}
"#;
        let functions = find_all_functions(content);
        assert_eq!(functions.len(), 2);
        assert!(functions.iter().any(|f| f.name == "main"));
        assert!(functions.iter().any(|f| f.name == "real"));
        assert!(!functions.iter().any(|f| f.name == "fake"));
    }

    #[test]
    fn test_braces_in_string() {
        let content = r#"
fn test() {
    println("{ } { }")
    let x = 5
}
"#;
        let functions = find_all_functions(content);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].body_end_line, 4); // Should correctly find the closing brace
    }
}
