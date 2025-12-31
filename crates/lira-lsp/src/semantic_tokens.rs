//! Semantic tokens support for Lira
//!
//! Provides enhanced syntax highlighting with semantic information.

use regex::Regex;
use tower_lsp::lsp_types::*;

/// Standard semantic token types for LSP
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::MODIFIER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::OPERATOR,
];

/// Standard semantic token modifiers for LSP
pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEFAULT_LIBRARY,
    SemanticTokenModifier::MODIFICATION,
];

/// Get the semantic token legend (types + modifiers)
pub fn get_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: TOKEN_MODIFIERS.to_vec(),
    }
}

/// A semantic token with position and type info
#[derive(Debug, Clone)]
struct Token {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

/// Get semantic tokens for a document
pub fn get_semantic_tokens(content: &str) -> SemanticTokens {
    let mut tokens: Vec<Token> = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = line_idx as u32;
        extract_line_tokens(line, line_num, &mut tokens);
    }

    // Sort tokens by position
    tokens.sort_by(|a, b| a.line.cmp(&b.line).then(a.start.cmp(&b.start)));

    // Convert to delta-encoded format
    let mut data = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for token in tokens {
        let delta_line = token.line - prev_line;
        let delta_start = if delta_line == 0 {
            token.start - prev_start
        } else {
            token.start
        };

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: token.modifiers,
        });

        prev_line = token.line;
        prev_start = token.start;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

fn extract_line_tokens(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    let trimmed = line.trim_start();
    let indent = (line.len() - trimmed.len()) as u32;

    // Skip empty lines
    if trimmed.is_empty() {
        return;
    }

    // Comments
    if trimmed.starts_with("//") {
        tokens.push(Token {
            line: line_num,
            start: indent,
            length: trimmed.len() as u32,
            token_type: token_type_index(SemanticTokenType::COMMENT),
            modifiers: 0,
        });
        return;
    }

    // Extract tokens from the line
    extract_keywords(line, line_num, tokens);
    extract_strings(line, line_num, tokens);
    extract_numbers(line, line_num, tokens);
    extract_declarations(line, line_num, tokens);
    extract_types(line, line_num, tokens);
    extract_function_calls(line, line_num, tokens);
    extract_operators(line, line_num, tokens);
}

fn extract_keywords(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    let keywords = [
        "fn", "let", "var", "const", "struct", "class", "enum", "trait", "impl", "if", "else",
        "match", "while", "for", "loop", "break", "continue", "return", "spawn", "select", "async",
        "try", "catch", "finally", "import", "use", "as", "is", "in", "pub", "priv", "true",
        "false", "null", "self", "Self", "super", "extends",
    ];

    for keyword in keywords {
        let pattern = format!(r"\b{}\b", regex::escape(keyword));
        if let Ok(re) = Regex::new(&pattern) {
            for mat in re.find_iter(line) {
                // Determine if it's a modifier or regular keyword
                let token_type = if matches!(keyword, "pub" | "priv" | "async" | "const") {
                    SemanticTokenType::MODIFIER
                } else {
                    SemanticTokenType::KEYWORD
                };

                tokens.push(Token {
                    line: line_num,
                    start: mat.start() as u32,
                    length: mat.len() as u32,
                    token_type: token_type_index(token_type),
                    modifiers: 0,
                });
            }
        }
    }
}

fn extract_strings(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    // Double-quoted strings
    if let Ok(re) = Regex::new(r#""([^"\\]|\\.)*""#) {
        for mat in re.find_iter(line) {
            tokens.push(Token {
                line: line_num,
                start: mat.start() as u32,
                length: mat.len() as u32,
                token_type: token_type_index(SemanticTokenType::STRING),
                modifiers: 0,
            });
        }
    }

    // Single-quoted chars
    if let Ok(re) = Regex::new(r"'([^'\\]|\\.)'") {
        for mat in re.find_iter(line) {
            tokens.push(Token {
                line: line_num,
                start: mat.start() as u32,
                length: mat.len() as u32,
                token_type: token_type_index(SemanticTokenType::STRING),
                modifiers: 0,
            });
        }
    }
}

