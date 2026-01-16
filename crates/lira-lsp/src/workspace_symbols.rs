//! Workspace symbol support for Lira
//!
//! Provides symbol search across all open documents.

use crate::utils::get_regex;
use tower_lsp::lsp_types::*;

/// Symbol information extracted from a document
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub container_name: Option<String>,
}

/// Extract all symbols from a document
pub fn extract_symbols(uri: &Url, content: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Function pattern
    if let Some(fn_re) = get_regex(r"^(\s*)(?:pub\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*[<(]") {
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(caps) = fn_re.captures(line) {
                let indent = caps.get(1).map_or(0, |m| m.as_str().len());
                if let Some(name_match) = caps.get(2) {
                    let name = name_match.as_str().to_string();
                    let container = if indent > 0 {
                        find_container(&lines, line_idx)
                    } else {
                        None
                    };

                    symbols.push(ExtractedSymbol {
                        name: name.clone(),
                        kind: SymbolKind::FUNCTION,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: line_idx as u32,
                                    character: name_match.start() as u32,
                                },
                                end: Position {
                                    line: line_idx as u32,
                                    character: name_match.end() as u32,
                                },
                            },
                        },
                        container_name: container,
                    });
                }
            }
        }
    }

    // Struct pattern
    if let Some(struct_re) = get_regex(r"^(?:pub\s+)?struct\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]") {
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(caps) = struct_re.captures(line) {
                if let Some(name_match) = caps.get(1) {
                    symbols.push(ExtractedSymbol {
                        name: name_match.as_str().to_string(),
                        kind: SymbolKind::STRUCT,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: line_idx as u32,
                                    character: name_match.start() as u32,
                                },
                                end: Position {
                                    line: line_idx as u32,
                                    character: name_match.end() as u32,
                                },
                            },
                        },
                        container_name: None,
                    });
                }
            }
        }
    }

    // Class pattern
    if let Some(class_re) = get_regex(r"^(?:pub\s+)?class\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]") {
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(caps) = class_re.captures(line) {
                if let Some(name_match) = caps.get(1) {
                    symbols.push(ExtractedSymbol {
                        name: name_match.as_str().to_string(),
                        kind: SymbolKind::CLASS,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: line_idx as u32,
                                    character: name_match.start() as u32,
                                },
                                end: Position {
                                    line: line_idx as u32,
                                    character: name_match.end() as u32,
                                },
                            },
                        },
                        container_name: None,
                    });
                }
            }
        }
    }

    // Enum pattern
    if let Some(enum_re) = get_regex(r"^(?:pub\s+)?enum\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]") {
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(caps) = enum_re.captures(line) {
                if let Some(name_match) = caps.get(1) {
                    symbols.push(ExtractedSymbol {
                        name: name_match.as_str().to_string(),
                        kind: SymbolKind::ENUM,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: line_idx as u32,
                                    character: name_match.start() as u32,
                                },
                                end: Position {
                                    line: line_idx as u32,
                                    character: name_match.end() as u32,
                                },
                            },
                        },
                        container_name: None,
                    });
                }
            }
        }
    }

    // Trait pattern
    if let Some(trait_re) = get_regex(r"^(?:pub\s+)?trait\s+([A-Z][a-zA-Z0-9_]*)\s*[<{]") {
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(caps) = trait_re.captures(line) {
                if let Some(name_match) = caps.get(1) {
                    symbols.push(ExtractedSymbol {
                        name: name_match.as_str().to_string(),
                        kind: SymbolKind::INTERFACE,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: line_idx as u32,
                                    character: name_match.start() as u32,
                                },
                                end: Position {
                                    line: line_idx as u32,
                                    character: name_match.end() as u32,
                                },
                            },
                        },
                        container_name: None,
                    });
                }
            }
        }
    }

    // Constant pattern
    if let Some(const_re) = get_regex(r"^(?:pub\s+)?const\s+([A-Z_][A-Z0-9_]*)\s*[=:]") {
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(caps) = const_re.captures(line) {
                if let Some(name_match) = caps.get(1) {
                    symbols.push(ExtractedSymbol {
                        name: name_match.as_str().to_string(),
                        kind: SymbolKind::CONSTANT,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: line_idx as u32,
                                    character: name_match.start() as u32,
                                },
                                end: Position {
                                    line: line_idx as u32,
                                    character: name_match.end() as u32,
                                },
                            },
                        },
                        container_name: None,
                    });
                }
            }
        }
    }

    symbols
}

