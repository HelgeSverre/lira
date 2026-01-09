//! Signature help support for Lira
//!
//! Provides parameter hints while typing function calls.

use regex::Regex;
use tower_lsp::lsp_types::*;

/// Get signature help at a position (inside a function call)
pub fn get_signature_help(content: &str, position: Position) -> Option<SignatureHelp> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Find the function call context
    let (func_name, active_param) = find_call_context(line, col)?;

    // Find the function definition
    let signature = find_function_signature(content, &func_name)?;

    Some(SignatureHelp {
        signatures: vec![signature],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}

/// Find the function being called and which parameter we're in
fn find_call_context(line: &str, col: usize) -> Option<(String, u32)> {
    if col > line.len() {
        return None;
    }

    let before_cursor = &line[..col];

    // Find the opening paren by scanning backwards
    let mut paren_depth = 0;
    let mut paren_pos = None;
    let chars: Vec<char> = before_cursor.chars().collect();

    for i in (0..chars.len()).rev() {
        match chars[i] {
            ')' => paren_depth += 1,
            '(' => {
                if paren_depth == 0 {
                    paren_pos = Some(i);
                    break;
                }
                paren_depth -= 1;
            }
            _ => {}
        }
    }

    let paren_pos = paren_pos?;

    // Extract function name before the paren
    let before_paren = &before_cursor[..paren_pos];
    let func_name = extract_identifier_before(before_paren)?;

    // Count commas to determine active parameter
    let inside_call = &before_cursor[paren_pos + 1..];
    let active_param = count_parameters(inside_call);

    Some((func_name, active_param))
}

/// Extract the identifier immediately before a position
fn extract_identifier_before(s: &str) -> Option<String> {
    let s = s.trim_end();
    if s.is_empty() {
        return None;
    }

    let chars: Vec<char> = s.chars().collect();
    let end = chars.len();
    let mut start = end;

    // Walk backwards to find identifier start
    while start > 0 {
        let c = chars[start - 1];
        if c.is_alphanumeric() || c == '_' {
            start -= 1;
        } else {
            break;
        }
    }

    if start == end {
        return None;
    }

    Some(chars[start..end].iter().collect())
}

/// Count the active parameter based on comma positions
fn count_parameters(s: &str) -> u32 {
    let mut count: u32 = 0;
    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut brace_depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for c in s.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        if c == '\\' {
            escape_next = true;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            continue;
        }

        if in_string {
            continue;
        }

        match c {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                count += 1;
            }
            _ => {}
        }
    }

    count
}

/// Find a function definition in the content and extract its signature
fn find_function_signature(content: &str, func_name: &str) -> Option<SignatureInformation> {
    // First check built-in functions
    if let Some(sig) = builtin_signature(func_name) {
        return Some(sig);
    }

    // Search for user-defined function
    let pattern = format!(
        r"fn\s+{}\s*(?:<[^>]*>)?\s*\(([^)]*)\)\s*(?:->\s*([^\s{{]+))?",
        regex::escape(func_name)
    );

    let re = Regex::new(&pattern).ok()?;

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let params_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let return_type = caps.get(2).map(|m| m.as_str());

            let parameters = parse_parameters(params_str);
            let label = format_signature(func_name, params_str, return_type);

            return Some(SignatureInformation {
                label,
                documentation: None,
                parameters: Some(parameters),
                active_parameter: None,
            });
        }
    }

    None
}

/// Parse parameter string into ParameterInformation list
fn parse_parameters(params_str: &str) -> Vec<ParameterInformation> {
    if params_str.trim().is_empty() {
        return Vec::new();
    }

    params_str
        .split(',')
        .map(|p| {
            let p = p.trim();
            ParameterInformation {
                label: ParameterLabel::Simple(p.to_string()),
                documentation: None,
            }
        })
        .collect()
}

/// Format a full function signature
fn format_signature(name: &str, params: &str, return_type: Option<&str>) -> String {
    match return_type {
        Some(ret) => format!("fn {}({}) -> {}", name, params, ret),
        None => format!("fn {}({})", name, params),
    }
}

