//! Completion support for Lira
//!
//! Provides keyword, built-in function, snippet, and user-defined symbol completions.

use regex::Regex;
use tower_lsp::lsp_types::*;

/// Get completions at a position in the document
pub fn get_completions(content: &str, position: Position) -> Vec<CompletionItem> {
    let mut completions = Vec::new();

    // Get the line and determine context
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        // Return all keywords for empty/new lines
        completions.extend(keyword_completions());
        completions.extend(builtin_completions());
        return completions;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word being typed
    let prefix = get_word_prefix(line, col);

    // Add keyword completions
    completions.extend(
        keyword_completions()
            .into_iter()
            .filter(|c| matches_prefix(&c.label, prefix)),
    );

    // Add built-in function completions
    completions.extend(
        builtin_completions()
            .into_iter()
            .filter(|c| matches_prefix(&c.label, prefix)),
    );

    // Add type completions
    completions.extend(
        type_completions()
            .into_iter()
            .filter(|c| matches_prefix(&c.label, prefix)),
    );

    // Add snippet completions
    completions.extend(
        snippet_completions()
            .into_iter()
            .filter(|c| matches_prefix(&c.label, prefix)),
    );

    // Add user-defined symbol completions
    completions.extend(
        user_symbol_completions(content)
            .into_iter()
            .filter(|c| matches_prefix(&c.label, prefix)),
    );

    completions
}

fn get_word_prefix(line: &str, col: usize) -> &str {
    if col > line.len() {
        return "";
    }

    let before_cursor = &line[..col];
    let start = before_cursor
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);

    &before_cursor[start..]
}

fn matches_prefix(label: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    label.to_lowercase().starts_with(&prefix.to_lowercase())
}

