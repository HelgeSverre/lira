//! Completion support for Lira
//!
//! Provides keyword, built-in function, snippet, and user-defined symbol
//! completions, plus type-aware member completion after `.` powered by the
//! semantic tables.

use crate::sema_index::{self, Cursor};
use lirac::checker::Type;
use lirac::sema::MemberInfo;
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

    // Member completion: if the cursor is in a `<receiver>.<prefix>` context,
    // resolve the receiver's type and offer its fields + methods.
    if let Some(member_completions) = member_completions(content, position, line, col) {
        return member_completions;
    }

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

/// A `<receiver>.<member_prefix>` member-access context split out of the text
/// before the cursor.
struct MemberContext {
    /// 0-indexed character column of the `.`.
    dot_col: usize,
    /// The partially-typed member name after the `.` (may be empty).
    prefix: String,
}

/// Detect a member-access context by scanning the text before the cursor for a
/// trailing `.<ident-chars>`. Returns `None` if there is no preceding `.`.
fn detect_member_context(line: &str, col: usize) -> Option<MemberContext> {
    let chars: Vec<char> = line.chars().collect();
    if col > chars.len() {
        return None;
    }
    // Walk back over the (possibly empty) member prefix being typed.
    let mut i = col;
    while i > 0 {
        let c = chars[i - 1];
        if c.is_alphanumeric() || c == '_' {
            i -= 1;
        } else {
            break;
        }
    }
    // The char immediately before the prefix must be a `.`.
    if i == 0 || chars[i - 1] != '.' {
        return None;
    }
    let dot_col = i - 1;
    let prefix: String = chars[i..col].iter().collect();
    Some(MemberContext { dot_col, prefix })
}

/// Build member completions for a `receiver.` context, or `None` if the cursor
/// is not in a member-access context or the receiver type can't be resolved.
fn member_completions(
    content: &str,
    position: Position,
    line: &str,
    col: usize,
) -> Option<Vec<CompletionItem>> {
    let ctx = detect_member_context(line, col)?;

    // A trailing `.` (or `.prefix`) typically fails to parse, which would
    // discard the whole AST/sema. Blank out the `.` + member prefix on the
    // cursor line so the receiver parses as a bare expression, then analyze.
    let patched = blank_member_access(content, position.line as usize, ctx.dot_col, col);
    let analysis = lirac::analyze(&patched).ok()?;

    // Resolve the receiver expression: place the cursor just before the `.` so it
    // lands on the last character of the receiver expression.
    let receiver_cursor = Cursor::from_lsp(position.line, ctx.dot_col.saturating_sub(1) as u32);
    let receiver = sema_index::expr_at(&analysis.program, &patched, receiver_cursor)?;
    let recv_ty = analysis.sema.expr_types.get(&receiver.id)?;

    let type_name = type_member_key(recv_ty)?;
    let members = analysis.sema.type_members.get(&type_name)?;

    let mut items: Vec<CompletionItem> = Vec::new();
    for field in &members.fields {
        if matches_prefix(&field.name, &ctx.prefix) {
            items.push(field_completion(field));
        }
    }
    for method in &members.methods {
        if matches_prefix(&method.name, &ctx.prefix) {
            items.push(method_completion(method));
        }
    }
    // If sema knows no matching members for this receiver (common for built-in
    // primitives whose operations are free functions rather than stored
    // methods), fall back to the keyword/builtin completion instead of
    // returning an empty member list.
    if items.is_empty() {
        return None;
    }
    Some(items)
}

/// Replace the `.` and trailing member prefix on `line_idx` (character columns
/// `[dot_col, end_col)`) with spaces, preserving every other character so source
/// positions stay stable. This turns an unparseable `receiver.` into a bare
/// `receiver` expression that the parser and checker can handle.
fn blank_member_access(content: &str, line_idx: usize, dot_col: usize, end_col: usize) -> String {
    let mut out = String::with_capacity(content.len());
    for (idx, line) in content.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if idx == line_idx {
            for (c, ch) in line.chars().enumerate() {
                if c >= dot_col && c < end_col {
                    out.push(' ');
                } else {
                    out.push(ch);
                }
            }
        } else {
            out.push_str(line);
        }
    }
    // Preserve a trailing newline if the original had one (lines() drops it).
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The `type_members` key for a resolved receiver type, if it is a named type.
fn type_member_key(ty: &Type) -> Option<String> {
    match ty {
        Type::Struct(n) | Type::Class(n) | Type::Enum(n) | Type::Interface(n) => Some(n.clone()),
        Type::String => Some("string".to_string()),
        _ => None,
    }
}

fn field_completion(field: &MemberInfo) -> CompletionItem {
    CompletionItem {
        label: field.name.clone(),
        kind: Some(CompletionItemKind::FIELD),
        detail: Some(field.ty.display_name()),
        insert_text: Some(field.name.clone()),
        ..Default::default()
    }
}

fn method_completion(method: &MemberInfo) -> CompletionItem {
    let (signature, snippet) = match &method.ty {
        Type::Function {
            params,
            return_type,
            ..
        } => {
            let params_str = params
                .iter()
                .map(|t| t.display_name())
                .collect::<Vec<_>>()
                .join(", ");
            (
                format!("fn({}) -> {}", params_str, return_type.display_name()),
                format!("{}($0)", method.name),
            )
        }
        other => (other.display_name(), format!("{}($0)", method.name)),
    };
    CompletionItem {
        label: method.name.clone(),
        kind: Some(CompletionItemKind::METHOD),
        detail: Some(signature),
        insert_text: Some(snippet),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    }
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

    #[test]
    fn test_detect_member_context() {
        let ctx = detect_member_context("p.", 2).expect("member context");
        assert_eq!(ctx.dot_col, 1);
        assert_eq!(ctx.prefix, "");

        let ctx = detect_member_context("p.ar", 4).expect("member context");
        assert_eq!(ctx.prefix, "ar");

        assert!(detect_member_context("foo", 3).is_none());
    }

    #[test]
    fn test_member_completion_field() {
        // `p.` should offer field `x` of struct P.
        let content = "struct P { x: int }\nlet p = P { x: 1 }\np.";
        let pos = Position {
            line: 2,
            character: 2,
        };
        let completions = get_completions(content, pos);
        let x = completions
            .iter()
            .find(|c| c.label == "x")
            .expect("field x present");
        assert_eq!(x.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(x.detail.as_deref(), Some("int"));
    }

    #[test]
    fn test_member_completion_method() {
        let content =
            "struct P { x: int }\nimpl P { fn area(self) -> int { self.x } }\nlet p = P { x: 1 }\np.";
        let pos = Position {
            line: 3,
            character: 2,
        };
        let completions = get_completions(content, pos);
        let area = completions
            .iter()
            .find(|c| c.label == "area")
            .expect("method area present");
        assert_eq!(area.kind, Some(CompletionItemKind::METHOD));
        assert!(area.detail.as_deref().unwrap_or("").contains("-> int"));
    }

    #[test]
    fn test_member_completion_prefix_filter() {
        // `p.a` should only offer members starting with `a`.
        let content =
            "struct P { x: int }\nimpl P { fn area(self) -> int { self.x } }\nlet p = P { x: 1 }\np.a";
        let pos = Position {
            line: 3,
            character: 3,
        };
        let completions = get_completions(content, pos);
        assert!(completions.iter().any(|c| c.label == "area"));
        assert!(
            !completions.iter().any(|c| c.label == "x"),
            "field x should be filtered out by prefix `a`"
        );
    }
}
