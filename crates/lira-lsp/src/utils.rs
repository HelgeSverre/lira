//! Shared utilities for the Lira Language Server
//!
//! This module provides common functionality used across multiple LSP features:
//! - Word/identifier extraction at cursor positions
//! - String and comment detection
//! - Safe regex compilation with caching
//! - UTF-8 aware position handling

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::sync::RwLock;

/// Cache for compiled regexes to avoid recompilation
static REGEX_CACHE: Lazy<RwLock<HashMap<String, Regex>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Get or compile a regex pattern, with caching
pub fn get_regex(pattern: &str) -> Option<Regex> {
    // Try read lock first
    {
        let cache = REGEX_CACHE.read().ok()?;
        if let Some(re) = cache.get(pattern) {
            return Some(re.clone());
        }
    }

    // Compile and cache
    let re = Regex::new(pattern).ok()?;
    {
        if let Ok(mut cache) = REGEX_CACHE.write() {
            cache.insert(pattern.to_string(), re.clone());
        }
    }
    Some(re)
}

/// Information about a word at a position
#[derive(Debug, Clone, PartialEq)]
pub struct WordInfo {
    /// The word text
    pub text: String,
    /// Start column (0-indexed, in characters not bytes)
    pub start_col: usize,
    /// End column (0-indexed, in characters not bytes)
    pub end_col: usize,
}

/// Get the word (identifier) at the given column position in a line.
/// Handles UTF-8 correctly by working with character indices.
///
/// # Arguments
/// * `line` - The line of text
/// * `col` - The column position (0-indexed, in characters)
///
/// # Returns
/// `Some(WordInfo)` if an identifier is found at the position, `None` otherwise
pub fn get_word_at_position(line: &str, col: usize) -> Option<WordInfo> {
    let chars: Vec<char> = line.chars().collect();

    // Bounds check
    if col >= chars.len() {
        return None;
    }

    // Check if we're on an identifier character
    let ch = chars[col];
    if !is_identifier_char(ch) {
        return None;
    }

    // Find start of word (scan backwards)
    let mut start = col;
    while start > 0 && is_identifier_char(chars[start - 1]) {
        start -= 1;
    }

    // Find end of word (scan forwards)
    let mut end = col;
    while end < chars.len() && is_identifier_char(chars[end]) {
        end += 1;
    }

    // Empty word check
    if start == end {
        return None;
    }

    let text: String = chars[start..end].iter().collect();
    Some(WordInfo {
        text,
        start_col: start,
        end_col: end,
    })
}

/// Check if a character can be part of an identifier
#[inline]
pub fn is_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Check if a character can start an identifier
#[inline]
pub fn is_identifier_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

/// Context information for string/comment detection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextContext {
    /// Normal code
    Code,
    /// Inside a line comment (// ...)
    LineComment,
    /// Inside a block comment (/* ... */)
    BlockComment,
    /// Inside a double-quoted string ("...")
    String,
    /// Inside a character literal ('.')
    Char,
}

/// Determine the text context at a specific position in a line.
/// This properly handles escape sequences and nested structures.
///
/// # Arguments
/// * `line` - The line of text
/// * `pos` - The byte position to check
///
/// # Returns
/// The `TextContext` at the given position
pub fn get_context_at_position(line: &str, pos: usize) -> TextContext {
    let before = if pos <= line.len() {
        &line[..pos]
    } else {
        line
    };

    let mut context = TextContext::Code;
    let mut chars = before.chars().peekable();
    let mut prev_char = '\0';

    while let Some(ch) = chars.next() {
        match context {
            TextContext::Code => {
                if ch == '/' {
                    if chars.peek() == Some(&'/') {
                        return TextContext::LineComment;
                    } else if chars.peek() == Some(&'*') {
                        chars.next();
                        context = TextContext::BlockComment;
                        continue;
                    }
                } else if ch == '"' && prev_char != '\\' {
                    context = TextContext::String;
                } else if ch == '\'' && prev_char != '\\' {
                    context = TextContext::Char;
                }
            }
            TextContext::String => {
                if ch == '"' && prev_char != '\\' {
                    context = TextContext::Code;
                }
            }
            TextContext::Char => {
                if ch == '\'' && prev_char != '\\' {
                    context = TextContext::Code;
                }
            }
            TextContext::BlockComment => {
                if prev_char == '*' && ch == '/' {
                    context = TextContext::Code;
                }
            }
            TextContext::LineComment => {
                // Line comments extend to end of line
                return TextContext::LineComment;
            }
        }
        prev_char = ch;
    }

    context
}

