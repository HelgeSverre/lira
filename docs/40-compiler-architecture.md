# Lira Compiler Architecture

## Document Information

| Property | Value |
|----------|-------|
| **Document ID** | 40-compiler-architecture |
| **Version** | 1.0.0-draft |
| **Status** | Draft Specification |
| **Implementation Language** | Rust |

---

## Table of Contents

1. [Overview](#1-overview)
2. [Compiler Pipeline](#2-compiler-pipeline)
3. [Lexer](#3-lexer)
4. [Parser](#4-parser)
5. [AST Structure](#5-ast-structure)
6. [Semantic Analysis](#6-semantic-analysis)
7. [Type Checking](#7-type-checking)
8. [Code Generation](#8-code-generation)
9. [Optimization](#9-optimization)
10. [Error Handling](#10-error-handling)

---

## 1. Overview

### 1.1 Compiler Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    LI-LANG COMPILER PIPELINE                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Source (.li, .liui)                                            │
│         │                                                        │
│         ▼                                                        │
│  ┌─────────────┐                                                │
│  │   LEXER     │  Tokenization                                  │
│  └─────────────┘                                                │
│         │ Tokens                                                │
│         ▼                                                        │
│  ┌─────────────┐                                                │
│  │   PARSER    │  Syntax Analysis                               │
│  └─────────────┘                                                │
│         │ AST                                                   │
│         ▼                                                        │
│  ┌─────────────┐                                                │
│  │  RESOLVER   │  Name Resolution                               │
│  └─────────────┘                                                │
│         │ Resolved AST                                          │
│         ▼                                                        │
│  ┌─────────────┐                                                │
│  │   TYPECK    │  Type Checking                                 │
│  └─────────────┘                                                │
│         │ Typed AST                                             │
│         ▼                                                        │
│  ┌─────────────┐                                                │
│  │   LOWER     │  Desugar + Simplify                           │
│  └─────────────┘                                                │
│         │ HIR                                                   │
│         ▼                                                        │
│  ┌─────────────┐                                                │
│  │  CODEGEN    │  Bytecode Generation                          │
│  └─────────────┘                                                │
│         │ Bytecode                                              │
│         ▼                                                        │
│  ┌─────────────┐                                                │
│  │  OPTIMIZE   │  Bytecode Optimization                        │
│  └─────────────┘                                                │
│         │                                                        │
│         ▼                                                        │
│  Output (.lic)                                                  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 Module Structure

```
lic/
├── src/
│   ├── main.rs           # CLI entry point
│   ├── lib.rs            # Library interface
│   ├── lexer/
│   │   ├── mod.rs
│   │   ├── token.rs
│   │   └── scanner.rs
│   ├── parser/
│   │   ├── mod.rs
│   │   ├── ast.rs
│   │   └── grammar.rs
│   ├── resolver/
│   │   ├── mod.rs
│   │   └── scope.rs
│   ├── typeck/
│   │   ├── mod.rs
│   │   ├── types.rs
│   │   └── infer.rs
│   ├── hir/
│   │   ├── mod.rs
│   │   └── lower.rs
│   ├── codegen/
│   │   ├── mod.rs
│   │   ├── bytecode.rs
│   │   └── builder.rs
│   ├── optimize/
│   │   ├── mod.rs
│   │   └── passes.rs
│   ├── diagnostics/
│   │   ├── mod.rs
│   │   └── span.rs
│   └── driver.rs         # Compilation orchestration
└── Cargo.toml
```

---

## 2. Compiler Pipeline

### 2.1 Compilation Driver

```rust
pub struct Compiler {
    /// Source files
    sources: SourceMap,

    /// Diagnostic collector
    diagnostics: DiagnosticEmitter,

    /// Compilation options
    options: CompileOptions,
}

pub struct CompileOptions {
    /// Output path
    pub output: PathBuf,

    /// Optimization level
    pub opt_level: OptLevel,

    /// Include debug info
    pub debug_info: bool,

    /// Target platform
    pub target: Target,
}

pub enum OptLevel {
    None,
    Size,
    Speed,
    Aggressive,
}

impl Compiler {
    pub fn compile(&mut self, input: &Path) -> Result<(), CompileError> {
        // Phase 1: Lexing
        let tokens = self.lex(input)?;

        // Phase 2: Parsing
        let ast = self.parse(&tokens)?;

        // Phase 3: Name Resolution
        let resolved_ast = self.resolve(ast)?;

        // Phase 4: Type Checking
        let typed_ast = self.typecheck(resolved_ast)?;

        // Phase 5: Lowering to HIR
        let hir = self.lower(typed_ast)?;

        // Phase 6: Code Generation
        let mut bytecode = self.codegen(&hir)?;

        // Phase 7: Optimization
        if self.options.opt_level != OptLevel::None {
            bytecode = self.optimize(bytecode)?;
        }

        // Phase 8: Write output
        self.emit(&bytecode)?;

        Ok(())
    }
}
```

---

## 3. Lexer

### 3.1 Token Types

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // Literals
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    CharLiteral(char),
    BoolLiteral(bool),

    // Identifiers and Keywords
    Ident(String),
    Keyword(Keyword),

    // Operators
    Plus, Minus, Star, Slash, Percent,
    PlusEq, MinusEq, StarEq, SlashEq, PercentEq,
    Eq, EqEq, BangEq,
    Lt, Le, Gt, Ge,
    And, Or, Bang,
    Ampersand, Pipe, Caret, Tilde,
    LtLt, GtGt,
    Question, QuestionDot, QuestionQuestion,
    Arrow, FatArrow,
    Dot, DotDot, DotDotEq,
    Colon, ColonColon,
    Semicolon, Comma,

    // Delimiters
    LParen, RParen,
    LBrace, RBrace,
    LBracket, RBracket,

    // Special
    Dollar, DollarLBrace,  // String interpolation
    At,                     // Decorators

    // Meta
    Newline,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Keyword {
    // Declarations
    Let, Var, Const, Fn, Class, Struct, Enum, Interface,
    Pub, Priv, Internal, Static, Abstract, Final, Override,

    // Control Flow
    If, Else, Match, While, For, In, Loop, Break, Continue, Return,

    // Types
    True, False, Null,

    // Modules
    Import, Export, Mod, Use, As, From,

    // Concurrency
    Spawn, Select, Async, Await,

    // Other
    Self_, Super, This, Try, Catch, Throw, Where,
}
```

### 3.2 Lexer Implementation

```rust
pub struct Lexer<'a> {
    /// Source code
    source: &'a str,

    /// Current position
    pos: usize,

    /// Current line
    line: u32,

    /// Current column
    column: u32,

    /// Start of current token
    start: usize,
    start_line: u32,
    start_column: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            column: 1,
            start: 0,
            start_line: 1,
            start_column: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace_and_comments();
            self.start_token();

            if self.is_at_end() {
                tokens.push(self.make_token(TokenKind::Eof));
                break;
            }

            let token = self.scan_token()?;
            tokens.push(token);
        }

        Ok(tokens)
    }

    fn scan_token(&mut self) -> Result<Token, LexError> {
        let c = self.advance();

        match c {
            // Single-character tokens
            '(' => Ok(self.make_token(TokenKind::LParen)),
            ')' => Ok(self.make_token(TokenKind::RParen)),
            '{' => Ok(self.make_token(TokenKind::LBrace)),
            '}' => Ok(self.make_token(TokenKind::RBrace)),
            '[' => Ok(self.make_token(TokenKind::LBracket)),
            ']' => Ok(self.make_token(TokenKind::RBracket)),
            ',' => Ok(self.make_token(TokenKind::Comma)),
            ';' => Ok(self.make_token(TokenKind::Semicolon)),

            // Two-character tokens
            '+' => Ok(self.make_token(
                if self.match_char('=') { TokenKind::PlusEq }
                else { TokenKind::Plus }
            )),
            '-' => Ok(self.make_token(
                if self.match_char('=') { TokenKind::MinusEq }
                else if self.match_char('>') { TokenKind::Arrow }
                else { TokenKind::Minus }
            )),
            '=' => Ok(self.make_token(
                if self.match_char('=') { TokenKind::EqEq }
                else if self.match_char('>') { TokenKind::FatArrow }
                else { TokenKind::Eq }
            )),

            // Literals
            '"' => self.string(),
            '\'' => self.character(),
            '0'..='9' => self.number(),

            // Identifiers and keywords
            'a'..='z' | 'A'..='Z' | '_' => self.identifier(),

            // String interpolation
            '$' => {
                if self.match_char('{') {
                    Ok(self.make_token(TokenKind::DollarLBrace))
                } else {
                    Ok(self.make_token(TokenKind::Dollar))
                }
            }

            _ => Err(self.error(format!("Unexpected character: '{}'", c))),
        }
    }

    fn identifier(&mut self) -> Result<Token, LexError> {
        while self.peek().is_alphanumeric() || self.peek() == '_' {
            self.advance();
        }

        let text = &self.source[self.start..self.pos];
        let kind = match text {
            "let" => TokenKind::Keyword(Keyword::Let),
            "var" => TokenKind::Keyword(Keyword::Var),
            "const" => TokenKind::Keyword(Keyword::Const),
            "fn" => TokenKind::Keyword(Keyword::Fn),
            "class" => TokenKind::Keyword(Keyword::Class),
            "if" => TokenKind::Keyword(Keyword::If),
            "else" => TokenKind::Keyword(Keyword::Else),
            "while" => TokenKind::Keyword(Keyword::While),
            "for" => TokenKind::Keyword(Keyword::For),
            "return" => TokenKind::Keyword(Keyword::Return),
            "true" => TokenKind::BoolLiteral(true),
            "false" => TokenKind::BoolLiteral(false),
            "null" => TokenKind::Keyword(Keyword::Null),
            // ... more keywords
            _ => TokenKind::Ident(text.to_string()),
        };

        Ok(self.make_token(kind))
    }

    fn number(&mut self) -> Result<Token, LexError> {
        // Integer part
        while self.peek().is_ascii_digit() {
            self.advance();
        }

        // Check for float
        if self.peek() == '.' && self.peek_next().is_ascii_digit() {
            self.advance(); // consume '.'
            while self.peek().is_ascii_digit() {
                self.advance();
            }

            let text = &self.source[self.start..self.pos];
            let value: f64 = text.parse().map_err(|_| self.error("Invalid float"))?;
            return Ok(self.make_token(TokenKind::FloatLiteral(value)));
        }

        let text = &self.source[self.start..self.pos];
        let value: i64 = text.parse().map_err(|_| self.error("Invalid integer"))?;
        Ok(self.make_token(TokenKind::IntLiteral(value)))
    }

    fn string(&mut self) -> Result<Token, LexError> {
        let mut value = String::new();

        while self.peek() != '"' && !self.is_at_end() {
            if self.peek() == '\\' {
                self.advance();
                value.push(self.escape_char()?);
            } else if self.peek() == '\n' {
                self.line += 1;
                self.column = 0;
                value.push(self.advance());
            } else {
                value.push(self.advance());
            }
        }

        if self.is_at_end() {
            return Err(self.error("Unterminated string"));
        }

        self.advance(); // closing quote
        Ok(self.make_token(TokenKind::StringLiteral(value)))
    }
}
```

---

## 4. Parser

### 4.1 Parser Structure

```rust
pub struct Parser<'a> {
    /// Token stream
    tokens: &'a [Token],

    /// Current position
    current: usize,

    /// Diagnostic emitter
    diagnostics: &'a mut DiagnosticEmitter,
}

impl<'a> Parser<'a> {
    pub fn parse(&mut self) -> Result<Module, ParseError> {
        let mut items = Vec::new();

        while !self.is_at_end() {
            items.push(self.parse_item()?);
        }

        Ok(Module { items })
    }
}
```

### 4.2 Expression Parsing (Pratt Parser)

```rust
impl<'a> Parser<'a> {
    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_precedence(Precedence::Assignment)
    }

    fn parse_precedence(&mut self, min_prec: Precedence) -> Result<Expr, ParseError> {
        // Parse prefix expression
        let mut left = self.parse_prefix()?;

        // Parse infix expressions
        while !self.is_at_end() {
            let op_prec = self.current_precedence();
            if op_prec < min_prec {
                break;
            }

            left = self.parse_infix(left, op_prec)?;
        }

        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        let token = self.advance();

        match &token.kind {
            TokenKind::IntLiteral(n) => Ok(Expr::IntLit(*n)),
            TokenKind::FloatLiteral(n) => Ok(Expr::FloatLit(*n)),
            TokenKind::StringLiteral(s) => Ok(Expr::StringLit(s.clone())),
            TokenKind::BoolLiteral(b) => Ok(Expr::BoolLit(*b)),
            TokenKind::Ident(name) => Ok(Expr::Ident(name.clone())),
            TokenKind::LParen => self.parse_grouped(),
            TokenKind::LBracket => self.parse_array_literal(),
            TokenKind::LBrace => self.parse_object_literal(),
            TokenKind::Minus => self.parse_unary(UnaryOp::Neg),
            TokenKind::Bang => self.parse_unary(UnaryOp::Not),
            TokenKind::Keyword(Keyword::If) => self.parse_if_expr(),
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expr(),
            TokenKind::Keyword(Keyword::Fn) => self.parse_lambda(),
            _ => Err(self.error("Expected expression")),
        }
    }

    fn parse_infix(&mut self, left: Expr, prec: Precedence) -> Result<Expr, ParseError> {
        let token = self.advance();

        match &token.kind {
            // Binary operators
            TokenKind::Plus => self.parse_binary(left, BinaryOp::Add, prec),
            TokenKind::Minus => self.parse_binary(left, BinaryOp::Sub, prec),
            TokenKind::Star => self.parse_binary(left, BinaryOp::Mul, prec),
            TokenKind::Slash => self.parse_binary(left, BinaryOp::Div, prec),
            TokenKind::EqEq => self.parse_binary(left, BinaryOp::Eq, prec),
            TokenKind::BangEq => self.parse_binary(left, BinaryOp::Ne, prec),
            TokenKind::Lt => self.parse_binary(left, BinaryOp::Lt, prec),
            TokenKind::Le => self.parse_binary(left, BinaryOp::Le, prec),
            TokenKind::Gt => self.parse_binary(left, BinaryOp::Gt, prec),
            TokenKind::Ge => self.parse_binary(left, BinaryOp::Ge, prec),
            TokenKind::And => self.parse_binary(left, BinaryOp::And, prec),
            TokenKind::Or => self.parse_binary(left, BinaryOp::Or, prec),

            // Postfix operators
            TokenKind::LParen => self.parse_call(left),
            TokenKind::LBracket => self.parse_index(left),
            TokenKind::Dot => self.parse_member_access(left),
            TokenKind::QuestionDot => self.parse_optional_chain(left),

            // Assignment
            TokenKind::Eq => self.parse_assignment(left),

            _ => Err(self.error("Expected operator")),
        }
    }

    fn parse_binary(
        &mut self,
        left: Expr,
        op: BinaryOp,
        prec: Precedence
    ) -> Result<Expr, ParseError> {
        let right = self.parse_precedence(prec.next())?;
        Ok(Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    None,
    Assignment,    // =
    Or,            // ||
    And,           // &&
    Equality,      // == !=
    Comparison,    // < > <= >=
    Term,          // + -
    Factor,        // * / %
    Unary,         // ! -
    Call,          // . () []
    Primary,
}
```

### 4.3 Statement Parsing

```rust
impl<'a> Parser<'a> {
    fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().kind {
            TokenKind::Keyword(Keyword::Let) => self.parse_let_stmt(),
            TokenKind::Keyword(Keyword::Var) => self.parse_var_stmt(),
            TokenKind::Keyword(Keyword::If) => self.parse_if_stmt(),
            TokenKind::Keyword(Keyword::While) => self.parse_while_stmt(),
            TokenKind::Keyword(Keyword::For) => self.parse_for_stmt(),
            TokenKind::Keyword(Keyword::Return) => self.parse_return_stmt(),
            TokenKind::Keyword(Keyword::Break) => self.parse_break_stmt(),
            TokenKind::Keyword(Keyword::Continue) => self.parse_continue_stmt(),
            TokenKind::LBrace => self.parse_block_stmt(),
            _ => self.parse_expr_stmt(),
        }
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::Keyword(Keyword::Let))?;

        let name = self.parse_identifier()?;

        let type_ann = if self.match_token(TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let init = if self.match_token(TokenKind::Eq) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(Stmt::Let { name, type_ann, init })
    }

    fn parse_function(&mut self) -> Result<FnDecl, ParseError> {
        self.expect(TokenKind::Keyword(Keyword::Fn))?;

        let name = self.parse_identifier()?;

        // Generic parameters
        let generics = if self.match_token(TokenKind::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };

        // Parameters
        self.expect(TokenKind::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(TokenKind::RParen)?;

        // Return type
        let return_type = if self.match_token(TokenKind::Arrow) {
            self.parse_type()?
        } else {
            Type::Void
        };

        // Body
        let body = self.parse_block()?;

        Ok(FnDecl {
            name,
            generics,
            params,
            return_type,
            body,
        })
    }
}
```

---

## 5. AST Structure

### 5.1 Core AST Nodes

```rust
/// Module (compilation unit)
pub struct Module {
    pub items: Vec<Item>,
}

/// Top-level items
pub enum Item {
    Import(ImportDecl),
    Function(FnDecl),
    Class(ClassDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Interface(InterfaceDecl),
    Const(ConstDecl),
}

/// Function declaration
pub struct FnDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Block,
    pub span: Span,
}

/// Class declaration
pub struct ClassDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub parent: Option<Type>,
    pub interfaces: Vec<Type>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<FnDecl>,
    pub span: Span,
}
```

### 5.2 Expression Nodes

```rust
pub enum Expr {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    CharLit(char),
    BoolLit(bool),
    NullLit,

    // Identifiers
    Ident(String),

    // Compound expressions
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    Member {
        object: Box<Expr>,
        field: Ident,
    },
    MethodCall {
        object: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },

    // Constructors
    ArrayLit(Vec<Expr>),
    ObjectLit {
        type_name: Option<Type>,
        fields: Vec<(Ident, Expr)>,
    },

    // Control flow expressions
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    // Closures
    Lambda {
        params: Vec<Param>,
        body: Box<Expr>,
    },

    // Assignment
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
}
```

### 5.3 Statement Nodes

```rust
pub enum Stmt {
    Let {
        name: Ident,
        type_ann: Option<Type>,
        init: Option<Expr>,
    },
    Expr(Expr),
    Block(Vec<Stmt>),
    If {
        condition: Expr,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    For {
        pattern: Pattern,
        iterable: Expr,
        body: Block,
    },
    Return(Option<Expr>),
    Break,
    Continue,
}
```

---

## 6. Semantic Analysis

### 6.1 Name Resolution

```rust
pub struct Resolver<'a> {
    /// Current scope stack
    scopes: Vec<Scope>,

    /// Resolved symbols
    symbols: SymbolTable,

    /// Diagnostics
    diagnostics: &'a mut DiagnosticEmitter,
}

pub struct Scope {
    /// Bindings in this scope
    bindings: HashMap<String, Symbol>,

    /// Scope kind (function, block, class, etc.)
    kind: ScopeKind,
}

pub enum ScopeKind {
    Global,
    Module,
    Function,
    Block,
    Class,
    Loop,
}

impl<'a> Resolver<'a> {
    pub fn resolve(&mut self, module: &mut Module) -> Result<(), ResolveError> {
        // First pass: collect declarations
        for item in &module.items {
            self.declare_item(item)?;
        }

        // Second pass: resolve references
        for item in &mut module.items {
            self.resolve_item(item)?;
        }

        Ok(())
    }

    fn resolve_expr(&mut self, expr: &mut Expr) -> Result<(), ResolveError> {
        match expr {
            Expr::Ident(name) => {
                // Look up in scope chain
                if let Some(symbol) = self.lookup(name) {
                    expr.resolved_symbol = Some(symbol.id);
                } else {
                    return Err(ResolveError::UndefinedVariable(name.clone()));
                }
            }
            Expr::Call { callee, args } => {
                self.resolve_expr(callee)?;
                for arg in args {
                    self.resolve_expr(arg)?;
                }
            }
            // ... handle other expression types
            _ => {}
        }
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.bindings.get(name) {
                return Some(symbol);
            }
        }
        None
    }
}
```

---

## 7. Type Checking

### 7.1 Type Representation

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    // Primitives
    Void,
    Never,
    Bool,
    Int,
    Int8, Int16, Int32, Int64,
    UInt8, UInt16, UInt32, UInt64,
    Float, Float32, Float64,
    Char,
    String,

    // Compound types
    Array(Box<Type>),
    Tuple(Vec<Type>),
    Optional(Box<Type>),
    Result(Box<Type>, Box<Type>),

    // Named types
    Named {
        name: String,
        args: Vec<Type>,
    },

    // Function types
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },

    // Type variable (for inference)
    Var(TypeVarId),

    // Error type (for error recovery)
    Error,
}
```

### 7.2 Type Checker

```rust
pub struct TypeChecker<'a> {
    /// Type environment
    env: TypeEnv,

    /// Type variable counter
    next_var_id: u32,

    /// Substitution (resolved type vars)
    substitution: HashMap<TypeVarId, Type>,

    /// Diagnostics
    diagnostics: &'a mut DiagnosticEmitter,
}

impl<'a> TypeChecker<'a> {
    pub fn check(&mut self, module: &mut Module) -> Result<(), TypeError> {
        for item in &mut module.items {
            self.check_item(item)?;
        }
        Ok(())
    }

    fn check_expr(&mut self, expr: &mut Expr, expected: Option<&Type>) -> Result<Type, TypeError> {
        let actual = match expr {
            Expr::IntLit(_) => Type::Int,
            Expr::FloatLit(_) => Type::Float,
            Expr::StringLit(_) => Type::String,
            Expr::BoolLit(_) => Type::Bool,

            Expr::Ident(name) => {
                self.env.lookup(name)
                    .ok_or_else(|| TypeError::UndefinedVariable(name.clone()))?
            }

            Expr::Binary { left, op, right } => {
                self.check_binary(left, *op, right)?
            }

            Expr::Call { callee, args } => {
                self.check_call(callee, args)?
            }

            Expr::Lambda { params, body } => {
                self.check_lambda(params, body)?
            }

            // ... other expressions
            _ => Type::Error,
        };

        // Unify with expected type if provided
        if let Some(expected) = expected {
            self.unify(&actual, expected)?;
        }

        expr.resolved_type = Some(actual.clone());
        Ok(actual)
    }

    fn unify(&mut self, a: &Type, b: &Type) -> Result<(), TypeError> {
        let a = self.resolve(a);
        let b = self.resolve(b);

        match (&a, &b) {
            // Same type
            (Type::Int, Type::Int) => Ok(()),
            (Type::Float, Type::Float) => Ok(()),
            (Type::String, Type::String) => Ok(()),
            (Type::Bool, Type::Bool) => Ok(()),

            // Type variable
            (Type::Var(id), ty) | (ty, Type::Var(id)) => {
                if !self.occurs_in(*id, ty) {
                    self.substitution.insert(*id, ty.clone());
                    Ok(())
                } else {
                    Err(TypeError::InfiniteType)
                }
            }

            // Compound types
            (Type::Array(a), Type::Array(b)) => {
                self.unify(a, b)
            }

            (Type::Function { params: p1, return_type: r1 },
             Type::Function { params: p2, return_type: r2 }) => {
                if p1.len() != p2.len() {
                    return Err(TypeError::ArityMismatch);
                }
                for (a, b) in p1.iter().zip(p2.iter()) {
                    self.unify(a, b)?;
                }
                self.unify(r1, r2)
            }

            // Mismatch
            _ => Err(TypeError::TypeMismatch(a.clone(), b.clone())),
        }
    }

    fn fresh_type_var(&mut self) -> Type {
        let id = self.next_var_id;
        self.next_var_id += 1;
        Type::Var(TypeVarId(id))
    }
}
```

---

## 8. Code Generation

### 8.1 Bytecode Builder

```rust
pub struct CodeGenerator<'a> {
    /// Current function being compiled
    function: FunctionBuilder,

    /// Constant pool
    constants: ConstantPool,

    /// Local variable table
    locals: LocalTable,

    /// Loop stack (for break/continue)
    loops: Vec<LoopContext>,

    /// Module being compiled
    module: &'a Module,
}

pub struct FunctionBuilder {
    /// Bytecode instructions
    code: Vec<u8>,

    /// Label targets
    labels: HashMap<Label, usize>,

    /// Unresolved jumps
    patches: Vec<(usize, Label)>,

    /// Max stack depth
    max_stack: u16,

    /// Current stack depth
    stack_depth: u16,
}

impl<'a> CodeGenerator<'a> {
    pub fn generate(&mut self) -> Result<CompiledModule, CodeGenError> {
        for item in &self.module.items {
            self.generate_item(item)?;
        }

        Ok(self.build_module())
    }

    fn generate_expr(&mut self, expr: &Expr) -> Result<(), CodeGenError> {
        match expr {
            Expr::IntLit(n) => {
                if *n >= -128 && *n <= 127 {
                    self.emit(Opcode::BIPUSH);
                    self.emit_i8(*n as i8);
                } else {
                    let idx = self.constants.add_int(*n);
                    self.emit_ldc(idx);
                }
            }

            Expr::FloatLit(n) => {
                let idx = self.constants.add_float(*n);
                self.emit_ldc2(idx);
            }

            Expr::StringLit(s) => {
                let idx = self.constants.add_string(s);
                self.emit_ldc(idx);
            }

            Expr::BoolLit(true) => self.emit(Opcode::CONST_TRUE),
            Expr::BoolLit(false) => self.emit(Opcode::CONST_FALSE),
            Expr::NullLit => self.emit(Opcode::CONST_NULL),

            Expr::Ident(name) => {
                let local = self.locals.get(name)?;
                self.emit_load(local.slot);
            }

            Expr::Binary { left, op, right } => {
                self.generate_expr(left)?;
                self.generate_expr(right)?;
                self.emit_binary_op(*op);
            }

            Expr::Call { callee, args } => {
                // Push arguments
                for arg in args {
                    self.generate_expr(arg)?;
                }

                // Emit call
                match callee.as_ref() {
                    Expr::Ident(name) => {
                        let func_idx = self.resolve_function(name)?;
                        self.emit(Opcode::INVOKE);
                        self.emit_u16(func_idx);
                    }
                    Expr::Member { object, field } => {
                        self.generate_expr(object)?;
                        let method_idx = self.resolve_method(field)?;
                        self.emit(Opcode::INVOKE_VIRTUAL);
                        self.emit_u16(method_idx);
                    }
                    _ => {
                        self.generate_expr(callee)?;
                        self.emit(Opcode::INVOKE_DYNAMIC);
                        self.emit_u16(args.len() as u16);
                    }
                }
            }

            Expr::If { condition, then_branch, else_branch } => {
                self.generate_expr(condition)?;

                let else_label = self.new_label();
                let end_label = self.new_label();

                self.emit(Opcode::IF_FALSE);
                self.emit_jump(else_label);

                self.generate_expr(then_branch)?;
                self.emit(Opcode::GOTO);
                self.emit_jump(end_label);

                self.place_label(else_label);
                if let Some(else_branch) = else_branch {
                    self.generate_expr(else_branch)?;
                } else {
                    self.emit(Opcode::CONST_NULL);
                }

                self.place_label(end_label);
            }

            // ... other expressions
            _ => {}
        }

        Ok(())
    }

    fn emit(&mut self, opcode: Opcode) {
        self.function.code.push(opcode as u8);
    }

    fn emit_u16(&mut self, value: u16) {
        self.function.code.push((value & 0xFF) as u8);
        self.function.code.push((value >> 8) as u8);
    }
}
```

---

## 9. Optimization

### 9.1 Optimization Passes

```rust
pub struct Optimizer {
    passes: Vec<Box<dyn OptPass>>,
}

pub trait OptPass {
    fn name(&self) -> &str;
    fn run(&mut self, code: &mut BytecodeFunction) -> bool;
}

impl Optimizer {
    pub fn new(level: OptLevel) -> Self {
        let mut passes: Vec<Box<dyn OptPass>> = Vec::new();

        match level {
            OptLevel::None => {}
            OptLevel::Size | OptLevel::Speed => {
                passes.push(Box::new(ConstantFolding));
                passes.push(Box::new(DeadCodeElimination));
                passes.push(Box::new(PeepholeOptimizer));
            }
            OptLevel::Aggressive => {
                passes.push(Box::new(ConstantFolding));
                passes.push(Box::new(DeadCodeElimination));
                passes.push(Box::new(CommonSubexprElim));
                passes.push(Box::new(PeepholeOptimizer));
                passes.push(Box::new(RegisterAllocation));
            }
        }

        Self { passes }
    }

    pub fn optimize(&mut self, code: &mut BytecodeFunction) {
        let mut changed = true;
        while changed {
            changed = false;
            for pass in &mut self.passes {
                if pass.run(code) {
                    changed = true;
                }
            }
        }
    }
}
```

### 9.2 Peephole Optimization

```rust
struct PeepholeOptimizer;

impl OptPass for PeepholeOptimizer {
    fn name(&self) -> &str { "peephole" }

    fn run(&mut self, func: &mut BytecodeFunction) -> bool {
        let mut changed = false;
        let mut i = 0;

        while i < func.code.len() {
            // Pattern: PUSH x, POP -> NOP
            if matches!(func.code.get(i..i+2), Some([op, Opcode::POP]) if is_push(*op)) {
                func.code[i] = Opcode::NOP;
                func.code[i + 1] = Opcode::NOP;
                changed = true;
            }

            // Pattern: GOTO next -> NOP
            if func.code[i] == Opcode::GOTO {
                let offset = get_offset(&func.code, i + 1);
                if offset == 3 { // jump to next instruction
                    func.code[i] = Opcode::NOP;
                    func.code[i + 1] = Opcode::NOP;
                    func.code[i + 2] = Opcode::NOP;
                    changed = true;
                }
            }

            // Pattern: CONST_I1, IADD -> IINC
            // ... more patterns

            i += 1;
        }

        changed
    }
}
```

---

## 10. Error Handling

### 10.1 Diagnostic System

```rust
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub span: Span,
    pub notes: Vec<Note>,
    pub suggestions: Vec<Suggestion>,
}

pub enum DiagnosticLevel {
    Error,
    Warning,
    Info,
    Hint,
}

pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

pub struct DiagnosticEmitter {
    diagnostics: Vec<Diagnostic>,
    has_errors: bool,
}

impl DiagnosticEmitter {
    pub fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            message: message.into(),
            span,
            notes: Vec::new(),
            suggestions: Vec::new(),
        });
        self.has_errors = true;
    }

    pub fn print_all(&self, sources: &SourceMap) {
        for diag in &self.diagnostics {
            self.print_diagnostic(diag, sources);
        }
    }

    fn print_diagnostic(&self, diag: &Diagnostic, sources: &SourceMap) {
        let file = sources.get_file(diag.span.file);
        let (line, col) = file.line_col(diag.span.start);

        // Error header
        let level_str = match diag.level {
            DiagnosticLevel::Error => "error",
            DiagnosticLevel::Warning => "warning",
            DiagnosticLevel::Info => "info",
            DiagnosticLevel::Hint => "hint",
        };

        eprintln!(
            "{}[{}]: {}",
            level_str,
            format!("{}:{}:{}", file.name(), line, col),
            diag.message
        );

        // Source snippet
        let line_content = file.get_line(line);
        eprintln!("  {} | {}", line, line_content);

        // Underline
        let underline = " ".repeat(col as usize) + &"^".repeat((diag.span.end - diag.span.start) as usize);
        eprintln!("    | {}", underline);

        // Notes and suggestions
        for note in &diag.notes {
            eprintln!("  note: {}", note.message);
        }

        for suggestion in &diag.suggestions {
            eprintln!("  suggestion: {}", suggestion.message);
        }

        eprintln!();
    }
}
```

---

## Appendix: Compiler Phases Summary

| Phase | Input | Output | Purpose |
|-------|-------|--------|---------|
| Lexer | Source text | Tokens | Tokenization |
| Parser | Tokens | AST | Syntax analysis |
| Resolver | AST | Resolved AST | Name resolution |
| TypeChecker | Resolved AST | Typed AST | Type checking |
| Lowering | Typed AST | HIR | Desugaring |
| CodeGen | HIR | Bytecode | Code generation |
| Optimizer | Bytecode | Optimized bytecode | Optimization |
| Emitter | Bytecode | .lic file | Output |

---

*This document is part of the Lira Language Specification.*
