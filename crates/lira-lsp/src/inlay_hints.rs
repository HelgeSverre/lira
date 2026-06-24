//! Inlay hints support for Lira
//!
//! Provides inline type annotations for variables with inferred types.

use crate::utils::get_regex;
use tower_lsp::lsp_types::*;

/// Get inlay hints for a document
pub fn get_inlay_hints(content: &str, range: Range) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let start_line = range.start.line as usize;
    let end_line = (range.end.line as usize).min(lines.len());

    // Pre-compute function return types for inference
    let function_types = extract_function_return_types(content);

    for line_idx in start_line..end_line {
        if line_idx >= lines.len() {
            break;
        }
        let line = lines[line_idx];

        // Find variable declarations without type annotations
        hints.extend(find_variable_hints(
            line,
            line_idx as u32,
            content,
            &function_types,
        ));

        // Find function parameter hints at call sites
        hints.extend(find_parameter_hints(line, line_idx as u32, content));
    }

    hints
}

/// Extract function return types from the document
fn extract_function_return_types(content: &str) -> Vec<(String, String)> {
    let mut types = Vec::new();

    // Pattern: fn name(...) -> Type
    let fn_re = match get_regex(
        r"fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*(?:<[^>]*>)?\s*\([^)]*\)\s*->\s*([a-zA-Z_][a-zA-Z0-9_<>\[\],\s]*)",
    ) {
        Some(r) => r,
        None => return types,
    };

    for line in content.lines() {
        if let Some(caps) = fn_re.captures(line) {
            if let (Some(name), Some(ret_type)) = (caps.get(1), caps.get(2)) {
                types.push((
                    name.as_str().to_string(),
                    ret_type.as_str().trim().to_string(),
                ));
            }
        }
    }

    types
}

/// Find type hints for variable declarations
fn find_variable_hints(
    line: &str,
    line_num: u32,
    content: &str,
    function_types: &[(String, String)],
) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    // Pattern: let/var name = value (without type annotation)
    let decl_re = match get_regex(r"(?:let|var)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(.+)") {
        Some(r) => r,
        None => return hints,
    };

    if let Some(caps) = decl_re.captures(line) {
        let name = caps.get(1).unwrap();
        let value = caps.get(2).unwrap().as_str().trim();

        // Check if there's already a type annotation (`: Type` before `=`)
        let before_eq = &line[..line.find('=').unwrap_or(line.len())];
        if before_eq.contains(':') {
            return hints; // Already has type annotation
        }

        // Infer type from the value
        if let Some(inferred_type) = infer_type_from_value(value, content, function_types) {
            // Convert byte position to character position for UTF-8
            let char_pos = line[..name.end()].chars().count();

            let position = Position {
                line: line_num,
                character: char_pos as u32,
            };

            hints.push(InlayHint {
                position,
                label: InlayHintLabel::String(format!(": {}", inferred_type)),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: Some(InlayHintTooltip::String(format!(
                    "Inferred type: {}",
                    inferred_type
                ))),
                padding_left: Some(false),
                padding_right: Some(true),
                data: None,
            });
        }
    }

    hints
}

