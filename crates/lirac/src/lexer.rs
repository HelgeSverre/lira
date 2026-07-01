//! Lira Lexer
//!
//! Tokenizes Lira source code into a stream of tokens.
//! See docs/lira/01-lexical-structure.md for the full specification.

use std::iter::Peekable;
use std::str::Chars;

/// A token in the Lira source
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenKind, lexeme: String, line: usize, column: usize) -> Self {
        Self {
            kind,
            lexeme,
            line,
            column,
        }
    }
}

/// Token types
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    IntLiteral(i64),
    /// An integer whose magnitude overflows `i64` but fits `u64`. Only valid as
    /// the operand of a unary minus (yielding `i64::MIN`); the parser folds it
    /// or reports an out-of-range error.
    BigIntLiteral(u64),
    FloatLiteral(f64),
    StringLiteral(String),
    /// Interpolated string literal: `"a ${x} b"`.
    ///
    /// `parts` holds the literal text segments (with escapes already resolved)
    /// and `exprs` holds the raw source of each `${...}` embedded expression.
    /// The invariant `parts.len() == exprs.len() + 1` always holds, so the
    /// template desugars to `parts[0] + exprs[0] + parts[1] + ... + parts[n]`.
    TemplateString {
        parts: Vec<String>,
        exprs: Vec<String>,
    },
    CharLiteral(char),
    BoolLiteral(bool),
    Null,

    // Identifiers
    Identifier(String),

    // Keywords
    // Declaration keywords
    Let,
    Var,
    Const,
    Fn,
    Class,
    Struct,
    Enum,
    Interface,
    Impl,
    Trait,
    Import,
    Export,
    Mod,
    Type,
    Pub,
    Priv,
    Static,
    Use,
    Self_,    // self keyword
    SelfType, // Self type
    // Control flow keywords
    If,
    Else,
    Match,
    While,
    For,
    In,
    Loop,
    Break,
    Continue,
    Return,
    When,
    Case,
    // Type keywords
    IntType,
    FloatType,
    BoolType,
    StringType,
    CharType,
    VoidType,
    // Expression keywords
    As,
    Is,
    This,
    Super,
    // Concurrency keywords
    Spawn,
    Select,
    Send,
    Receive,
    Async,
    Await,
    // Error handling keywords
    Try,
    Catch,
    Finally,
    Throw,
    // Modifier keywords
    Abstract,
    Extends,
    Mut,
    Override,
    Where,

    // Operators
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Percent,    // %
    StarStar,   // **
    PlusPlus,   // ++
    MinusMinus, // --

    // Comparison
    EqEq,   // ==
    BangEq, // !=
    Lt,     // <
    Le,     // <=
    Gt,     // >
    Ge,     // >=

    // Logical
    AmpAmp,   // &&
    PipePipe, // ||
    Bang,     // !

    // Bitwise
    Amp,    // &
    Pipe,   // |
    Caret,  // ^
    Tilde,  // ~
    LtLt,   // <<
    GtGt,   // >>
    GtGtGt, // >>>

    // Assignment
    Eq,        // =
    PlusEq,    // +=
    MinusEq,   // -=
    StarEq,    // *=
    SlashEq,   // /=
    PercentEq, // %=
    AmpEq,     // &=
    PipeEq,    // |=
    CaretEq,   // ^=
    LtLtEq,    // <<=
    GtGtEq,    // >>=

    // Special operators
    Question,         // ?
    QuestionDot,      // ?.
    QuestionQuestion, // ??
    DotDot,           // ..
    DotDotEq,         // ..=
    Arrow,            // ->
    FatArrow,         // =>
    LtMinus,          // <-

    // Delimiters
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    Comma,      // ,
    Colon,      // :
    ColonColon, // ::
    Semicolon,  // ;
    Dot,        // .
    At,         // @
    Hash,       // #
    Dollar,     // $
    Underscore, // _

    // Special
    Eof,
    Error(String),
}