/// Check if a position is inside a string or comment
pub fn is_in_string_or_comment(line: &str, pos: usize) -> bool {
    matches!(
        get_context_at_position(line, pos),
        TextContext::String
            | TextContext::Char
            | TextContext::LineComment
            | TextContext::BlockComment
    )
}

/// Convert a character column to a byte offset in a string.
/// Handles UTF-8 correctly.
pub fn char_col_to_byte_offset(line: &str, char_col: usize) -> usize {
    line.chars()
        .take(char_col)
        .map(|c| c.len_utf8())
        .sum()
}

/// Convert a byte offset to a character column.
/// Handles UTF-8 correctly.
pub fn byte_offset_to_char_col(line: &str, byte_offset: usize) -> usize {
    line[..byte_offset.min(line.len())].chars().count()
}

/// Get the byte range for a word in a line given character positions
pub fn word_byte_range(line: &str, start_col: usize, end_col: usize) -> (usize, usize) {
    let start_byte = char_col_to_byte_offset(line, start_col);
    let end_byte = char_col_to_byte_offset(line, end_col);
    (start_byte, end_byte)
}

/// Extract a substring using character positions (UTF-8 safe)
pub fn substring_by_chars(s: &str, start: usize, end: usize) -> &str {
    let start_byte = char_col_to_byte_offset(s, start);
    let end_byte = char_col_to_byte_offset(s, end);
    &s[start_byte..end_byte.min(s.len())]
}

/// Check if a string is a valid Lira identifier
pub fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_identifier_start(c) => chars.all(is_identifier_char),
        _ => false,
    }
}

/// Lira keywords that cannot be used as identifiers
pub const KEYWORDS: &[&str] = &[
    "fn", "let", "var", "const", "if", "else", "while", "for", "loop", "match",
    "return", "break", "continue", "struct", "enum", "trait", "impl", "class",
    "type", "import", "use", "pub", "priv", "mut", "ref", "self", "Self",
    "true", "false", "null", "spawn", "select", "chan", "send", "recv",
    "try", "catch", "throw", "async", "await", "yield", "in", "as", "is",
];

/// Check if a string is a Lira keyword
pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

/// Lira builtin types
pub const BUILTIN_TYPES: &[&str] = &[
    "int", "int8", "int16", "int32", "int64",
    "uint", "uint8", "uint16", "uint32", "uint64",
    "float", "float32", "float64", "bool", "string", "char",
    "void", "any", "never",
];

/// Check if a string is a builtin type
pub fn is_builtin_type(s: &str) -> bool {
    BUILTIN_TYPES.contains(&s)
}

/// Lira builtin functions
pub const BUILTIN_FUNCTIONS: &[&str] = &[
    "print", "println", "len", "push", "pop", "append", "insert", "remove",
    "contains", "keys", "values", "clear", "clone", "to_string", "parse",
    "abs", "min", "max", "sqrt", "pow", "floor", "ceil", "round",
    "sin", "cos", "tan", "log", "exp", "random",
    "open", "read", "write", "close", "exists", "mkdir", "remove_file",
    "sleep", "now", "spawn", "send", "recv", "select", "chan",
    "type_of", "size_of", "assert", "panic", "error",
];

