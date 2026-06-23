use crate::ast::Span;
use crate::checker::Type;
use std::fmt;

/// Where a type-mismatch was detected. Distinguishes the many
/// context-specific "expected X got Y" / "must be bool/integer" checks
/// so consumers (LSP quick-fixes) can branch on the situation.
#[derive(Debug, Clone)]
pub enum TypeContext {
    /// `let`/`const` initializer does not match the declared type.
    VarInit,
    /// `return` value does not match the function's return type.
    Return,
    /// Assignment value does not match the target's type.
    Assignment,
    /// Call argument does not match the parameter's type.
    Argument,
    /// Array element does not match the inferred element type.
    ArrayElement,
    /// Match arm value does not match the first arm's type.
    MatchArm,
    /// Default parameter value does not match the parameter's type.
    DefaultValue { param: String },
}

/// Which integer-required position triggered a "must be integer" error.
#[derive(Debug, Clone)]
pub enum IntContext {
    ArrayIndex,
    StringIndex,
    RangeStart,
    RangeEnd,
}

/// Structured error from the type checker
#[derive(Debug, Clone)]
pub enum CheckerError {
    TypeMismatch {
        expected: Type,
        got: Type,
        span: Span,
    },
    UndefinedVariable {
        name: String,
        span: Span,
    },
    UnknownMethod {
        method: String,
        on_type: String,
        span: Span,
    },
    UnknownField {
        field: String,
        on_type: String,
        span: Span,
    },
    UnknownEnumVariant {
        variant: String,
        enum_name: String,
        span: Span,
    },
    NotAnEnum {
        type_name: String,
        span: Span,
    },
    UnknownType {
        name: String,
        span: Span,
    },
    ArgumentCountMismatch {
        expected: usize,
        got: usize,
        span: Span,
    },
    NotCallable {
        ty: Type,
        span: Span,
    },
    ImmutableAssignment {
        name: String,
        span: Span,
    },
    BreakOutsideLoop {
        span: Span,
    },
    ContinueOutsideLoop {
        span: Span,
    },
    TraitNotImplemented {
        trait_name: String,
        type_name: String,
        span: Span,
    },
    MissingSelfParameter {
        method: String,
        span: Span,
    },
    /// Context-specific "expected/got" type mismatch carrying both Types.
    TypeMismatchContext {
        context: TypeContext,
        expected: Type,
        got: Type,
        span: Span,
    },
    /// A condition expression must be bool. `got` is present only where the
    /// original message included it (if/while statements); `None` for the
    /// `if`-expression form.
    ConditionMustBeBool {
        got: Option<Type>,
        span: Span,
    },
    /// An operand/index must be an integer (no Type in original text).
    MustBeInteger {
        context: IntContext,
        span: Span,
    },
    /// An operand must be bool (logical not / logical operators).
    RequiresBoolOperand {
        /// `true` => "Logical operators require bool operands",
        /// `false` => "Logical not requires bool operand".
        plural: bool,
        span: Span,
    },
    /// An operand must be an integer (bitwise not / bitwise operators).
    RequiresIntegerOperand {
        /// `true` => "Bitwise operators require integer operands",
        /// `false` => "Bitwise not requires integer operand".
        plural: bool,
        span: Span,
    },
    /// Map literal key/value type mismatch (no Types in original text).
    MapEntryTypeMismatch {
        /// `false` => "Map key type mismatch", `true` => "Map value type mismatch".
        value: bool,
        span: Span,
    },
    /// Compound assignment operand types do not match.
    CompoundAssignmentTypeMismatch {
        span: Span,
    },
    /// Calling a non-function value. Carries the Type (display_name in text).
    NotAFunction {
        ty: Type,
        span: Span,
    },
    /// Indexing a non-indexable Type. Carries the Type.
    CannotIndex {
        ty: Type,
        span: Span,
    },
    /// Field access on a non-aggregate Type. Carries the Type.
    CannotAccessField {
        ty: Type,
        span: Span,
    },
    /// Unknown enum name in a path expression.
    UnknownEnum {
        name: String,
        span: Span,
    },
    GenericError {
        message: String,
        span: Span,
    },
}