fn extract_numbers(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    // Hex, binary, and decimal numbers
    if let Ok(re) = Regex::new(r"\b(0x[0-9a-fA-F]+|0b[01]+|\d+\.?\d*([eE][+-]?\d+)?)\b") {
        for mat in re.find_iter(line) {
            tokens.push(Token {
                line: line_num,
                start: mat.start() as u32,
                length: mat.len() as u32,
                token_type: token_type_index(SemanticTokenType::NUMBER),
                modifiers: 0,
            });
        }
    }
}

fn extract_declarations(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    // Function declarations: fn name
    if let Ok(re) = Regex::new(r"\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::FUNCTION),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                });
            }
        }
    }

    // Variable declarations: let/var name
    if let Ok(re) = Regex::new(r"\b(let|var)\s+([a-zA-Z_][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(2) {
                let is_readonly = caps.get(1).map(|m| m.as_str() == "let").unwrap_or(false);
                let modifiers = if is_readonly {
                    modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::READONLY,
                    ])
                } else {
                    modifier_bits(&[SemanticTokenModifier::DECLARATION])
                };

                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::VARIABLE),
                    modifiers,
                });
            }
        }
    }

    // Constant declarations: const NAME
    if let Ok(re) = Regex::new(r"\bconst\s+([A-Z][A-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::VARIABLE),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::READONLY,
                        SemanticTokenModifier::STATIC,
                    ]),
                });
            }
        }
    }

    // Struct declarations: struct Name
    if let Ok(re) = Regex::new(r"\bstruct\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::STRUCT),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                });
            }
        }
    }

    // Class declarations: class Name
    if let Ok(re) = Regex::new(r"\bclass\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::CLASS),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                });
            }
        }
    }

    // Enum declarations: enum Name
    if let Ok(re) = Regex::new(r"\benum\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::ENUM),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                });
            }
        }
    }

    // Trait declarations: trait Name
    if let Ok(re) = Regex::new(r"\btrait\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::INTERFACE),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                });
            }
        }
    }

    // Impl blocks: impl Type or impl Trait for Type
    if let Ok(re) = Regex::new(r"\bimpl\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::TYPE),
                    modifiers: 0,
                });
            }
        }
    }

    // Parameter declarations in function signatures: (name: Type)
    if let Ok(re) = Regex::new(r"\(([^)]*)\)") {
        let param_re = Regex::new(r"([a-z_][a-zA-Z0-9_]*)\s*:").ok();
        for caps in re.captures_iter(line) {
            if let Some(params) = caps.get(1) {
                let params_str = params.as_str();
                let params_start = params.start();

                // Parse individual parameters
                if let Some(ref param_re) = param_re {
                    for param_caps in param_re.captures_iter(params_str) {
                        if let Some(param_name) = param_caps.get(1) {
                            // Skip 'self' as it's a keyword
                            if param_name.as_str() != "self" {
                                tokens.push(Token {
                                    line: line_num,
                                    start: (params_start + param_name.start()) as u32,
                                    length: param_name.len() as u32,
                                    token_type: token_type_index(SemanticTokenType::PARAMETER),
                                    modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}

fn extract_types(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    // Built-in types
    let builtin_types = [
        "int", "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "float",
        "bool", "string", "char", "void", "List", "Map", "Set", "Option", "Result", "Channel",
    ];

    for type_name in builtin_types {
        let pattern = format!(r"\b{}\b", regex::escape(type_name));
        if let Ok(re) = Regex::new(&pattern) {
            for mat in re.find_iter(line) {
                // Check if this is in a type context (after : or ->, in generics, etc.)
                let before = &line[..mat.start()];
                if before.ends_with(": ")
                    || before.ends_with(":")
                    || before.ends_with("-> ")
                    || before.ends_with("->")
                    || before.ends_with("< ")
                    || before.ends_with("<")
                    || before.ends_with(", ")
                    || before.ends_with(",")
                {
                    tokens.push(Token {
                        line: line_num,
                        start: mat.start() as u32,
                        length: mat.len() as u32,
                        token_type: token_type_index(SemanticTokenType::TYPE),
                        modifiers: modifier_bits(&[SemanticTokenModifier::DEFAULT_LIBRARY]),
                    });
                }
            }
        }
    }

    // Type annotations with capital letters (user-defined types)
    if let Ok(re) = Regex::new(r":\s*([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(type_name) = caps.get(1) {
                // Skip if it's a builtin (already handled)
                if !builtin_types.contains(&type_name.as_str()) {
                    tokens.push(Token {
                        line: line_num,
                        start: type_name.start() as u32,
                        length: type_name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::TYPE),
                        modifiers: 0,
                    });
                }
            }
        }
    }

    // Return types: -> Type
    if let Ok(re) = Regex::new(r"->\s*([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(type_name) = caps.get(1) {
                if !builtin_types.contains(&type_name.as_str()) {
                    tokens.push(Token {
                        line: line_num,
                        start: type_name.start() as u32,
                        length: type_name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::TYPE),
                        modifiers: 0,
                    });
                }
            }
        }
    }

    // Generic type parameters: <T, U>
    if let Ok(re) = Regex::new(r"<([A-Z][a-zA-Z0-9_]*(?:\s*,\s*[A-Z][a-zA-Z0-9_]*)*)>") {
        let type_param_re = Regex::new(r"[A-Z][a-zA-Z0-9_]*").ok();
        for caps in re.captures_iter(line) {
            if let Some(params) = caps.get(1) {
                let params_str = params.as_str();
                let base_start = params.start();

                // Extract individual type parameters
                if let Some(ref param_re) = type_param_re {
                    for param_mat in param_re.find_iter(params_str) {
                        let param_name = param_mat.as_str();
                        // Single uppercase letters are type parameters
                        if param_name.len() == 1 {
                            tokens.push(Token {
                                line: line_num,
                                start: (base_start + param_mat.start()) as u32,
                                length: param_mat.len() as u32,
                                token_type: token_type_index(SemanticTokenType::TYPE_PARAMETER),
                                modifiers: 0,
                            });
                        }
                    }
                }
            }
        }
    }
}

fn extract_function_calls(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    // Function calls: name(
    if let Ok(re) = Regex::new(r"\b([a-z_][a-zA-Z0-9_]*)\s*\(") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                let func_name = name.as_str();

                // Skip keywords that look like function calls
                let keywords = ["if", "while", "for", "match", "spawn", "select"];
                if keywords.contains(&func_name) {
                    continue;
                }

                // Check if it's a builtin function
                let builtins = [
                    "print",
                    "println",
                    "debug",
                    "assert",
                    "len",
                    "push",
                    "pop",
                    "append",
                    "panic",
                    "todo",
                    "unreachable",
                ];
                let modifiers = if builtins.contains(&func_name) {
                    modifier_bits(&[SemanticTokenModifier::DEFAULT_LIBRARY])
                } else {
                    0
                };

                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::FUNCTION),
                    modifiers,
                });
            }
        }
    }

    // Method calls: .name(
    if let Ok(re) = Regex::new(r"\.([a-z_][a-zA-Z0-9_]*)\s*\(") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: name.start() as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::METHOD),
                    modifiers: 0,
                });
            }
        }
    }

    // Enum variant access: Type::Variant
    if let Ok(re) = Regex::new(r"([A-Z][a-zA-Z0-9_]*)::([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(type_name) = caps.get(1) {
                tokens.push(Token {
                    line: line_num,
                    start: type_name.start() as u32,
                    length: type_name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::ENUM),
                    modifiers: 0,
                });
            }
            if let Some(variant) = caps.get(2) {
                tokens.push(Token {
                    line: line_num,
                    start: variant.start() as u32,
                    length: variant.len() as u32,
                    token_type: token_type_index(SemanticTokenType::ENUM_MEMBER),
                    modifiers: 0,
                });
            }
        }
    }
}