/// Get signature for built-in functions
fn builtin_signature(name: &str) -> Option<SignatureInformation> {
    let (label, params) = match name {
        "print" => (
            "print(value: any) -> void",
            vec![param("value: any", "The value to print")],
        ),
        "println" => (
            "println(value: any) -> void",
            vec![param("value: any", "The value to print with newline")],
        ),
        "debug" => (
            "debug(value: any) -> void",
            vec![param("value: any", "The value to debug print")],
        ),
        "assert" => (
            "assert(condition: bool) -> void",
            vec![param("condition: bool", "The condition to assert")],
        ),
        "len" => (
            "len(collection: [T]) -> int",
            vec![param("collection: [T]", "Array or string to measure")],
        ),
        "push" => (
            "push(array: [T], value: T) -> void",
            vec![
                param("array: [T]", "The array to append to"),
                param("value: T", "The value to append"),
            ],
        ),
        "pop" => (
            "pop(array: [T]) -> T?",
            vec![param("array: [T]", "The array to pop from")],
        ),
        "append" => (
            "append(a: [T], b: [T]) -> [T]",
            vec![
                param("a: [T]", "First array"),
                param("b: [T]", "Second array to append"),
            ],
        ),
        "panic" => (
            "panic(message: string) -> never",
            vec![param("message: string", "Error message")],
        ),
        "todo" => (
            "todo(message: string) -> never",
            vec![param("message: string", "Description of unimplemented code")],
        ),
        "unreachable" => ("unreachable() -> never", vec![]),
        "type_of" => (
            "type_of(value: any) -> string",
            vec![param("value: any", "Value to get type of")],
        ),
        "to_string" => (
            "to_string(value: any) -> string",
            vec![param("value: any", "Value to convert to string")],
        ),
        "parse_int" => (
            "parse_int(s: string) -> int?",
            vec![param("s: string", "String to parse as integer")],
        ),
        "parse_float" => (
            "parse_float(s: string) -> float?",
            vec![param("s: string", "String to parse as float")],
        ),
        _ => return None,
    };

    Some(SignatureInformation {
        label: label.to_string(),
        documentation: None,
        parameters: Some(params),
        active_parameter: None,
    })
}

fn param(label: &str, doc: &str) -> ParameterInformation {
    ParameterInformation {
        label: ParameterLabel::Simple(label.to_string()),
        documentation: Some(Documentation::String(doc.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_call_context() {
        let line = "    foo(1, 2, ";
        let col = 14;
        let result = find_call_context(line, col);
        assert!(result.is_some());
        let (name, param) = result.unwrap();
        assert_eq!(name, "foo");
        assert_eq!(param, 2); // After two commas
    }

    #[test]
    fn test_find_call_context_first_param() {
        let line = "println(";
        let col = 8;
        let result = find_call_context(line, col);
        assert!(result.is_some());
        let (name, param) = result.unwrap();
        assert_eq!(name, "println");
        assert_eq!(param, 0);
    }

    #[test]
    fn test_extract_identifier_before() {
        assert_eq!(
            extract_identifier_before("  foo"),
            Some("foo".to_string())
        );
        assert_eq!(
            extract_identifier_before("bar.baz"),
            Some("baz".to_string())
        );
        assert_eq!(
            extract_identifier_before("test_func"),
            Some("test_func".to_string())
        );
    }

    #[test]
    fn test_count_parameters() {
        assert_eq!(count_parameters(""), 0);
        assert_eq!(count_parameters("1"), 0);
        assert_eq!(count_parameters("1, 2"), 1);
        assert_eq!(count_parameters("1, 2, 3"), 2);
        assert_eq!(count_parameters("foo(1, 2), 3"), 1); // Nested call
    }

    #[test]
    fn test_builtin_signature() {
        let sig = builtin_signature("println");
        assert!(sig.is_some());
        let sig = sig.unwrap();
        assert_eq!(sig.label, "println(value: any) -> void");
        assert_eq!(sig.parameters.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_find_function_signature() {
        let content = r#"
fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    add(1, 2)
}
"#;
        let sig = find_function_signature(content, "add");
        assert!(sig.is_some());
        let sig = sig.unwrap();
        assert_eq!(sig.label, "fn add(a: int, b: int) -> int");
        assert_eq!(sig.parameters.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_signature_help() {
        let content = r#"
fn greet(name: string, times: int) -> void {
    println(name)
}

fn main() {
    greet("hello",
}
"#;
        let pos = Position {
            line: 6,
            character: 18,
        };
        let help = get_signature_help(content, pos);
        assert!(help.is_some());
        let help = help.unwrap();
        assert_eq!(help.signatures.len(), 1);
        assert_eq!(help.active_parameter, Some(1)); // Second parameter
    }
}