impl CheckerError {
    /// Get the source span for this error
    pub fn span(&self) -> &Span {
        match self {
            Self::TypeMismatch { span, .. }
            | Self::UndefinedVariable { span, .. }
            | Self::UnknownMethod { span, .. }
            | Self::UnknownField { span, .. }
            | Self::UnknownEnumVariant { span, .. }
            | Self::NotAnEnum { span, .. }
            | Self::UnknownType { span, .. }
            | Self::ArgumentCountMismatch { span, .. }
            | Self::NotCallable { span, .. }
            | Self::ImmutableAssignment { span, .. }
            | Self::BreakOutsideLoop { span }
            | Self::ContinueOutsideLoop { span }
            | Self::TraitNotImplemented { span, .. }
            | Self::MissingSelfParameter { span, .. }
            | Self::TypeMismatchContext { span, .. }
            | Self::ConditionMustBeBool { span, .. }
            | Self::MustBeInteger { span, .. }
            | Self::RequiresBoolOperand { span, .. }
            | Self::RequiresIntegerOperand { span, .. }
            | Self::MapEntryTypeMismatch { span, .. }
            | Self::CompoundAssignmentTypeMismatch { span }
            | Self::NotAFunction { span, .. }
            | Self::CannotIndex { span, .. }
            | Self::CannotAccessField { span, .. }
            | Self::UnknownEnum { span, .. }
            | Self::GenericError { span, .. } => span,
        }
    }

    /// Format as a human-readable error message with location
    pub fn message(&self) -> String {
        match self {
            Self::TypeMismatch {
                expected,
                got,
                span,
            } => {
                format!(
                    "{}:{}: Type mismatch: expected '{}', got '{}'",
                    span.line,
                    span.column,
                    expected.display_name(),
                    got.display_name()
                )
            }
            Self::UndefinedVariable { name, span } => {
                format!(
                    "{}:{}: Undefined variable: {}",
                    span.line, span.column, name
                )
            }
            Self::UnknownMethod {
                method,
                on_type,
                span,
            } => {
                format!(
                    "{}:{}: Unknown method: {} on type {}",
                    span.line, span.column, method, on_type
                )
            }
            Self::UnknownField {
                field,
                on_type,
                span,
            } => {
                format!(
                    "{}:{}: Unknown field: {} on type {}",
                    span.line, span.column, field, on_type
                )
            }
            Self::UnknownEnumVariant {
                variant,
                enum_name,
                span,
            } => {
                format!(
                    "{}:{}: Unknown variant '{}' for enum '{}'",
                    span.line, span.column, variant, enum_name
                )
            }
            Self::NotAnEnum { type_name, span } => {
                format!(
                    "{}:{}: '{}' is not an enum",
                    span.line, span.column, type_name
                )
            }
            Self::UnknownType { name, span } => {
                format!("{}:{}: Unknown type: {}", span.line, span.column, name)
            }
            Self::ArgumentCountMismatch {
                expected,
                got,
                span,
            } => {
                format!(
                    "{}:{}: Expected {} arguments, got {}",
                    span.line, span.column, expected, got
                )
            }
            Self::NotCallable { ty, span } => {
                format!(
                    "{}:{}: Type '{}' is not callable",
                    span.line,
                    span.column,
                    ty.display_name()
                )
            }
            Self::ImmutableAssignment { name, span } => {
                format!(
                    "{}:{}: Cannot assign to immutable variable '{}'",
                    span.line, span.column, name
                )
            }
            Self::BreakOutsideLoop { span } => {
                format!("{}:{}: break outside of loop", span.line, span.column)
            }
            Self::ContinueOutsideLoop { span } => {
                format!("{}:{}: continue outside of loop", span.line, span.column)
            }
            Self::TraitNotImplemented {
                trait_name,
                type_name,
                span,
            } => {
                format!(
                    "{}:{}: Trait '{}' is not implemented for type '{}'",
                    span.line, span.column, trait_name, type_name
                )
            }
            Self::MissingSelfParameter { method, span } => {
                format!(
                    "{}:{}: Method '{}' must have 'self' as first parameter",
                    span.line, span.column, method
                )
            }
            Self::TypeMismatchContext {
                context,
                expected,
                got,
                span,
            } => {
                let e = expected.display_name();
                let g = got.display_name();
                match context {
                    TypeContext::VarInit => format!(
                        "{}:{}: Type mismatch: expected '{}', got '{}'",
                        span.line, span.column, e, g
                    ),
                    TypeContext::Return => format!(
                        "{}:{}: Return type mismatch: expected '{}', got '{}'",
                        span.line, span.column, e, g
                    ),
                    TypeContext::Assignment => format!(
                        "{}:{}: Assignment type mismatch: expected '{}', got '{}'",
                        span.line, span.column, e, g
                    ),
                    TypeContext::Argument => format!(
                        "{}:{}: Argument type mismatch: expected '{}', got '{}'",
                        span.line, span.column, e, g
                    ),
                    TypeContext::ArrayElement => format!(
                        "{}:{}: Array element type mismatch: expected '{}', got '{}'",
                        span.line, span.column, e, g
                    ),
                    TypeContext::MatchArm => format!(
                        "{}:{}: Match arm type mismatch: expected '{}', got '{}'",
                        span.line, span.column, e, g
                    ),
                    TypeContext::DefaultValue { param } => format!(
                        "{}:{}: Default value type mismatch: parameter '{}' expects '{}', got '{}'",
                        span.line, span.column, param, e, g
                    ),
                }
            }
            Self::ConditionMustBeBool { got, span } => match got {
                Some(t) => format!(
                    "{}:{}: Condition must be bool, got '{}'",
                    span.line,
                    span.column,
                    t.display_name()
                ),
                None => format!("{}:{}: If condition must be bool", span.line, span.column),
            },
            Self::MustBeInteger { context, span } => {
                let what = match context {
                    IntContext::ArrayIndex => "Array index",
                    IntContext::StringIndex => "String index",
                    IntContext::RangeStart => "Range start",
                    IntContext::RangeEnd => "Range end",
                };
                format!("{}:{}: {} must be integer", span.line, span.column, what)
            }
            Self::RequiresBoolOperand { plural, span } => {
                if *plural {
                    format!(
                        "{}:{}: Logical operators require bool operands",
                        span.line, span.column
                    )
                } else {
                    format!(
                        "{}:{}: Logical not requires bool operand",
                        span.line, span.column
                    )
                }
            }
            Self::RequiresIntegerOperand { plural, span } => {
                if *plural {
                    format!(
                        "{}:{}: Bitwise operators require integer operands",
                        span.line, span.column
                    )
                } else {
                    format!(
                        "{}:{}: Bitwise not requires integer operand",
                        span.line, span.column
                    )
                }
            }
            Self::MapEntryTypeMismatch { value, span } => {
                if *value {
                    format!("{}:{}: Map value type mismatch", span.line, span.column)
                } else {
                    format!("{}:{}: Map key type mismatch", span.line, span.column)
                }
            }
            Self::CompoundAssignmentTypeMismatch { span } => {
                format!(
                    "{}:{}: Compound assignment type mismatch",
                    span.line, span.column
                )
            }
            Self::NotAFunction { ty, span } => {
                format!(
                    "{}:{}: Cannot call non-function type: '{}'",
                    span.line,
                    span.column,
                    ty.display_name()
                )
            }
            Self::CannotIndex { ty, span } => {
                format!(
                    "{}:{}: Cannot index type: '{}'",
                    span.line,
                    span.column,
                    ty.display_name()
                )
            }
            Self::CannotAccessField { ty, span } => {
                format!(
                    "{}:{}: Cannot access field on type: '{}'",
                    span.line,
                    span.column,
                    ty.display_name()
                )
            }
            Self::UnknownEnum { name, span } => {
                format!("{}:{}: Unknown enum: {}", span.line, span.column, name)
            }
            Self::GenericError { message, span } => {
                format!("{}:{}: {}", span.line, span.column, message)
            }
        }
    }
}

