//! Lira Abstract Syntax Tree
//!
//! Defines AST node types for the Lira parser.
//! See docs/lira/03-syntax-constructs.md for the full specification.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::ids::NodeId;
use crate::lexer::Token;

/// Source location information
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

impl From<&Token> for Span {
    fn from(token: &Token) -> Self {
        Span {
            line: token.line,
            column: token.column,
        }
    }
}

/// Generic type parameter (e.g., T in fn identity<T>)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TypeParam {
    pub name: String,
    pub bounds: Vec<String>, // Trait bounds (e.g., T: Display)
}

/// A complete Lira program
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Program {
    pub statements: Vec<Statement>,
}

/// Top-level and block statements
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Statement {
    pub id: NodeId,
    pub kind: StatementKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum StatementKind {
    /// Variable declaration: let x = expr or var x = expr
    /// Also supports destructuring: let (a, b) = tuple, let { x, y } = struct
    VarDecl {
        pattern: Pattern,
        mutable: bool,
        type_ann: Option<TypeExpr>,
        initializer: Option<Expression>,
    },
    /// Constant declaration
    ConstDecl {
        name: String,
        type_ann: Option<TypeExpr>,
        initializer: Expression,
    },
    /// Function declaration
    FnDecl {
        name: String,
        type_params: Vec<TypeParam>,
        params: Vec<Parameter>,
        return_type: Option<TypeExpr>,
        body: Block,
        is_public: bool,
        is_override: bool,
    },
    /// Class declaration
    ClassDecl {
        name: String,
        parent: Option<String>,
        interfaces: Vec<String>,
        fields: Vec<Field>,
        methods: Vec<Statement>,
    },
    /// Struct declaration
    StructDecl {
        name: String,
        type_params: Vec<TypeParam>,
        fields: Vec<Field>,
        methods: Vec<Statement>,
    },
    /// Enum declaration
    EnumDecl {
        name: String,
        variants: Vec<EnumVariant>,
    },
    /// Interface declaration
    InterfaceDecl {
        name: String,
        methods: Vec<InterfaceMethod>,
    },
    /// Type alias
    TypeAlias { name: String, type_expr: TypeExpr },
    /// Expression statement
    Expression(Expression),
    /// Return statement
    Return(Option<Expression>),
    /// If statement
    If {
        condition: Expression,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    /// While loop
    While { condition: Expression, body: Block },
    /// For loop
    For {
        variable: String,
        iterable: Expression,
        body: Block,
    },
    /// Loop (infinite)
    Loop { body: Block },
    /// Break statement
    Break(Option<Expression>),
    /// Continue statement
    Continue,
    /// Import statement (legacy: import std.fs)
    Import {
        path: Vec<String>,
        items: Option<Vec<String>>,
    },
    /// Use statement (new: use std::fs)
    Use {
        path: Vec<String>,
        alias: Option<String>,
        items: Option<Vec<String>>,
    },
    /// Trait declaration
    TraitDecl {
        name: String,
        type_params: Vec<TypeParam>,
        methods: Vec<TraitMethod>,
        is_public: bool,
    },
    /// Impl block
    ImplDecl {
        trait_name: Option<String>,
        type_name: String,
        type_params: Vec<TypeParam>,
        methods: Vec<Statement>,
    },
    /// Block statement
    Block(Block),
}

/// A block of statements
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

/// Function parameter
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Parameter {
    pub id: NodeId,
    pub name: String,
    pub type_ann: TypeExpr,
    pub default: Option<Expression>,
    pub span: Span,
}

/// Struct/class field
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Field {
    pub name: String,
    pub type_ann: TypeExpr,
    pub is_public: bool,
    pub is_mutable: bool,
    pub span: Span,
}

/// Enum variant
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<TypeExpr>,
    pub span: Span,
}

/// Interface method signature
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InterfaceMethod {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeExpr>,
    pub span: Span,
}

/// Trait method signature (with optional default implementation)
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeExpr>,
    pub has_self: bool,
    pub default_impl: Option<Block>,
    pub span: Span,
}

/// Type expression
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum TypeExprKind {
    /// Simple type name
    Named(String),
    /// Generic type: List<T>
    Generic { name: String, args: Vec<TypeExpr> },
    /// Optional type: T?
    Optional(Box<TypeExpr>),
    /// Function type: fn(A, B) -> C
    Function {
        params: Vec<TypeExpr>,
        return_type: Box<TypeExpr>,
    },
    /// Tuple type: (A, B, C)
    Tuple(Vec<TypeExpr>),
    /// Array type: [T]
    Array(Box<TypeExpr>),
    /// Result type: Result<T, E>
    Result {
        ok_type: Box<TypeExpr>,
        err_type: Box<TypeExpr>,
    },
    /// Path type: std::fs::File
    Path(Vec<String>),
    /// Inferred type — used for un-annotated function parameters.
    /// Resolves to `Type::Any` in the checker (the VM is dynamically typed).
    Infer,
}