/// Check if a string is a builtin function
pub fn is_builtin_function(s: &str) -> bool {
    BUILTIN_FUNCTIONS.contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Word extraction tests
    #[test]
    fn test_get_word_at_position_simple() {
        let word = get_word_at_position("let foo = 5", 4).unwrap();
        assert_eq!(word.text, "foo");
        assert_eq!(word.start_col, 4);
        assert_eq!(word.end_col, 7);
    }

    #[test]
    fn test_get_word_at_position_start() {
        let word = get_word_at_position("hello world", 0).unwrap();
        assert_eq!(word.text, "hello");
    }

    #[test]
    fn test_get_word_at_position_end() {
        let word = get_word_at_position("hello world", 10).unwrap();
        assert_eq!(word.text, "world");
    }

    #[test]
    fn test_get_word_at_position_unicode() {
        // Test with unicode characters
        let word = get_word_at_position("let 变量 = 5", 4).unwrap();
        assert_eq!(word.text, "变量");
    }

    #[test]
    fn test_get_word_at_position_emoji_before() {
        // Emoji followed by identifier
        let word = get_word_at_position("let 🎉 foo = 5", 6).unwrap();
        assert_eq!(word.text, "foo");
    }

    #[test]
    fn test_get_word_at_position_on_space() {
        assert!(get_word_at_position("let foo = 5", 3).is_none());
    }

    #[test]
    fn test_get_word_at_position_on_operator() {
        assert!(get_word_at_position("a + b", 2).is_none());
    }

    #[test]
    fn test_get_word_at_position_empty() {
        assert!(get_word_at_position("", 0).is_none());
    }

    #[test]
    fn test_get_word_at_position_out_of_bounds() {
        assert!(get_word_at_position("hello", 10).is_none());
    }

    #[test]
    fn test_get_word_underscore() {
        let word = get_word_at_position("let _foo_bar = 5", 5).unwrap();
        assert_eq!(word.text, "_foo_bar");
    }

    // Context detection tests
    #[test]
    fn test_context_code() {
        assert_eq!(get_context_at_position("let x = 5", 4), TextContext::Code);
    }

    #[test]
    fn test_context_line_comment() {
        assert_eq!(
            get_context_at_position("let x = 5 // comment", 15),
            TextContext::LineComment
        );
    }

    #[test]
    fn test_context_string() {
        assert_eq!(
            get_context_at_position("let x = \"hello\"", 10),
            TextContext::String
        );
    }

    #[test]
    fn test_context_string_escaped_quote() {
        // Position inside string with escaped quote
        assert_eq!(
            get_context_at_position(r#"let x = "hello\"world""#, 15),
            TextContext::String
        );
    }

    #[test]
    fn test_context_after_string() {
        assert_eq!(
            get_context_at_position("let x = \"hi\" + y", 14),
            TextContext::Code
        );
    }

    #[test]
    fn test_context_char_literal() {
        assert_eq!(
            get_context_at_position("let x = 'a'", 9),
            TextContext::Char
        );
    }

    #[test]
    fn test_context_block_comment() {
        assert_eq!(
            get_context_at_position("let /* comment */ x", 8),
            TextContext::BlockComment
        );
    }

    #[test]
    fn test_context_after_block_comment() {
        assert_eq!(
            get_context_at_position("let /* comment */ x", 18),
            TextContext::Code
        );
    }

    #[test]
    fn test_is_in_string_or_comment() {
        assert!(!is_in_string_or_comment("let x = 5", 4));
        assert!(is_in_string_or_comment("let x = \"hello\"", 10));
        assert!(is_in_string_or_comment("// comment", 5));
    }

    // UTF-8 conversion tests
    #[test]
    fn test_char_col_to_byte_offset_ascii() {
        assert_eq!(char_col_to_byte_offset("hello", 2), 2);
    }

    #[test]
    fn test_char_col_to_byte_offset_unicode() {
        // "日本語" has 3 chars but 9 bytes
        assert_eq!(char_col_to_byte_offset("日本語", 1), 3);
        assert_eq!(char_col_to_byte_offset("日本語", 2), 6);
    }

    #[test]
    fn test_byte_offset_to_char_col() {
        assert_eq!(byte_offset_to_char_col("日本語", 3), 1);
        assert_eq!(byte_offset_to_char_col("日本語", 6), 2);
    }

    // Identifier validation tests
    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("foo"));
        assert!(is_valid_identifier("_bar"));
        assert!(is_valid_identifier("foo123"));
        assert!(is_valid_identifier("_"));
        assert!(!is_valid_identifier("123foo"));
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("foo-bar"));
    }

    #[test]
    fn test_is_keyword() {
        assert!(is_keyword("fn"));
        assert!(is_keyword("let"));
        assert!(is_keyword("struct"));
        assert!(!is_keyword("foo"));
        assert!(!is_keyword("Function"));
    }

    // Regex cache tests
    #[test]
    fn test_get_regex_valid() {
        let re = get_regex(r"\w+").unwrap();
        assert!(re.is_match("hello"));
    }

    #[test]
    fn test_get_regex_invalid() {
        assert!(get_regex(r"[invalid").is_none());
    }

    #[test]
    fn test_get_regex_cached() {
        let re1 = get_regex(r"\d+").unwrap();
        let re2 = get_regex(r"\d+").unwrap();
        // Both should work (cached)
        assert!(re1.is_match("123"));
        assert!(re2.is_match("456"));
    }
}