impl CheckerError {
    /// The message text without the leading "line:column: " location prefix.
    /// Useful for consumers (e.g. the LSP) that carry the location separately.
    pub fn body(&self) -> String {
        let span = self.span();
        let prefix = format!("{}:{}: ", span.line, span.column);
        let message = self.message();
        message
            .strip_prefix(&prefix)
            .map(str::to_string)
            .unwrap_or(message)
    }
}

impl fmt::Display for CheckerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// Structured error from the code generator
#[derive(Debug, Clone)]
pub enum CodegenError {
    UndefinedVariable {
        name: String,
    },
    WrongArgumentCount {
        function: String,
        expected: usize,
        got: usize,
    },
    UnknownFunction {
        name: String,
    },
    UnknownMethod {
        method: String,
        on_type: String,
    },
    GenericError {
        message: String,
    },
}

impl CodegenError {
    pub fn message(&self) -> String {
        match self {
            Self::UndefinedVariable { name } => {
                format!("Undefined variable: {}", name)
            }
            Self::WrongArgumentCount {
                function,
                expected,
                got,
            } => {
                format!(
                    "{}() requires {} arguments, got {}",
                    function, expected, got
                )
            }
            Self::UnknownFunction { name } => {
                format!("Unknown function: {}", name)
            }
            Self::UnknownMethod { method, on_type } => {
                format!("Unknown method: {} on {}", method, on_type)
            }
            Self::GenericError { message } => message.clone(),
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// Result type alias for checker operations
pub type CheckerResult<T> = Result<T, CheckerError>;

/// Result type alias for codegen operations
pub type CodegenResult<T> = Result<T, CodegenError>;
