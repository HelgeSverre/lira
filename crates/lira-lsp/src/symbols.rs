//! Document symbols support for Lira
//!
//! Provides outline view with all symbols in a document.

use regex::Regex;
use tower_lsp::lsp_types::*;

/// Get all symbols in a document
pub fn get_document_symbols(content: &str) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    // Find all top-level declarations
    for (line_idx, line) in content.lines().enumerate() {
        if let Some(symbol) = parse_line_for_symbol(line, line_idx as u32) {
            symbols.push(symbol);
        }
    }

    symbols
}

fn parse_line_for_symbol(line: &str, line_num: u32) -> Option<DocumentSymbol> {
    let trimmed = line.trim_start();

    // Skip empty lines and comments
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
        return None;
    }

    // Function declaration
    if let Some(symbol) = parse_function(trimmed, line_num, line) {
        return Some(symbol);
    }

    // Struct declaration
    if let Some(symbol) = parse_struct(trimmed, line_num, line) {
        return Some(symbol);
    }

    // Class declaration
    if let Some(symbol) = parse_class(trimmed, line_num, line) {
        return Some(symbol);
    }

    // Enum declaration
    if let Some(symbol) = parse_enum(trimmed, line_num, line) {
        return Some(symbol);
    }

    // Trait declaration
    if let Some(symbol) = parse_trait(trimmed, line_num, line) {
        return Some(symbol);
    }

    // Type alias
    if let Some(symbol) = parse_type_alias(trimmed, line_num, line) {
        return Some(symbol);
    }

    // Impl block
    if let Some(symbol) = parse_impl(trimmed, line_num, line) {
        return Some(symbol);
    }

    // Top-level constants
    if let Some(symbol) = parse_const(trimmed, line_num, line) {
        return Some(symbol);
    }

    None
}