fn extract_operators(line: &str, line_num: u32, tokens: &mut Vec<Token>) {
    // Multi-character operators first (to avoid partial matches)
    let operators = [
        "==", "!=", "<=", ">=", "&&", "||", "??", "->", "=>", "+=", "-=", "*=", "/=", "%=", "&=",
        "|=", "^=", "<<=", ">>=", "++", "--", "..", "..=", "::", "<-",
    ];

    for op in operators {
        let pattern = regex::escape(op);
        if let Ok(re) = Regex::new(&pattern) {
            for mat in re.find_iter(line) {
                tokens.push(Token {
                    line: line_num,
                    start: mat.start() as u32,
                    length: mat.len() as u32,
                    token_type: token_type_index(SemanticTokenType::OPERATOR),
                    modifiers: 0,
                });
            }
        }
    }
}

fn token_type_index(token_type: SemanticTokenType) -> u32 {
    TOKEN_TYPES
        .iter()
        .position(|t| *t == token_type)
        .unwrap_or(0) as u32
}

fn modifier_bits(modifiers: &[SemanticTokenModifier]) -> u32 {
    let mut bits = 0u32;
    for modifier in modifiers {
        if let Some(idx) = TOKEN_MODIFIERS.iter().position(|m| m == modifier) {
            bits |= 1 << idx;
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to check if a token type exists in the tokens
    fn has_token_type(tokens: &SemanticTokens, token_type: SemanticTokenType) -> bool {
        let type_idx = token_type_index(token_type);
        tokens.data.iter().any(|t| t.token_type == type_idx)
    }

    // Helper to count tokens of a specific type
    fn count_token_type(tokens: &SemanticTokens, token_type: SemanticTokenType) -> usize {
        let type_idx = token_type_index(token_type);
        tokens
            .data
            .iter()
            .filter(|t| t.token_type == type_idx)
            .count()
    }

    // ==================== KEYWORD TESTS ====================

    #[test]
    fn test_keyword_fn() {
        let content = "fn main() {}";
        let tokens = get_semantic_tokens(content);
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
    }

    #[test]
    fn test_keyword_let_var() {
        let content = "let x = 1\nvar y = 2";
        let tokens = get_semantic_tokens(content);
        assert!(count_token_type(&tokens, SemanticTokenType::KEYWORD) >= 2);
    }

    #[test]
    fn test_keyword_control_flow() {
        let content = "if true { } else { } while false { } for i in items { } match x { } loop { break } continue return";
        let tokens = get_semantic_tokens(content);
        // Should find: if, else, while, for, in, match, loop, break, continue, return
        assert!(count_token_type(&tokens, SemanticTokenType::KEYWORD) >= 10);
    }

    #[test]
    fn test_keyword_concurrency() {
        let content = "spawn { } select { } async";
        let tokens = get_semantic_tokens(content);
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
    }

    #[test]
    fn test_keyword_literals() {
        let content = "let a = true\nlet b = false\nlet c = null";
        let tokens = get_semantic_tokens(content);
        // true, false, null should be keywords
        assert!(count_token_type(&tokens, SemanticTokenType::KEYWORD) >= 6); // let x3 + true/false/null
    }

    #[test]
    fn test_modifier_keywords() {
        let content = "pub fn foo() {}\npriv const X = 1\nasync fn bar() {}";
        let tokens = get_semantic_tokens(content);
        // pub, priv, const, async are modifiers
        assert!(has_token_type(&tokens, SemanticTokenType::MODIFIER));
    }

    // ==================== DECLARATION TESTS ====================

    #[test]
    fn test_function_declaration() {
        let content = "fn add(a: int, b: int) -> int { a + b }";
        let tokens = get_semantic_tokens(content);

        // Should have: fn (keyword), add (function with declaration modifier)
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));

        // Check function name has declaration modifier
        let func_type_idx = token_type_index(SemanticTokenType::FUNCTION);
        let func_token = tokens.data.iter().find(|t| t.token_type == func_type_idx);
        assert!(func_token.is_some());
        let decl_bit = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        assert!(func_token.unwrap().token_modifiers_bitset & decl_bit != 0);
    }

    #[test]
    fn test_struct_declaration() {
        let content = "struct Point { x: float, y: float }";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD)); // struct
        assert!(has_token_type(&tokens, SemanticTokenType::STRUCT)); // Point

        // Verify struct has declaration+definition modifiers
        let struct_type_idx = token_type_index(SemanticTokenType::STRUCT);
        let struct_token = tokens.data.iter().find(|t| t.token_type == struct_type_idx);
        assert!(struct_token.is_some());
    }

    #[test]
    fn test_class_declaration() {
        let content = "class Animal { fn speak(self) {} }";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::CLASS));
    }

    #[test]
    fn test_enum_declaration() {
        let content = "enum Color { Red, Green, Blue }";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::ENUM));
    }

    #[test]
    fn test_trait_declaration() {
        let content = "trait Display { fn display(self) -> string }";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::INTERFACE)); // trait maps to INTERFACE
    }

    #[test]
    fn test_impl_block() {
        let content = "impl Point { fn new() -> Point {} }";
        let tokens = get_semantic_tokens(content);

        // impl keyword and Point type
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_impl_trait_for_type() {
        let content = "impl Display for Point { fn display(self) -> string {} }";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD)); // impl, for
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    // ==================== VARIABLE TESTS ====================

    #[test]
    fn test_let_readonly() {
        let content = "let x = 42";
        let tokens = get_semantic_tokens(content);

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let var_token = tokens.data.iter().find(|t| t.token_type == var_type_idx);
        assert!(var_token.is_some());

        // Should have readonly modifier
        let readonly_bit = modifier_bits(&[SemanticTokenModifier::READONLY]);
        assert!(var_token.unwrap().token_modifiers_bitset & readonly_bit != 0);
    }

    #[test]
    fn test_var_mutable() {
        let content = "var count = 0";
        let tokens = get_semantic_tokens(content);

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let var_token = tokens.data.iter().find(|t| t.token_type == var_type_idx);
        assert!(var_token.is_some());

        // Should NOT have readonly modifier
        let readonly_bit = modifier_bits(&[SemanticTokenModifier::READONLY]);
        assert!(var_token.unwrap().token_modifiers_bitset & readonly_bit == 0);
    }

    #[test]
    fn test_const_declaration() {
        let content = "const MAX_SIZE = 100";
        let tokens = get_semantic_tokens(content);

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let const_token = tokens.data.iter().find(|t| t.token_type == var_type_idx);
        assert!(const_token.is_some());

        // Should have readonly + static modifiers
        let expected_bits = modifier_bits(&[
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::STATIC,
        ]);
        assert!(const_token.unwrap().token_modifiers_bitset & expected_bits != 0);
    }

    // ==================== PARAMETER TESTS ====================

    #[test]
    fn test_function_parameters() {
        let content = "fn process(input: string, count: int) {}";
        let tokens = get_semantic_tokens(content);

        // Should have 2 parameters
        assert_eq!(count_token_type(&tokens, SemanticTokenType::PARAMETER), 2);
    }

    #[test]
    fn test_self_parameter_skipped() {
        let content = "fn method(self, other: int) {}";
        let tokens = get_semantic_tokens(content);

        // self should be a keyword, not a parameter
        // only 'other' should be a parameter
        assert_eq!(count_token_type(&tokens, SemanticTokenType::PARAMETER), 1);
    }

    // ==================== TYPE TESTS ====================

    #[test]
    fn test_builtin_types() {
        let content =
            "let a: int = 1\nlet b: float = 2.0\nlet c: bool = true\nlet d: string = \"hi\"";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_sized_integer_types() {
        let content = "let a: int8 = 1\nlet b: int16 = 2\nlet c: int32 = 3\nlet d: int64 = 4";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::TYPE) >= 4);
    }

    #[test]
    fn test_unsigned_types() {
        let content = "let a: uint8 = 1\nlet b: uint16 = 2\nlet c: uint32 = 3\nlet d: uint64 = 4";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::TYPE) >= 4);
    }

    #[test]
    fn test_collection_types() {
        let content = "let a: List<int> = []\nlet b: Map<string, int> = {}\nlet c: Set<int> = {}";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_return_type() {
        let content = "fn get_value() -> int { 42 }";
        let tokens = get_semantic_tokens(content);

        // int after -> should be a type
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_user_defined_type() {
        let content = "let p: Point = Point { x: 0, y: 0 }";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_generic_type_parameter() {
        let content = "fn identity<T>(x: T) -> T { x }";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE_PARAMETER));
    }

    // ==================== FUNCTION CALL TESTS ====================

    #[test]
    fn test_function_call() {
        let content = "let result = calculate(10, 20)";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));
    }

    #[test]
    fn test_builtin_function_call() {
        let content = "println(\"Hello\")\nprint(42)\ndebug(x)";
        let tokens = get_semantic_tokens(content);

        // Should have builtin functions with DEFAULT_LIBRARY modifier
        let func_type_idx = token_type_index(SemanticTokenType::FUNCTION);
        let default_lib_bit = modifier_bits(&[SemanticTokenModifier::DEFAULT_LIBRARY]);

        let builtin_count = tokens
            .data
            .iter()
            .filter(|t| {
                t.token_type == func_type_idx && t.token_modifiers_bitset & default_lib_bit != 0
            })
            .count();

        assert!(builtin_count >= 3);
    }

    #[test]
    fn test_method_call() {
        let content = "let len = text.length()\nlist.push(item)";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::METHOD) >= 2);
    }

    #[test]
    fn test_chained_method_calls() {
        let content = "items.filter().map().collect()";
        let tokens = get_semantic_tokens(content);

        assert_eq!(count_token_type(&tokens, SemanticTokenType::METHOD), 3);
    }

    // ==================== ENUM VARIANT TESTS ====================

    #[test]
    fn test_enum_variant_access() {
        let content = "let color = Color::Red";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::ENUM));
        assert!(has_token_type(&tokens, SemanticTokenType::ENUM_MEMBER));
    }

    #[test]
    fn test_option_variant() {
        let content = "let x = Option::Some(42)\nlet y = Option::None";
        let tokens = get_semantic_tokens(content);

        assert_eq!(count_token_type(&tokens, SemanticTokenType::ENUM), 2);
        assert_eq!(count_token_type(&tokens, SemanticTokenType::ENUM_MEMBER), 2);
    }

    #[test]
    fn test_result_variant() {
        let content = "Result::Ok(value)\nResult::Err(error)";
        let tokens = get_semantic_tokens(content);

        assert_eq!(count_token_type(&tokens, SemanticTokenType::ENUM), 2);
        assert_eq!(count_token_type(&tokens, SemanticTokenType::ENUM_MEMBER), 2);
    }

    // ==================== LITERAL TESTS ====================

    #[test]
    fn test_number_literals() {
        let content = "let a = 42\nlet b = 3.14\nlet c = 0xFF\nlet d = 0b1010";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::NUMBER) >= 4);
    }

    #[test]
    fn test_string_literals() {
        let content = r#"let s = "hello world""#;
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::STRING));
    }

    #[test]
    fn test_char_literal() {
        let content = "let c = 'a'";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::STRING)); // chars use STRING type
    }

    #[test]
    fn test_escape_sequences() {
        let content = r#"let s = "hello\nworld""#;
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::STRING));
    }

    // ==================== COMMENT TESTS ====================

    #[test]
    fn test_line_comment() {
        let content = "// This is a comment";
        let tokens = get_semantic_tokens(content);

        assert_eq!(tokens.data.len(), 1);
        assert_eq!(
            tokens.data[0].token_type,
            token_type_index(SemanticTokenType::COMMENT)
        );
    }

    #[test]
    fn test_comment_with_code() {
        let content = "let x = 42 // inline comment";
        let tokens = get_semantic_tokens(content);

        // Should have both code tokens and comment is NOT parsed (we only check line-start comments)
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::VARIABLE));
    }

    // ==================== OPERATOR TESTS ====================

    #[test]
    fn test_comparison_operators() {
        let content = "a == b\nc != d\ne <= f\ng >= h";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::OPERATOR) >= 4);
    }

    #[test]
    fn test_logical_operators() {
        let content = "a && b || c";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::OPERATOR) >= 2);
    }

    #[test]
    fn test_compound_assignment() {
        let content = "x += 1\ny -= 2\nz *= 3\nw /= 4";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::OPERATOR) >= 4);
    }

    #[test]
    fn test_increment_decrement() {
        let content = "x++\ny--\n++a\n--b";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::OPERATOR) >= 4);
    }

    #[test]
    fn test_range_operators() {
        let content = "1..10\n1..=10";
        let tokens = get_semantic_tokens(content);

        assert!(count_token_type(&tokens, SemanticTokenType::OPERATOR) >= 2);
    }

    #[test]
    fn test_null_coalescing() {
        let content = "x ?? default";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::OPERATOR));
    }

    #[test]
    fn test_arrow_operators() {
        let content = "fn foo() -> int {}\nmatch x { a => b }";
        let tokens = get_semantic_tokens(content);

        // -> and => operators
        assert!(count_token_type(&tokens, SemanticTokenType::OPERATOR) >= 2);
    }

    // ==================== MULTI-LINE TESTS ====================

    #[test]
    fn test_multiline_function() {
        let content = r#"
fn calculate(x: int, y: int) -> int {
    let sum = x + y
    let product = x * y
    return sum + product
}
"#;
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));
        assert!(has_token_type(&tokens, SemanticTokenType::PARAMETER));
        assert!(has_token_type(&tokens, SemanticTokenType::VARIABLE));
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
    }

    #[test]
    fn test_multiline_struct_with_impl() {
        let content = r#"
struct Point {
    x: float,
    y: float,
}

impl Point {
    fn new(x: float, y: float) -> Point {
        Point { x, y }
    }
}
"#;
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::STRUCT));
        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_delta_encoding() {
        let content = "let a = 1\nlet b = 2\nlet c = 3";
        let tokens = get_semantic_tokens(content);

        // Verify delta encoding: tokens on different lines should have delta_line > 0
        let mut found_line_delta = false;
        for token in &tokens.data {
            if token.delta_line > 0 {
                found_line_delta = true;
                break;
            }
        }
        assert!(found_line_delta);
    }

    // ==================== EDGE CASE TESTS ====================

    #[test]
    fn test_empty_content() {
        let content = "";
        let tokens = get_semantic_tokens(content);

        assert!(tokens.data.is_empty());
    }

    #[test]
    fn test_whitespace_only() {
        let content = "   \n\t\n   ";
        let tokens = get_semantic_tokens(content);

        assert!(tokens.data.is_empty());
    }

    #[test]
    fn test_mixed_content() {
        let content = r#"
// A simple counter module
const MAX = 100

struct Counter {
    value: int,
}

impl Counter {
    fn new() -> Counter {
        Counter { value: 0 }
    }

    fn increment(self) {
        if self.value < MAX {
            self.value += 1
        }
    }
}

fn main() {
    let counter = Counter::new()
    counter.increment()
    println(counter.value)
}
"#;
        let tokens = get_semantic_tokens(content);

        // Comprehensive check of mixed content
        assert!(has_token_type(&tokens, SemanticTokenType::COMMENT));
        assert!(has_token_type(&tokens, SemanticTokenType::MODIFIER)); // const
        assert!(has_token_type(&tokens, SemanticTokenType::STRUCT));
        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));
        assert!(has_token_type(&tokens, SemanticTokenType::METHOD));
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_keyword_not_in_identifier() {
        // "letter" contains "let" but should not match keyword
        let content = "let letter = \"a\"";
        let tokens = get_semantic_tokens(content);

        // Should have exactly 1 'let' keyword, not match 'let' in 'letter'
        let keyword_count = count_token_type(&tokens, SemanticTokenType::KEYWORD);
        let var_count = count_token_type(&tokens, SemanticTokenType::VARIABLE);

        assert_eq!(keyword_count, 1); // just 'let'
        assert_eq!(var_count, 1); // 'letter' as variable
    }

    #[test]
    fn test_deeply_nested_generics() {
        let content = "let map: Map<string, List<Option<int>>> = {}";
        let tokens = get_semantic_tokens(content);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    // ==================== HELPER FUNCTION TESTS ====================

    #[test]
    fn test_token_type_index() {
        assert_eq!(token_type_index(SemanticTokenType::NAMESPACE), 0);
        assert_eq!(token_type_index(SemanticTokenType::KEYWORD), 13);
        assert_eq!(token_type_index(SemanticTokenType::COMMENT), 15);
    }

    #[test]
    fn test_modifier_bits_single() {
        let bits = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        assert_eq!(bits, 1); // First modifier = bit 0
    }

    #[test]
    fn test_modifier_bits_multiple() {
        let bits = modifier_bits(&[
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::READONLY,
        ]);
        // DECLARATION = bit 0, READONLY = bit 2
        assert_eq!(bits, 0b101);
    }

    #[test]
    fn test_get_legend() {
        let legend = get_legend();

        assert_eq!(legend.token_types.len(), TOKEN_TYPES.len());
        assert_eq!(legend.token_modifiers.len(), TOKEN_MODIFIERS.len());
    }
}