fn keyword_completions() -> Vec<CompletionItem> {
    let keywords = [
        // Declaration keywords
        (
            "fn",
            "Function declaration",
            "fn ${1:name}(${2:params}) ${3:-> Type }{\n\t$0\n}",
        ),
        ("let", "Immutable variable", "let ${1:name} = $0"),
        ("var", "Mutable variable", "var ${1:name} = $0"),
        ("const", "Constant", "const ${1:NAME} = $0"),
        (
            "struct",
            "Struct declaration",
            "struct ${1:Name} {\n\t${2:field}: ${3:Type},\n}",
        ),
        ("class", "Class declaration", "class ${1:Name} {\n\t$0\n}"),
        (
            "enum",
            "Enum declaration",
            "enum ${1:Name} {\n\t${2:Variant},\n}",
        ),
        ("trait", "Trait declaration", "trait ${1:Name} {\n\t$0\n}"),
        ("impl", "Implementation block", "impl ${1:Type} {\n\t$0\n}"),
        ("type", "Type alias", "type ${1:Name} = $0"),
        // Control flow
        ("if", "If expression", "if ${1:condition} {\n\t$0\n}"),
        ("else", "Else branch", "else {\n\t$0\n}"),
        (
            "match",
            "Match expression",
            "match ${1:value} {\n\t${2:pattern} => $0,\n}",
        ),
        ("while", "While loop", "while ${1:condition} {\n\t$0\n}"),
        (
            "for",
            "For loop",
            "for ${1:item} in ${2:iterable} {\n\t$0\n}",
        ),
        ("loop", "Infinite loop", "loop {\n\t$0\n}"),
        ("break", "Break from loop", "break"),
        ("continue", "Continue loop", "continue"),
        ("return", "Return from function", "return $0"),
        // Concurrency
        ("spawn", "Spawn a fiber", "spawn {\n\t$0\n}"),
        ("select", "Select on channels", "select {\n\t$0\n}"),
        ("async", "Async function", "async"),
        // Error handling
        ("try", "Try block", "try {\n\t$0\n}"),
        (
            "catch",
            "Catch block",
            "catch ${1:Error} as ${2:e} {\n\t$0\n}",
        ),
        // Visibility
        ("pub", "Public visibility", "pub"),
        ("priv", "Private visibility", "priv"),
        // Other
        ("import", "Import module", "import ${1:path}"),
        ("use", "Use declaration", "use ${1:path}"),
        ("as", "Type cast / alias", "as"),
        ("is", "Type check", "is"),
        ("in", "In keyword", "in"),
    ];

    keywords
        .into_iter()
        .map(|(label, detail, snippet)| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

fn builtin_completions() -> Vec<CompletionItem> {
    let builtins = [
        (
            "print",
            "Print to stdout",
            "print(${1:value})",
            "(value: any) -> void",
        ),
        (
            "println",
            "Print line to stdout",
            "println(${1:value})",
            "(value: any) -> void",
        ),
        (
            "debug",
            "Debug print",
            "debug(${1:value})",
            "(value: any) -> void",
        ),
        (
            "assert",
            "Assert condition",
            "assert(${1:condition})",
            "(condition: bool) -> void",
        ),
        (
            "len",
            "Get length",
            "len(${1:collection})",
            "(collection: [T]) -> int",
        ),
        (
            "push",
            "Push to array",
            "push(${1:array}, ${2:value})",
            "(array: [T], value: T) -> void",
        ),
        (
            "pop",
            "Pop from array",
            "pop(${1:array})",
            "(array: [T]) -> T?",
        ),
        (
            "append",
            "Append arrays",
            "append(${1:array}, ${2:other})",
            "(a: [T], b: [T]) -> [T]",
        ),
        (
            "panic",
            "Panic with message",
            "panic(${1:message})",
            "(message: string) -> never",
        ),
        (
            "todo",
            "Mark as todo",
            "todo(${1:message})",
            "(message: string) -> never",
        ),
        (
            "unreachable",
            "Mark as unreachable",
            "unreachable()",
            "() -> never",
        ),
    ];

    builtins
        .into_iter()
        .map(|(label, detail, snippet, signature)| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(format!("{}: {}", detail, signature)),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

fn type_completions() -> Vec<CompletionItem> {
    let types = [
        ("int", "64-bit signed integer"),
        ("int8", "8-bit signed integer"),
        ("int16", "16-bit signed integer"),
        ("int32", "32-bit signed integer"),
        ("int64", "64-bit signed integer"),
        ("uint8", "8-bit unsigned integer"),
        ("uint16", "16-bit unsigned integer"),
        ("uint32", "32-bit unsigned integer"),
        ("uint64", "64-bit unsigned integer"),
        ("float", "64-bit floating point"),
        ("bool", "Boolean"),
        ("string", "String"),
        ("char", "Character"),
        ("void", "Void type"),
        // Built-in generic types
        ("List", "Dynamic array"),
        ("Map", "Hash map"),
        ("Set", "Hash set"),
        ("Option", "Optional value"),
        ("Result", "Result type"),
        ("Channel", "Channel for concurrency"),
    ];

    types
        .into_iter()
        .map(|(label, detail)| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect()
}

fn snippet_completions() -> Vec<CompletionItem> {
    let snippets = [
        (
            "main",
            "Main function",
            "fn main() {\n\t$0\n}",
            "Entry point",
        ),
        (
            "test",
            "Test function",
            "fn test_${1:name}() {\n\t$0\n}",
            "Test case",
        ),
        (
            "ifel",
            "If-else",
            "if ${1:condition} {\n\t$2\n} else {\n\t$0\n}",
            "If-else expression",
        ),
        (
            "match_opt",
            "Match Option",
            "match ${1:value} {\n\tSome(${2:v}) => $3,\n\tNone => $0,\n}",
            "Match on Option",
        ),
        (
            "match_res",
            "Match Result",
            "match ${1:value} {\n\tOk(${2:v}) => $3,\n\tErr(${4:e}) => $0,\n}",
            "Match on Result",
        ),
        (
            "for_range",
            "For range loop",
            "for ${1:i} in ${2:0}..${3:n} {\n\t$0\n}",
            "For loop over range",
        ),
        (
            "impl_trait",
            "Impl trait for type",
            "impl ${1:Trait} for ${2:Type} {\n\t$0\n}",
            "Implement trait",
        ),
        (
            "channel",
            "Create channel",
            "let (${1:tx}, ${2:rx}) = Channel::new()",
            "Channel creation",
        ),
    ];

    snippets
        .into_iter()
        .map(|(label, detail, snippet, documentation)| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(detail.to_string()),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            documentation: Some(Documentation::String(documentation.to_string())),
            ..Default::default()
        })
        .collect()
}

/// Extract user-defined symbols from the document
fn user_symbol_completions(content: &str) -> Vec<CompletionItem> {
    let mut completions = Vec::new();

    // Function pattern: fn name( or fn name<
    let fn_re = Regex::new(r"fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*[<(]").unwrap();
    for caps in fn_re.captures_iter(content) {
        if let Some(name) = caps.get(1) {
            let name = name.as_str();
            // Extract the full signature for detail
            let detail = extract_function_detail(content, name);
            completions.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail,
                insert_text: Some(format!("{}($0)", name)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
        }
    }

    // Struct pattern: struct Name
    let struct_re = Regex::new(r"struct\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]").unwrap();
    for caps in struct_re.captures_iter(content) {
        if let Some(name) = caps.get(1) {
            completions.push(CompletionItem {
                label: name.as_str().to_string(),
                kind: Some(CompletionItemKind::STRUCT),
                detail: Some("struct".to_string()),
                ..Default::default()
            });
        }
    }

    // Class pattern: class Name
    let class_re = Regex::new(r"class\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]").unwrap();
    for caps in class_re.captures_iter(content) {
        if let Some(name) = caps.get(1) {
            completions.push(CompletionItem {
                label: name.as_str().to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("class".to_string()),
                ..Default::default()
            });
        }
    }

    // Enum pattern: enum Name
    let enum_re = Regex::new(r"enum\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]").unwrap();
    for caps in enum_re.captures_iter(content) {
        if let Some(name) = caps.get(1) {
            completions.push(CompletionItem {
                label: name.as_str().to_string(),
                kind: Some(CompletionItemKind::ENUM),
                detail: Some("enum".to_string()),
                ..Default::default()
            });
        }
    }

    // Trait pattern: trait Name
    let trait_re = Regex::new(r"trait\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]").unwrap();
    for caps in trait_re.captures_iter(content) {
        if let Some(name) = caps.get(1) {
            completions.push(CompletionItem {
                label: name.as_str().to_string(),
                kind: Some(CompletionItemKind::INTERFACE),
                detail: Some("trait".to_string()),
                ..Default::default()
            });
        }
    }

    // Constant pattern: const NAME
    let const_re = Regex::new(r"const\s+([A-Z_][A-Z0-9_]*)\s*[=:]").unwrap();
    for caps in const_re.captures_iter(content) {
        if let Some(name) = caps.get(1) {
            completions.push(CompletionItem {
                label: name.as_str().to_string(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: Some("const".to_string()),
                ..Default::default()
            });
        }
    }

    // Variable pattern: let/var name (top-level only, simple heuristic)
    let var_re = Regex::new(r"^(?:let|var)\s+([a-z_][a-zA-Z0-9_]*)\s*[=:]").unwrap();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(caps) = var_re.captures(trimmed) {
            if let Some(name) = caps.get(1) {
                completions.push(CompletionItem {
                    label: name.as_str().to_string(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("variable".to_string()),
                    ..Default::default()
                });
            }
        }
    }

    // Deduplicate by label (in case of duplicates)
    let mut seen = std::collections::HashSet::new();
    completions.retain(|c| seen.insert(c.label.clone()));

    completions
}

/// Extract function signature detail
fn extract_function_detail(content: &str, func_name: &str) -> Option<String> {
    let pattern = format!(
        r"fn\s+{}\s*(?:<[^>]*>)?\s*\(([^)]*)\)\s*(?:->\s*([^\s{{]+))?",
        regex::escape(func_name)
    );
    let re = Regex::new(&pattern).ok()?;

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let params = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let ret = caps.get(2).map(|m| m.as_str());
            return Some(match ret {
                Some(r) => format!("fn({}) -> {}", params, r),
                None => format!("fn({})", params),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_word_prefix() {
        assert_eq!(get_word_prefix("let foo = ", 7), "foo"); // cursor at end of "foo"
        assert_eq!(get_word_prefix("fn test", 5), "te"); // cursor after "te"
        assert_eq!(get_word_prefix("", 0), "");
        assert_eq!(get_word_prefix("let x", 3), "let"); // cursor after "let"
        assert_eq!(get_word_prefix("  fn", 4), "fn"); // cursor after "fn"
    }

    #[test]
    fn test_matches_prefix() {
        assert!(matches_prefix("fn", "f"));
        assert!(matches_prefix("function", "Fun"));
        assert!(!matches_prefix("fn", "x"));
    }

    #[test]
    fn test_completions_include_keywords() {
        let content = "f";
        let pos = Position {
            line: 0,
            character: 1,
        };
        let completions = get_completions(content, pos);

        let fn_completion = completions.iter().find(|c| c.label == "fn");
        assert!(fn_completion.is_some());
    }
}