/// Infer type from a value expression
fn infer_type_from_value(
    value: &str,
    _content: &str,
    function_types: &[(String, String)],
) -> Option<String> {
    let value = value.trim();

    // Remove trailing comments
    let value = if let Some(idx) = value.find("//") {
        value[..idx].trim()
    } else {
        value
    };

    // Integer literal
    if value.parse::<i64>().is_ok() {
        return Some("int".to_string());
    }

    // Float literal (contains . or e/E)
    if (value.contains('.') || value.to_lowercase().contains('e')) && value.parse::<f64>().is_ok() {
        return Some("float".to_string());
    }

    // Hex/binary/octal literals
    if value.starts_with("0x")
        || value.starts_with("0X")
        || value.starts_with("0b")
        || value.starts_with("0B")
        || value.starts_with("0o")
        || value.starts_with("0O")
    {
        return Some("int".to_string());
    }

    // String literal
    if value.starts_with('"') && value.ends_with('"') {
        return Some("string".to_string());
    }
    if value.starts_with("\"\"\"") {
        return Some("string".to_string());
    }

    // Character literal
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Some("char".to_string());
    }

    // Boolean literals
    if value == "true" || value == "false" {
        return Some("bool".to_string());
    }

    // Null
    if value == "null" {
        return None; // Can't infer type from null alone
    }

    // Array literal
    if value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1].trim();
        if inner.is_empty() {
            return None;
        }
        let first = inner.split(',').next()?.trim();
        if let Some(elem_type) = infer_type_from_value(first, _content, function_types) {
            return Some(format!("[{}]", elem_type));
        }
    }

    // Map literal
    if value.starts_with('{') && value.ends_with('}') && value.contains(':') {
        return Some("Map".to_string());
    }

    // Channel creation
    if value.contains("Channel::new") || value.contains("channel(") || value.contains("chan(") {
        return Some("Channel".to_string());
    }

    // Range expressions
    if value.contains("..=") || value.contains("..") {
        return Some("Range".to_string());
    }

    // Function call - try to infer from return type
    if let Some(func_name) = extract_function_call_name(value) {
        // Check built-in function return types
        if let Some(ret_type) = builtin_return_type(&func_name) {
            return Some(ret_type.to_string());
        }

        // Check user-defined function return types
        for (name, ret_type) in function_types {
            if name == &func_name {
                return Some(ret_type.clone());
            }
        }
    }

    // Struct construction: Name { ... }
    let struct_re = get_regex(r"^([A-Z][a-zA-Z0-9_]*)\s*\{");
    if let Some(re) = struct_re {
        if let Some(caps) = re.captures(value) {
            if let Some(name) = caps.get(1) {
                return Some(name.as_str().to_string());
            }
        }
    }

    // Method call chain - look at the base
    if value.contains('.') {
        let parts: Vec<&str> = value.split('.').collect();
        if !parts.is_empty() {
            // Check for known method return types
            if let Some(last_call) = parts.last() {
                if let Some(method_name) = extract_function_call_name(last_call) {
                    if let Some(ret_type) = builtin_method_return_type(&method_name) {
                        return Some(ret_type.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Extract function name from a function call expression
fn extract_function_call_name(expr: &str) -> Option<String> {
    let call_re = get_regex(r"^([a-zA-Z_][a-zA-Z0-9_]*)\s*\(")?;
    call_re
        .captures(expr)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Get return type for built-in functions
fn builtin_return_type(name: &str) -> Option<&'static str> {
    match name {
        "len" | "size" => Some("int"),
        "to_string" | "format" => Some("string"),
        "parse_int" => Some("int"),
        "parse_float" => Some("float"),
        "abs" | "floor" | "ceil" | "round" => Some("int"),
        "sqrt" | "sin" | "cos" | "tan" | "log" | "exp" => Some("float"),
        "min" | "max" => None, // Depends on arguments
        "random" => Some("float"),
        "now" => Some("int"),
        "read_file" | "read_to_string" => Some("string"),
        "exists" | "is_file" | "is_dir" => Some("bool"),
        "keys" | "values" => Some("[any]"),
        "clone" => None, // Same as input
        "pop" => None,   // Element type
        "recv" => None,  // Channel type
        "type_of" => Some("string"),
        "error" => Some("Error"),
        _ => None,
    }
}

/// Get return type for built-in methods
fn builtin_method_return_type(name: &str) -> Option<&'static str> {
    match name {
        "len" | "count" | "size" => Some("int"),
        "is_empty" | "contains" | "starts_with" | "ends_with" => Some("bool"),
        "to_string" | "to_lowercase" | "to_uppercase" | "trim" => Some("string"),
        "split" | "lines" => Some("[string]"),
        "chars" => Some("[char]"),
        "bytes" => Some("[int]"),
        "parse_int" => Some("int"),
        "parse_float" => Some("float"),
        "first" | "last" | "get" => None, // Element type
        "keys" => Some("[any]"),
        "values" => Some("[any]"),
        "entries" => Some("[(any, any)]"),
        _ => None,
    }
}

/// Find parameter name hints at function call sites
fn find_parameter_hints(line: &str, line_num: u32, content: &str) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    // Find function calls: name(arg1, arg2, ...)
    let call_re = match get_regex(r"([a-zA-Z_][a-zA-Z0-9_]*)\s*\(([^)]*)\)") {
        Some(r) => r,
        None => return hints,
    };

    for caps in call_re.captures_iter(line) {
        let func_name = caps.get(1).unwrap().as_str();
        let args_match = caps.get(2).unwrap();
        let args_str = args_match.as_str();

        // Skip if no arguments
        if args_str.trim().is_empty() {
            continue;
        }

        // Find function definition to get parameter names
        if let Some(params) = find_function_params(content, func_name) {
            let args: Vec<&str> = split_arguments(args_str);

            let mut arg_offset = 0;
            for (i, arg) in args.iter().enumerate() {
                if i >= params.len() {
                    break;
                }

                let param_name = &params[i];
                let arg_trimmed = arg.trim();

                // Skip if argument is already named (contains `name:`)
                if arg_trimmed.contains(':') && !arg_trimmed.starts_with('"') {
                    arg_offset += arg.len() + 1;
                    continue;
                }

                // Skip if argument is same as parameter name
                if arg_trimmed == param_name {
                    arg_offset += arg.len() + 1;
                    continue;
                }

                // Calculate position
                let position = Position {
                    line: line_num,
                    character: (args_match.start() + arg_offset) as u32,
                };

                hints.push(InlayHint {
                    position,
                    label: InlayHintLabel::String(format!("{}:", param_name)),
                    kind: Some(InlayHintKind::PARAMETER),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(false),
                    padding_right: Some(true),
                    data: None,
                });

                arg_offset += arg.len() + 1;
            }
        }
    }

    hints
}

/// Split function arguments, respecting nested parentheses and strings
fn split_arguments(args: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut start = 0;

    for (i, c) in args.char_indices() {
        match c {
            '"' if !in_string => in_string = true,
            '"' if in_string => in_string = false,
            '(' | '[' | '{' if !in_string => depth += 1,
            ')' | ']' | '}' if !in_string => depth -= 1,
            ',' if depth == 0 && !in_string => {
                result.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    if start < args.len() {
        result.push(&args[start..]);
    }

    result
}

/// Find parameter names for a function
fn find_function_params(content: &str, func_name: &str) -> Option<Vec<String>> {
    // First check built-in functions
    if let Some(params) = builtin_params(func_name) {
        return Some(params);
    }

    // Search for user-defined function
    let pattern = format!(
        r"fn\s+{}\s*(?:<[^>]*>)?\s*\(([^)]*)\)",
        regex::escape(func_name)
    );
    let re = get_regex(&pattern)?;

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let params_str = caps.get(1)?.as_str();
            let params: Vec<String> = params_str
                .split(',')
                .filter_map(|p| {
                    let p = p.trim();
                    if p.is_empty() {
                        return None;
                    }
                    let name = p.split(':').next()?.trim();
                    if name == "self" || name == "self mut" {
                        return None;
                    }
                    Some(name.to_string())
                })
                .collect();
            return Some(params);
        }
    }

    None
}

/// Get parameter names for built-in functions
fn builtin_params(name: &str) -> Option<Vec<String>> {
    let params = match name {
        "print" | "println" | "debug" => vec!["value"],
        "assert" => vec!["condition"],
        "len" => vec!["collection"],
        "push" => vec!["array", "value"],
        "pop" => vec!["array"],
        "append" => vec!["a", "b"],
        "panic" | "todo" => vec!["message"],
        "type_of" | "to_string" => vec!["value"],
        "parse_int" | "parse_float" => vec!["s"],
        "min" | "max" => vec!["a", "b"],
        "sqrt" | "abs" | "floor" | "ceil" | "round" => vec!["n"],
        "sin" | "cos" | "tan" | "log" | "exp" => vec!["x"],
        "pow" => vec!["base", "exp"],
        "open" | "read_file" | "write_file" => vec!["path"],
        "sleep" => vec!["ms"],
        "spawn" => vec!["fn"],
        "send" => vec!["channel", "value"],
        "recv" => vec!["channel"],
        _ => return None,
    };
    Some(params.into_iter().map(String::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_fn_types() -> Vec<(String, String)> {
        vec![]
    }

    #[test]
    fn test_infer_int() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("42", "", &ft),
            Some("int".to_string())
        );
        assert_eq!(
            infer_type_from_value("-100", "", &ft),
            Some("int".to_string())
        );
        assert_eq!(
            infer_type_from_value("0x1F", "", &ft),
            Some("int".to_string())
        );
        assert_eq!(
            infer_type_from_value("0b1010", "", &ft),
            Some("int".to_string())
        );
    }

    #[test]
    fn test_infer_float() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("3.14", "", &ft),
            Some("float".to_string())
        );
        assert_eq!(
            infer_type_from_value("1e10", "", &ft),
            Some("float".to_string())
        );
        assert_eq!(
            infer_type_from_value("2.5E-3", "", &ft),
            Some("float".to_string())
        );
    }

    #[test]
    fn test_infer_string() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("\"hello\"", "", &ft),
            Some("string".to_string())
        );
    }

    #[test]
    fn test_infer_bool() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("true", "", &ft),
            Some("bool".to_string())
        );
        assert_eq!(
            infer_type_from_value("false", "", &ft),
            Some("bool".to_string())
        );
    }

    #[test]
    fn test_infer_char() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("'a'", "", &ft),
            Some("char".to_string())
        );
    }

    #[test]
    fn test_infer_array() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("[1, 2, 3]", "", &ft),
            Some("[int]".to_string())
        );
        assert_eq!(
            infer_type_from_value("[\"a\", \"b\"]", "", &ft),
            Some("[string]".to_string())
        );
    }

    #[test]
    fn test_infer_function_call() {
        let ft = vec![("get_count".to_string(), "int".to_string())];
        assert_eq!(
            infer_type_from_value("get_count()", "", &ft),
            Some("int".to_string())
        );
    }

    #[test]
    fn test_infer_builtin_function() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("len(items)", "", &ft),
            Some("int".to_string())
        );
        assert_eq!(
            infer_type_from_value("to_string(x)", "", &ft),
            Some("string".to_string())
        );
    }

    #[test]
    fn test_infer_struct_construction() {
        let ft = empty_fn_types();
        assert_eq!(
            infer_type_from_value("Point { x: 1, y: 2 }", "", &ft),
            Some("Point".to_string())
        );
    }

    #[test]
    fn test_find_variable_hints() {
        let content = "    let x = 42";
        let ft = empty_fn_types();
        let hints = find_variable_hints(content, 0, content, &ft);
        assert_eq!(hints.len(), 1);
        if let InlayHintLabel::String(s) = &hints[0].label {
            assert_eq!(s, ": int");
        } else {
            panic!("Expected String label");
        }
    }

    #[test]
    fn test_no_hint_with_type_annotation() {
        let content = "    let x: int = 42";
        let ft = empty_fn_types();
        let hints = find_variable_hints(content, 0, content, &ft);
        assert!(hints.is_empty());
    }

    #[test]
    fn test_split_arguments() {
        assert_eq!(split_arguments("1, 2, 3"), vec!["1", " 2", " 3"]);
        assert_eq!(split_arguments("foo(1), 2"), vec!["foo(1)", " 2"]);
        assert_eq!(split_arguments("\"a,b\", c"), vec!["\"a,b\"", " c"]);
    }

    #[test]
    fn test_builtin_params() {
        assert_eq!(
            builtin_params("push"),
            Some(vec!["array".to_string(), "value".to_string()])
        );
        assert_eq!(builtin_params("unknown"), None);
    }

    #[test]
    fn test_extract_function_return_types() {
        let content = r#"
fn add(a: int, b: int) -> int {
    a + b
}

fn get_name() -> string {
    "hello"
}
"#;
        let types = extract_function_return_types(content);
        assert!(types.iter().any(|(n, t)| n == "add" && t == "int"));
        assert!(types.iter().any(|(n, t)| n == "get_name" && t == "string"));
    }
}