fn parse_function(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    let re = Regex::new(r"^(?:pub\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*[<(]").ok()?;
    let caps = re.captures(trimmed)?;
    let name = caps.get(1)?.as_str();

    let start_col = line.find("fn ").unwrap_or(0) as u32;
    let name_start = line.find(name).unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::FUNCTION,
        detail: Some(extract_function_signature(trimmed)),
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: name_start,
            },
            end: Position {
                line: line_num,
                character: name_start + name.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn parse_struct(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    let re = Regex::new(r"^(?:pub\s+)?struct\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]?").ok()?;
    let caps = re.captures(trimmed)?;
    let name = caps.get(1)?.as_str();

    let start_col = line.find("struct ").unwrap_or(0) as u32;
    let name_start = line.find(name).unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::STRUCT,
        detail: None,
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: name_start,
            },
            end: Position {
                line: line_num,
                character: name_start + name.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn parse_class(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    let re = Regex::new(r"^(?:pub\s+)?class\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]?").ok()?;
    let caps = re.captures(trimmed)?;
    let name = caps.get(1)?.as_str();

    let start_col = line.find("class ").unwrap_or(0) as u32;
    let name_start = line.find(name).unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::CLASS,
        detail: None,
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: name_start,
            },
            end: Position {
                line: line_num,
                character: name_start + name.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn parse_enum(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    let re = Regex::new(r"^(?:pub\s+)?enum\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]?").ok()?;
    let caps = re.captures(trimmed)?;
    let name = caps.get(1)?.as_str();

    let start_col = line.find("enum ").unwrap_or(0) as u32;
    let name_start = line.find(name).unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::ENUM,
        detail: None,
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: name_start,
            },
            end: Position {
                line: line_num,
                character: name_start + name.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn parse_trait(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    let re = Regex::new(r"^(?:pub\s+)?trait\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]?").ok()?;
    let caps = re.captures(trimmed)?;
    let name = caps.get(1)?.as_str();

    let start_col = line.find("trait ").unwrap_or(0) as u32;
    let name_start = line.find(name).unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::INTERFACE,
        detail: None,
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: name_start,
            },
            end: Position {
                line: line_num,
                character: name_start + name.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn parse_type_alias(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    let re = Regex::new(r"^(?:pub\s+)?type\s+([A-Z][a-zA-Z0-9_]*)\s*[<=]").ok()?;
    let caps = re.captures(trimmed)?;
    let name = caps.get(1)?.as_str();

    let start_col = line.find("type ").unwrap_or(0) as u32;
    let name_start = line.find(name).unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::TYPE_PARAMETER,
        detail: None,
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: name_start,
            },
            end: Position {
                line: line_num,
                character: name_start + name.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn parse_impl(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    // impl Trait for Type or impl Type
    let re = Regex::new(r"^impl\s+(?:([A-Z][a-zA-Z0-9_]*)\s+for\s+)?([A-Z][a-zA-Z0-9_]*)\s*[<{]?")
        .ok()?;
    let caps = re.captures(trimmed)?;

    let (name, detail) = if let Some(trait_name) = caps.get(1) {
        let type_name = caps.get(2)?.as_str();
        (
            format!("{} for {}", trait_name.as_str(), type_name),
            Some(format!("impl {} for {}", trait_name.as_str(), type_name)),
        )
    } else {
        let type_name = caps.get(2)?.as_str();
        (type_name.to_string(), Some(format!("impl {}", type_name)))
    };

    let start_col = line.find("impl ").unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        kind: SymbolKind::NAMESPACE,
        detail,
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn parse_const(trimmed: &str, line_num: u32, line: &str) -> Option<DocumentSymbol> {
    let re = Regex::new(r"^(?:pub\s+)?const\s+([A-Z][A-Z0-9_]*)\s*[=:]").ok()?;
    let caps = re.captures(trimmed)?;
    let name = caps.get(1)?.as_str();

    let start_col = line.find("const ").unwrap_or(0) as u32;
    let name_start = line.find(name).unwrap_or(0) as u32;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::CONSTANT,
        detail: None,
        range: Range {
            start: Position {
                line: line_num,
                character: start_col,
            },
            end: Position {
                line: line_num,
                character: line.len() as u32,
            },
        },
        selection_range: Range {
            start: Position {
                line: line_num,
                character: name_start,
            },
            end: Position {
                line: line_num,
                character: name_start + name.len() as u32,
            },
        },
        children: None,
        deprecated: None,
        tags: None,
    })
}

fn extract_function_signature(line: &str) -> String {
    // Extract from "fn" to end of params/return type
    if let Some(fn_idx) = line.find("fn ") {
        let rest = &line[fn_idx..];
        // Find the opening brace or end of line
        if let Some(brace_idx) = rest.find('{') {
            return rest[..brace_idx].trim().to_string();
        }
        return rest.trim_end().to_string();
    }
    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_function() {
        let content = r#"
fn add(a: int, b: int) -> int {
    a + b
}

pub fn main() {
}
"#;
        let symbols = get_document_symbols(content);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "add");
        assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
        assert_eq!(symbols[1].name, "main");
    }

    #[test]
    fn test_parse_struct() {
        let content = r#"
struct Point {
    x: float,
    y: float,
}
"#;
        let symbols = get_document_symbols(content);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Point");
        assert_eq!(symbols[0].kind, SymbolKind::STRUCT);
    }

    #[test]
    fn test_parse_enum() {
        let content = r#"
enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let symbols = get_document_symbols(content);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Color");
        assert_eq!(symbols[0].kind, SymbolKind::ENUM);
    }

    #[test]
    fn test_parse_impl() {
        let content = r#"
impl Point {
    fn new(x: float, y: float) -> Point {
    }
}

impl Display for Point {
    fn display(self) -> string {
    }
}
"#;
        let symbols = get_document_symbols(content);
        // Should find impl Point, fn new, impl Display for Point, fn display
        assert!(symbols
            .iter()
            .any(|s| s.name == "Point" && s.kind == SymbolKind::NAMESPACE));
        assert!(symbols.iter().any(|s| s.name == "Display for Point"));
    }
}