/// Find container name (impl block or similar) for a nested symbol
fn find_container(lines: &[&str], current_line: usize) -> Option<String> {
    let impl_re = get_regex(r"^impl\s+(?:([A-Z][a-zA-Z0-9_<>]*)\s+for\s+)?([A-Z][a-zA-Z0-9_<>]*)")?;

    for idx in (0..current_line).rev() {
        let line = lines[idx];
        let trimmed = line.trim_start();

        // Check for impl block
        if let Some(caps) = impl_re.captures(trimmed) {
            if let Some(type_match) = caps.get(2) {
                return Some(type_match.as_str().to_string());
            }
        }

        // Check for struct/class/enum with methods
        if trimmed.starts_with("struct ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub class ")
        {
            let name_re = get_regex(r"(?:struct|class)\s+([A-Z][a-zA-Z0-9_]*)")?;
            if let Some(caps) = name_re.captures(trimmed) {
                if let Some(name) = caps.get(1) {
                    return Some(name.as_str().to_string());
                }
            }
        }
    }

    None
}

/// Filter symbols by query string (fuzzy match)
pub fn filter_symbols(symbols: &[ExtractedSymbol], query: &str) -> Vec<SymbolInformation> {
    if query.is_empty() {
        return symbols
            .iter()
            .map(|s| to_symbol_information(s))
            .collect();
    }

    let query_lower = query.to_lowercase();

    symbols
        .iter()
        .filter(|s| {
            let name_lower = s.name.to_lowercase();
            // Simple fuzzy match: query chars appear in order
            fuzzy_match(&name_lower, &query_lower)
        })
        .map(|s| to_symbol_information(s))
        .collect()
}

fn fuzzy_match(text: &str, pattern: &str) -> bool {
    let mut text_chars = text.chars().peekable();

    for pattern_char in pattern.chars() {
        loop {
            match text_chars.next() {
                Some(text_char) if text_char == pattern_char => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }

    true
}

#[allow(deprecated)]
fn to_symbol_information(symbol: &ExtractedSymbol) -> SymbolInformation {
    SymbolInformation {
        name: symbol.name.clone(),
        kind: symbol.kind,
        tags: None,
        deprecated: None,
        location: symbol.location.clone(),
        container_name: symbol.container_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_functions() {
        let content = r#"
fn main() {
    println("hello")
}

fn helper() -> int {
    42
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let symbols = extract_symbols(&uri, content);

        let functions: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::FUNCTION)
            .collect();
        assert_eq!(functions.len(), 2);
        assert!(functions.iter().any(|s| s.name == "main"));
        assert!(functions.iter().any(|s| s.name == "helper"));
    }

    #[test]
    fn test_extract_types() {
        let content = r#"
struct Point {
    x: int,
    y: int,
}

enum Color {
    Red,
    Green,
    Blue,
}

trait Display {
    fn display(self) -> string
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let symbols = extract_symbols(&uri, content);

        assert!(symbols.iter().any(|s| s.name == "Point" && s.kind == SymbolKind::STRUCT));
        assert!(symbols.iter().any(|s| s.name == "Color" && s.kind == SymbolKind::ENUM));
        assert!(symbols.iter().any(|s| s.name == "Display" && s.kind == SymbolKind::INTERFACE));
    }

    #[test]
    fn test_container_name() {
        let content = r#"
impl Point {
    fn new(x: int, y: int) -> Point {
        Point { x, y }
    }
}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let symbols = extract_symbols(&uri, content);

        let new_fn = symbols.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(new_fn.container_name, Some("Point".to_string()));
    }

    #[test]
    fn test_fuzzy_filter() {
        let content = r#"
fn getUserName() {}
fn getUser() {}
fn createUser() {}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let symbols = extract_symbols(&uri, content);

        // "gun" should match "getUserName"
        let filtered = filter_symbols(&symbols, "gun");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "getUserName");

        // "gu" should match both getUser*
        let filtered = filter_symbols(&symbols, "gu");
        assert_eq!(filtered.len(), 2);

        // Empty query returns all
        let filtered = filter_symbols(&symbols, "");
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_pub_symbols() {
        let content = r#"
pub fn exported() {}
pub struct PublicType {}
"#;
        let uri = Url::parse("file:///test.li").unwrap();
        let symbols = extract_symbols(&uri, content);

        assert!(symbols.iter().any(|s| s.name == "exported"));
        assert!(symbols.iter().any(|s| s.name == "PublicType"));
    }
}