/// The lexer
pub struct Lexer<'a> {
    #[allow(dead_code)]
    source: &'a str,
    chars: Peekable<Chars<'a>>,
    line: usize,
    column: usize,
    start_line: usize,
    start_column: usize,
    current_lexeme: String,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars().peekable(),
            line: 1,
            column: 1,
            start_line: 1,
            start_column: 1,
            current_lexeme: String::new(),
        }
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        self.current_lexeme.push(ch);
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn peek_next(&self) -> Option<char> {
        let mut iter = self.chars.clone();
        iter.next();
        iter.next()
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn start_token(&mut self) {
        self.start_line = self.line;
        self.start_column = self.column;
        self.current_lexeme.clear();
    }

    fn make_token(&self, kind: TokenKind) -> Token {
        Token::new(
            kind,
            self.current_lexeme.clone(),
            self.start_line,
            self.start_column,
        )
    }

    fn error_token(&self, message: &str) -> Token {
        Token::new(
            TokenKind::Error(message.to_string()),
            self.current_lexeme.clone(),
            self.start_line,
            self.start_column,
        )
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            match ch {
                ' ' | '\t' | '\r' | '\n' => {
                    self.advance();
                }
                '/' => {
                    if self.peek_next() == Some('/') {
                        // Line comment
                        while self.peek().is_some() && self.peek() != Some('\n') {
                            self.advance();
                        }
                    } else if self.peek_next() == Some('*') {
                        // Block comment
                        self.advance(); // consume /
                        self.advance(); // consume *
                        while let Some(ch) = self.peek() {
                            if ch == '*' && self.peek_next() == Some('/') {
                                self.advance(); // consume *
                                self.advance(); // consume /
                                break;
                            }
                            self.advance();
                        }
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    fn scan_identifier(&mut self) -> Token {
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let lexeme = &self.current_lexeme;
        let kind = match lexeme.as_str() {
            // Declaration keywords
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "const" => TokenKind::Const,
            "fn" => TokenKind::Fn,
            "class" => TokenKind::Class,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            "interface" => TokenKind::Interface,
            "impl" => TokenKind::Impl,
            "trait" => TokenKind::Trait,
            "import" => TokenKind::Import,
            "export" => TokenKind::Export,
            "mod" => TokenKind::Mod,
            "type" => TokenKind::Type,
            "pub" => TokenKind::Pub,
            "priv" => TokenKind::Priv,
            "static" => TokenKind::Static,
            "use" => TokenKind::Use,
            "self" => TokenKind::Self_,
            "Self" => TokenKind::SelfType,
            // Control flow
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "match" => TokenKind::Match,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "loop" => TokenKind::Loop,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "return" => TokenKind::Return,
            "when" => TokenKind::When,
            "case" => TokenKind::Case,
            // 'default' is intentionally NOT a keyword: it lexes as an
            // identifier so `fn default()`, `let default`, `obj.default` work.
            // Type keywords
            "int" => TokenKind::IntType,
            "float" => TokenKind::FloatType,
            "bool" => TokenKind::BoolType,
            "string" => TokenKind::StringType,
            "char" => TokenKind::CharType,
            "void" => TokenKind::VoidType,
            // Expression keywords
            "as" => TokenKind::As,
            "is" => TokenKind::Is,
            "this" => TokenKind::This,
            "super" => TokenKind::Super,
            // Concurrency
            "spawn" => TokenKind::Spawn,
            "select" => TokenKind::Select,
            // NOTE: `send` is intentionally NOT a keyword. The blocking channel
            // send is exposed as the `send(channel, value)` builtin (codegen
            // emits ChanSend); reserving the word would shadow that call. The
            // select-arm send syntax uses `value -> channel`, not `send`.
            "receive" => TokenKind::Receive,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            // Error handling
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "finally" => TokenKind::Finally,
            "throw" => TokenKind::Throw,
            // Modifiers
            "abstract" => TokenKind::Abstract,
            "extends" => TokenKind::Extends,
            "mut" => TokenKind::Mut,
            "override" => TokenKind::Override,
            "where" => TokenKind::Where,
            // Literals
            "true" => TokenKind::BoolLiteral(true),
            "false" => TokenKind::BoolLiteral(false),
            "null" => TokenKind::Null,
            // Identifier
            _ => TokenKind::Identifier(lexeme.clone()),
        };

        self.make_token(kind)
    }

    fn scan_number(&mut self) -> Token {
        let first_char = self.current_lexeme.chars().next().unwrap();

        // Check for hex, octal, or binary
        if first_char == '0' {
            match self.peek() {
                Some('x') | Some('X') => return self.scan_hex(),
                Some('o') | Some('O') => return self.scan_octal(),
                Some('b') | Some('B') => return self.scan_binary(),
                _ => {}
            }
        }

        // Scan decimal part
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        // Check for float
        let mut is_float = false;
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.advance(); // consume .
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Check for exponent
        if let Some('e') | Some('E') = self.peek() {
            is_float = true;
            self.advance();
            if let Some('+') | Some('-') = self.peek() {
                self.advance();
            }
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Parse the number
        let clean: String = self.current_lexeme.chars().filter(|&c| c != '_').collect();

        if is_float {
            match clean.parse::<f64>() {
                Ok(value) => self.make_token(TokenKind::FloatLiteral(value)),
                Err(_) => self.error_token("Invalid float literal"),
            }
        } else {
            match clean.parse::<i64>() {
                Ok(value) => self.make_token(TokenKind::IntLiteral(value)),
                // Overflowed i64 as a positive value. If the magnitude still
                // fits u64 it may be the operand of a unary minus (i64::MIN);
                // defer that decision to the parser.
                Err(_) => match clean.parse::<u64>() {
                    Ok(mag) => self.make_token(TokenKind::BigIntLiteral(mag)),
                    Err(_) => self.error_token("Invalid integer literal"),
                },
            }
        }
    }

    fn scan_hex(&mut self) -> Token {
        self.advance(); // consume x/X
        while let Some(ch) = self.peek() {
            if ch.is_ascii_hexdigit() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let clean: String = self.current_lexeme[2..]
            .chars()
            .filter(|&c| c != '_')
            .collect();
        match i64::from_str_radix(&clean, 16) {
            Ok(value) => self.make_token(TokenKind::IntLiteral(value)),
            Err(_) => self.error_token("Invalid hex literal"),
        }
    }

    fn scan_octal(&mut self) -> Token {
        self.advance(); // consume o/O
        while let Some(ch) = self.peek() {
            if ch.is_digit(8) || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let clean: String = self.current_lexeme[2..]
            .chars()
            .filter(|&c| c != '_')
            .collect();
        match i64::from_str_radix(&clean, 8) {
            Ok(value) => self.make_token(TokenKind::IntLiteral(value)),
            Err(_) => self.error_token("Invalid octal literal"),
        }
    }

    fn scan_binary(&mut self) -> Token {
        self.advance(); // consume b/B
        while let Some(ch) = self.peek() {
            if ch == '0' || ch == '1' || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let clean: String = self.current_lexeme[2..]
            .chars()
            .filter(|&c| c != '_')
            .collect();
        match i64::from_str_radix(&clean, 2) {
            Ok(value) => self.make_token(TokenKind::IntLiteral(value)),
            Err(_) => self.error_token("Invalid binary literal"),
        }
    }

    fn scan_string(&mut self) -> Token {
        // Literal text segments (escapes resolved) split at each `${...}`, and
        // the raw source of each embedded interpolation expression.
        let mut parts: Vec<String> = Vec::new();
        let mut exprs: Vec<String> = Vec::new();
        let mut value = String::new();

        while let Some(ch) = self.peek() {
            if ch == '"' {
                self.advance(); // consume closing quote
                if exprs.is_empty() {
                    // No interpolation: behave exactly as a plain string literal.
                    return self.make_token(TokenKind::StringLiteral(value));
                }
                parts.push(value);
                return self.make_token(TokenKind::TemplateString { parts, exprs });
            } else if ch == '$' && self.peek_next() == Some('{') {
                // Start of an interpolation: flush the current literal segment,
                // then capture the embedded expression source up to the matching
                // closing brace (tracking nested `{}` so balanced braces work).
                self.advance(); // consume $
                self.advance(); // consume {
                parts.push(std::mem::take(&mut value));
                match self.scan_interpolation_source() {
                    Ok(src) => exprs.push(src),
                    Err(token) => return token,
                }
            } else if ch == '\\' {
                self.advance(); // consume backslash
                match self.peek() {
                    Some('n') => {
                        self.advance();
                        value.push('\n');
                    }
                    Some('r') => {
                        self.advance();
                        value.push('\r');
                    }
                    Some('t') => {
                        self.advance();
                        value.push('\t');
                    }
                    Some('\\') => {
                        self.advance();
                        value.push('\\');
                    }
                    Some('"') => {
                        self.advance();
                        value.push('"');
                    }
                    Some('\'') => {
                        self.advance();
                        value.push('\'');
                    }
                    Some('$') => {
                        // `\$` escapes interpolation: emit a literal `$`.
                        self.advance();
                        value.push('$');
                    }
                    Some('0') => {
                        self.advance();
                        value.push('\0');
                    }
                    Some('u') => {
                        self.advance(); // consume u
                        if self.match_char('{') {
                            let mut hex = String::new();
                            while let Some(c) = self.peek() {
                                if c == '}' {
                                    self.advance();
                                    break;
                                }
                                hex.push(c);
                                self.advance();
                            }
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    value.push(c);
                                }
                            }
                        }
                    }
                    _ => {
                        return self.error_token("Invalid escape sequence");
                    }
                }
            } else if ch == '\n' {
                return self.error_token("Unterminated string");
            } else {
                self.advance();
                value.push(ch);
            }
        }

        self.error_token("Unterminated string")
    }

    /// Capture the raw source of a `${...}` interpolation expression.
    ///
    /// The opening `${` has already been consumed. Collects characters up to the
    /// matching `}`, tracking nested brace depth so balanced `{}` inside the
    /// expression (e.g. struct literals) is preserved. String literals inside
    /// the expression are passed through verbatim so braces or quotes within
    /// them don't disturb the depth tracking.
    fn scan_interpolation_source(&mut self) -> Result<String, Token> {
        let mut src = String::new();
        let mut depth: usize = 0;

        while let Some(ch) = self.peek() {
            match ch {
                '}' if depth == 0 => {
                    self.advance(); // consume closing }
                    return Ok(src);
                }
                '{' => {
                    depth += 1;
                    self.advance();
                    src.push('{');
                }
                '}' => {
                    depth -= 1;
                    self.advance();
                    src.push('}');
                }
                '"' => {
                    // Pass through a nested string literal verbatim, including
                    // its escapes, so its contents don't affect brace tracking.
                    self.advance();
                    src.push('"');
                    while let Some(c) = self.peek() {
                        self.advance();
                        src.push(c);
                        if c == '\\' {
                            if let Some(next) = self.peek() {
                                self.advance();
                                src.push(next);
                            }
                        } else if c == '"' {
                            break;
                        }
                    }
                }
                '\n' => return Err(self.error_token("Unterminated string interpolation")),
                _ => {
                    self.advance();
                    src.push(ch);
                }
            }
        }

        Err(self.error_token("Unterminated string interpolation"))
    }

    fn scan_char(&mut self) -> Token {
        let ch = match self.peek() {
            Some('\\') => {
                self.advance();
                match self.peek() {
                    Some('n') => {
                        self.advance();
                        '\n'
                    }
                    Some('r') => {
                        self.advance();
                        '\r'
                    }
                    Some('t') => {
                        self.advance();
                        '\t'
                    }
                    Some('\\') => {
                        self.advance();
                        '\\'
                    }
                    Some('\'') => {
                        self.advance();
                        '\''
                    }
                    Some('"') => {
                        self.advance();
                        '"'
                    }
                    Some('0') => {
                        self.advance();
                        '\0'
                    }
                    _ => return self.error_token("Invalid escape sequence"),
                }
            }
            Some(c) if c != '\'' => {
                self.advance();
                c
            }
            _ => return self.error_token("Empty character literal"),
        };

        if self.match_char('\'') {
            self.make_token(TokenKind::CharLiteral(ch))
        } else {
            self.error_token("Unterminated character literal")
        }
    }

    pub fn scan_token(&mut self) -> Token {
        self.skip_whitespace();
        self.start_token();

        let ch = match self.advance() {
            Some(ch) => ch,
            None => return self.make_token(TokenKind::Eof),
        };

        // Identifiers and keywords
        if ch.is_alphabetic() || ch == '_' {
            return self.scan_identifier();
        }

        // Numbers
        if ch.is_ascii_digit() {
            return self.scan_number();
        }

        match ch {
            // String
            '"' => self.scan_string(),

            // Character
            '\'' => self.scan_char(),

            // Single-char tokens
            '(' => self.make_token(TokenKind::LParen),
            ')' => self.make_token(TokenKind::RParen),
            '{' => self.make_token(TokenKind::LBrace),
            '}' => self.make_token(TokenKind::RBrace),
            '[' => self.make_token(TokenKind::LBracket),
            ']' => self.make_token(TokenKind::RBracket),
            ',' => self.make_token(TokenKind::Comma),
            ';' => self.make_token(TokenKind::Semicolon),
            '@' => self.make_token(TokenKind::At),
            '#' => self.make_token(TokenKind::Hash),
            '$' => self.make_token(TokenKind::Dollar),
            '~' => self.make_token(TokenKind::Tilde),

            // Multi-char tokens
            '+' => {
                if self.match_char('+') {
                    self.make_token(TokenKind::PlusPlus)
                } else if self.match_char('=') {
                    self.make_token(TokenKind::PlusEq)
                } else {
                    self.make_token(TokenKind::Plus)
                }
            }
            '-' => {
                if self.match_char('-') {
                    self.make_token(TokenKind::MinusMinus)
                } else if self.match_char('=') {
                    self.make_token(TokenKind::MinusEq)
                } else if self.match_char('>') {
                    self.make_token(TokenKind::Arrow)
                } else {
                    self.make_token(TokenKind::Minus)
                }
            }
            '*' => {
                if self.match_char('*') {
                    self.make_token(TokenKind::StarStar)
                } else if self.match_char('=') {
                    self.make_token(TokenKind::StarEq)
                } else {
                    self.make_token(TokenKind::Star)
                }
            }
            '/' => {
                if self.match_char('=') {
                    self.make_token(TokenKind::SlashEq)
                } else {
                    self.make_token(TokenKind::Slash)
                }
            }
            '%' => {
                if self.match_char('=') {
                    self.make_token(TokenKind::PercentEq)
                } else {
                    self.make_token(TokenKind::Percent)
                }
            }
            '=' => {
                if self.match_char('=') {
                    self.make_token(TokenKind::EqEq)
                } else if self.match_char('>') {
                    self.make_token(TokenKind::FatArrow)
                } else {
                    self.make_token(TokenKind::Eq)
                }
            }
            '!' => {
                if self.match_char('=') {
                    self.make_token(TokenKind::BangEq)
                } else {
                    self.make_token(TokenKind::Bang)
                }
            }
            '<' => {
                if self.match_char('<') {
                    if self.match_char('=') {
                        self.make_token(TokenKind::LtLtEq)
                    } else {
                        self.make_token(TokenKind::LtLt)
                    }
                } else if self.match_char('=') {
                    self.make_token(TokenKind::Le)
                } else if self.match_char('-') {
                    self.make_token(TokenKind::LtMinus)
                } else {
                    self.make_token(TokenKind::Lt)
                }
            }
            '>' => {
                if self.match_char('>') {
                    if self.match_char('>') {
                        self.make_token(TokenKind::GtGtGt)
                    } else if self.match_char('=') {
                        self.make_token(TokenKind::GtGtEq)
                    } else {
                        self.make_token(TokenKind::GtGt)
                    }
                } else if self.match_char('=') {
                    self.make_token(TokenKind::Ge)
                } else {
                    self.make_token(TokenKind::Gt)
                }
            }
            '&' => {
                if self.match_char('&') {
                    self.make_token(TokenKind::AmpAmp)
                } else if self.match_char('=') {
                    self.make_token(TokenKind::AmpEq)
                } else {
                    self.make_token(TokenKind::Amp)
                }
            }
            '|' => {
                if self.match_char('|') {
                    self.make_token(TokenKind::PipePipe)
                } else if self.match_char('=') {
                    self.make_token(TokenKind::PipeEq)
                } else {
                    self.make_token(TokenKind::Pipe)
                }
            }
            '^' => {
                if self.match_char('=') {
                    self.make_token(TokenKind::CaretEq)
                } else {
                    self.make_token(TokenKind::Caret)
                }
            }
            '.' => {
                if self.match_char('.') {
                    if self.match_char('=') {
                        self.make_token(TokenKind::DotDotEq)
                    } else {
                        self.make_token(TokenKind::DotDot)
                    }
                } else {
                    self.make_token(TokenKind::Dot)
                }
            }
            ':' => {
                if self.match_char(':') {
                    self.make_token(TokenKind::ColonColon)
                } else {
                    self.make_token(TokenKind::Colon)
                }
            }
            '?' => {
                if self.match_char('.') {
                    self.make_token(TokenKind::QuestionDot)
                } else if self.match_char('?') {
                    self.make_token(TokenKind::QuestionQuestion)
                } else {
                    self.make_token(TokenKind::Question)
                }
            }

            _ => self.error_token(&format!("Unexpected character: {}", ch)),
        }
    }
}

/// Tokenize Lira source code
pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    loop {
        let token = lexer.scan_token();
        match &token.kind {
            TokenKind::Eof => {
                tokens.push(token);
                break;
            }
            TokenKind::Error(msg) => {
                errors.push(format!("{}:{}: {}", token.line, token.column, msg));
            }
            _ => {
                tokens.push(token);
            }
        }
    }

    if errors.is_empty() {
        Ok(tokens)
    } else {
        Err(errors.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords() {
        let tokens = tokenize("let var fn if else while for return").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Let));
        assert!(matches!(tokens[1].kind, TokenKind::Var));
        assert!(matches!(tokens[2].kind, TokenKind::Fn));
        assert!(matches!(tokens[3].kind, TokenKind::If));
        assert!(matches!(tokens[4].kind, TokenKind::Else));
        assert!(matches!(tokens[5].kind, TokenKind::While));
        assert!(matches!(tokens[6].kind, TokenKind::For));
        assert!(matches!(tokens[7].kind, TokenKind::Return));
    }

    #[test]
    fn test_numbers() {
        let tokens = tokenize("42 2.5 0xFF 0b1010 1_000_000").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::IntLiteral(42)));
        assert!(matches!(tokens[1].kind, TokenKind::FloatLiteral(f) if (f - 2.5).abs() < 0.001));
        assert!(matches!(tokens[2].kind, TokenKind::IntLiteral(255)));
        assert!(matches!(tokens[3].kind, TokenKind::IntLiteral(10)));
        assert!(matches!(tokens[4].kind, TokenKind::IntLiteral(1_000_000)));
    }

    #[test]
    fn test_strings() {
        let tokens = tokenize(r#""hello" "world\n""#).unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::StringLiteral(s) if s == "hello"));
        assert!(matches!(&tokens[1].kind, TokenKind::StringLiteral(s) if s == "world\n"));
    }

    #[test]
    fn test_template_string() {
        let tokens = tokenize(r#""a ${x} b ${1 + 2} c""#).unwrap();
        match &tokens[0].kind {
            TokenKind::TemplateString { parts, exprs } => {
                assert_eq!(
                    parts,
                    &vec!["a ".to_string(), " b ".to_string(), " c".to_string()]
                );
                assert_eq!(exprs, &vec!["x".to_string(), "1 + 2".to_string()]);
            }
            other => panic!("expected TemplateString, got {other:?}"),
        }
    }

    #[test]
    fn test_template_string_leading_and_trailing() {
        let tokens = tokenize(r#""${x} more""#).unwrap();
        match &tokens[0].kind {
            TokenKind::TemplateString { parts, exprs } => {
                assert_eq!(parts, &vec!["".to_string(), " more".to_string()]);
                assert_eq!(exprs, &vec!["x".to_string()]);
            }
            other => panic!("expected TemplateString, got {other:?}"),
        }
    }

    #[test]
    fn test_escaped_interpolation_is_plain_string() {
        // `\${x}` is a literal and a lone `$` stays literal: no TemplateString.
        let tokens = tokenize(r#""literal \${x} costs $5""#).unwrap();
        assert!(
            matches!(&tokens[0].kind, TokenKind::StringLiteral(s) if s == "literal ${x} costs $5"),
            "got {:?}",
            tokens[0].kind
        );
    }

    #[test]
    fn test_plain_string_unaffected() {
        // A string with no `${...}` is still a plain StringLiteral.
        let tokens = tokenize(r#""hello world""#).unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::StringLiteral(s) if s == "hello world"));
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("+ - * / == != <= >= && || ->").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Plus));
        assert!(matches!(tokens[1].kind, TokenKind::Minus));
        assert!(matches!(tokens[2].kind, TokenKind::Star));
        assert!(matches!(tokens[3].kind, TokenKind::Slash));
        assert!(matches!(tokens[4].kind, TokenKind::EqEq));
        assert!(matches!(tokens[5].kind, TokenKind::BangEq));
        assert!(matches!(tokens[6].kind, TokenKind::Le));
        assert!(matches!(tokens[7].kind, TokenKind::Ge));
        assert!(matches!(tokens[8].kind, TokenKind::AmpAmp));
        assert!(matches!(tokens[9].kind, TokenKind::PipePipe));
        assert!(matches!(tokens[10].kind, TokenKind::Arrow));
    }

    #[test]
    fn test_comments() {
        let tokens = tokenize("// comment\nlet x = 1 /* block */ + 2").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Let));
        assert!(matches!(&tokens[1].kind, TokenKind::Identifier(s) if s == "x"));
        assert!(matches!(tokens[2].kind, TokenKind::Eq));
        assert!(matches!(tokens[3].kind, TokenKind::IntLiteral(1)));
        assert!(matches!(tokens[4].kind, TokenKind::Plus));
        assert!(matches!(tokens[5].kind, TokenKind::IntLiteral(2)));
    }

    #[test]
    fn test_double_colon() {
        let tokens = tokenize("Color::Red").unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Identifier(s) if s == "Color"));
        assert!(matches!(tokens[1].kind, TokenKind::ColonColon));
        assert!(matches!(&tokens[2].kind, TokenKind::Identifier(s) if s == "Red"));
    }

    #[test]
    fn test_compound_operators() {
        let tokens = tokenize("+= -= *= /= %= &= |= ^= <<= >>=").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::PlusEq));
        assert!(matches!(tokens[1].kind, TokenKind::MinusEq));
        assert!(matches!(tokens[2].kind, TokenKind::StarEq));
        assert!(matches!(tokens[3].kind, TokenKind::SlashEq));
        assert!(matches!(tokens[4].kind, TokenKind::PercentEq));
        assert!(matches!(tokens[5].kind, TokenKind::AmpEq));
        assert!(matches!(tokens[6].kind, TokenKind::PipeEq));
        assert!(matches!(tokens[7].kind, TokenKind::CaretEq));
        assert!(matches!(tokens[8].kind, TokenKind::LtLtEq));
        assert!(matches!(tokens[9].kind, TokenKind::GtGtEq));
    }

    #[test]
    fn test_increment_decrement_tokens() {
        let tokens = tokenize("++ -- ??").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::PlusPlus));
        assert!(matches!(tokens[1].kind, TokenKind::MinusMinus));
        assert!(matches!(tokens[2].kind, TokenKind::QuestionQuestion));
    }

    #[test]
    fn test_import_keyword() {
        let tokens = tokenize("import std.fs").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Import));
        assert!(matches!(&tokens[1].kind, TokenKind::Identifier(s) if s == "std"));
        assert!(matches!(tokens[2].kind, TokenKind::Dot));
        assert!(matches!(&tokens[3].kind, TokenKind::Identifier(s) if s == "fs"));
    }

    #[test]
    fn test_bitwise_operators() {
        let tokens = tokenize("& | ^ ~ << >>").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Amp));
        assert!(matches!(tokens[1].kind, TokenKind::Pipe));
        assert!(matches!(tokens[2].kind, TokenKind::Caret));
        assert!(matches!(tokens[3].kind, TokenKind::Tilde));
        assert!(matches!(tokens[4].kind, TokenKind::LtLt));
        assert!(matches!(tokens[5].kind, TokenKind::GtGt));
    }

    #[test]
    fn test_hex_and_binary_literals() {
        let tokens = tokenize("0xFF 0b1010 0o777").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::IntLiteral(255)));
        assert!(matches!(tokens[1].kind, TokenKind::IntLiteral(10)));
        assert!(matches!(tokens[2].kind, TokenKind::IntLiteral(511)));
    }

    #[test]
    fn test_trait_keyword() {
        let tokens = tokenize("trait Display").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Trait));
        assert!(matches!(&tokens[1].kind, TokenKind::Identifier(s) if s == "Display"));
    }

    #[test]
    fn test_self_keywords() {
        let tokens = tokenize("self Self").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Self_));
        assert!(matches!(tokens[1].kind, TokenKind::SelfType));
    }

    #[test]
    fn test_use_keyword() {
        let tokens = tokenize("use std::fs").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Use));
        assert!(matches!(&tokens[1].kind, TokenKind::Identifier(s) if s == "std"));
        assert!(matches!(tokens[2].kind, TokenKind::ColonColon));
        assert!(matches!(&tokens[3].kind, TokenKind::Identifier(s) if s == "fs"));
    }

    #[test]
    fn test_impl_block_tokens() {
        let tokens = tokenize("impl File { fn read(self) }").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Impl));
        assert!(matches!(&tokens[1].kind, TokenKind::Identifier(s) if s == "File"));
        assert!(matches!(tokens[2].kind, TokenKind::LBrace));
        assert!(matches!(tokens[3].kind, TokenKind::Fn));
        assert!(matches!(&tokens[4].kind, TokenKind::Identifier(s) if s == "read"));
        assert!(matches!(tokens[5].kind, TokenKind::LParen));
        assert!(matches!(tokens[6].kind, TokenKind::Self_));
        assert!(matches!(tokens[7].kind, TokenKind::RParen));
        assert!(matches!(tokens[8].kind, TokenKind::RBrace));
    }

    #[test]
    fn test_path_expression_tokens() {
        let tokens = tokenize("std::fs::read").unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Identifier(s) if s == "std"));
        assert!(matches!(tokens[1].kind, TokenKind::ColonColon));
        assert!(matches!(&tokens[2].kind, TokenKind::Identifier(s) if s == "fs"));
        assert!(matches!(tokens[3].kind, TokenKind::ColonColon));
        assert!(matches!(&tokens[4].kind, TokenKind::Identifier(s) if s == "read"));
    }

    #[test]
    fn test_question_mark_operator() {
        let tokens = tokenize("file.read()?").unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Identifier(s) if s == "file"));
        assert!(matches!(tokens[1].kind, TokenKind::Dot));
        assert!(matches!(&tokens[2].kind, TokenKind::Identifier(s) if s == "read"));
        assert!(matches!(tokens[3].kind, TokenKind::LParen));
        assert!(matches!(tokens[4].kind, TokenKind::RParen));
        assert!(matches!(tokens[5].kind, TokenKind::Question));
    }
}