/// Expressions
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Expression {
    pub id: NodeId,
    pub kind: ExpressionKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum ExpressionKind {
    /// Integer literal
    IntLiteral(i64),
    /// Float literal
    FloatLiteral(f64),
    /// String literal
    StringLiteral(String),
    /// Character literal
    CharLiteral(char),
    /// Boolean literal
    BoolLiteral(bool),
    /// Null literal
    Null,
    /// Identifier
    Identifier(String),
    /// Binary operation
    Binary {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    /// Unary operation
    Unary {
        op: UnaryOp,
        operand: Box<Expression>,
    },
    /// Function call (supports named arguments and explicit type arguments)
    Call {
        callee: Box<Expression>,
        /// Explicit type arguments for generic functions (e.g., foo::<int, string>())
        type_args: Vec<TypeExpr>,
        args: Vec<Argument>,
    },
    /// Field access
    FieldAccess {
        object: Box<Expression>,
        field: String,
    },
    /// Optional chaining field access
    OptionalAccess {
        object: Box<Expression>,
        field: String,
    },
    /// Index access
    Index {
        object: Box<Expression>,
        index: Box<Expression>,
    },
    /// Array literal
    Array(Vec<Expression>),
    /// Map literal
    Map(Vec<(Expression, Expression)>),
    /// Tuple literal
    Tuple(Vec<Expression>),
    /// Struct/object literal
    StructLiteral {
        name: Option<String>,
        fields: Vec<(String, Expression)>,
    },
    /// Lambda expression
    Lambda {
        params: Vec<Parameter>,
        body: Box<Expression>,
    },
    /// If expression
    IfExpr {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
    /// Match expression
    Match {
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
    },
    /// Range expression
    Range {
        start: Option<Box<Expression>>,
        end: Option<Box<Expression>>,
        inclusive: bool,
    },
    /// Type cast
    Cast {
        expr: Box<Expression>,
        type_expr: TypeExpr,
    },
    /// Type check
    TypeCheck {
        expr: Box<Expression>,
        type_expr: TypeExpr,
    },
    /// Assignment
    Assign {
        target: Box<Expression>,
        value: Box<Expression>,
    },
    /// Compound assignment (+=, -=, etc.)
    CompoundAssign {
        target: Box<Expression>,
        op: BinaryOp,
        value: Box<Expression>,
    },
    /// Block expression
    Block(Block),
    /// Spawn expression
    Spawn(Box<Expression>),
    /// Select expression for multiple channel wait
    Select(Vec<SelectArm>),
    /// Enum variant access (Color::Red)
    EnumVariant {
        enum_name: String,
        variant_name: String,
    },
    /// Path expression (std::fs::read)
    Path { segments: Vec<String> },
    /// Try expression (expr?)
    Try(Box<Expression>),
    /// Method call with implicit self (receiver.method(args))
    MethodCall {
        receiver: Box<Expression>,
        method: String,
        /// Explicit type arguments for generic methods (e.g., obj.method::<int>())
        type_args: Vec<TypeExpr>,
        args: Vec<Argument>,
    },
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    // Null coalescing
    NullCoalesce,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    PreInc,  // ++x
    PreDec,  // --x
    PostInc, // x++
    PostDec, // x--
}

/// Match arm
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expression>,
    pub body: Expression,
    pub span: Span,
}

/// Function call argument (supports named arguments)
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Argument {
    /// Optional parameter name for named arguments (e.g., `name:` in `foo(name: "bar")`)
    pub name: Option<String>,
    /// The argument value expression
    pub value: Expression,
    pub span: Span,
}

/// Select arm for channel select statement
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SelectArm {
    pub kind: SelectArmKind,
    pub body: Expression,
    pub span: Span,
}

/// Type of select arm
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
pub enum SelectArmKind {
    /// Receive from channel: variable = <-channel
    Recv {
        variable: Option<String>,
        channel: Expression,
    },
    /// Send to channel: value -> channel
    Send {
        value: Expression,
        channel: Expression,
    },
    /// Default case (non-blocking)
    Default,
}

/// Pattern for match expressions
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Pattern {
    pub id: NodeId,
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum PatternKind {
    /// Wildcard pattern: _
    Wildcard,
    /// Literal pattern
    Literal(Expression),
    /// Variable binding
    Variable(String),
    /// Tuple pattern
    Tuple(Vec<Pattern>),
    /// Struct pattern
    Struct {
        name: String,
        fields: Vec<(String, Pattern)>,
        rest: bool,
    },
    /// Constructor/enum variant pattern: Some(x)
    Constructor { name: String, fields: Vec<Pattern> },
    /// Range pattern
    Range {
        start: Box<Pattern>,
        end: Box<Pattern>,
        inclusive: bool,
    },
    /// Or pattern
    Or(Vec<Pattern>),
    /// Binding pattern with @
    Binding { name: String, pattern: Box<Pattern> },
}
