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

/// Whether an operand-type error concerns a single operand (unary operator) or
/// multiple operands (binary operator).
#[derive(Debug, Clone)]
pub enum OperandArity {
    Single,
    Multiple,
}

/// Which side of a map entry carried a type mismatch.
#[derive(Debug, Clone)]
pub enum MapEntry {
    Key,
    Value,
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
        arity: OperandArity,
        span: Span,
    },
    /// An operand must be an integer (bitwise not / bitwise operators).
    RequiresIntegerOperand {
        arity: OperandArity,
        span: Span,
    },
    /// Map literal key/value type mismatch (no Types in original text).
    MapEntryTypeMismatch {
        entry: MapEntry,
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
    /// A `match` over an enum scrutinee does not cover all variants and has no
    /// wildcard / catch-all arm.
    NonExhaustiveMatch {
        enum_name: String,
        missing: Vec<String>,
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
            | Self::NonExhaustiveMatch { span, .. }
            | Self::GenericError { span, .. } => span,
        }
    }

    /// The message text without the leading "line:column: " location prefix.
    /// [`message`](Self::message) prepends the location to this exactly once,
    /// so each arm here is the single source of truth for its wording.
    pub fn body(&self) -> String {
        match self {
            Self::TypeMismatch { expected, got, .. } => format!(
                "Type mismatch: expected '{}', got '{}'",
                expected.display_name(),
                got.display_name()
            ),
            Self::UndefinedVariable { name, .. } => {
                format!("Undefined variable: {}", name)
            }
            Self::UnknownMethod {
                method, on_type, ..
            } => {
                format!("Unknown method: {} on type {}", method, on_type)
            }
            Self::UnknownField { field, on_type, .. } => {
                format!("Unknown field: {} on type {}", field, on_type)
            }
            Self::UnknownEnumVariant {
                variant, enum_name, ..
            } => {
                format!("Unknown variant '{}' for enum '{}'", variant, enum_name)
            }
            Self::NotAnEnum { type_name, .. } => {
                format!("'{}' is not an enum", type_name)
            }
            Self::UnknownType { name, .. } => format!("Unknown type: {}", name),
            Self::ArgumentCountMismatch { expected, got, .. } => {
                format!("Expected {} arguments, got {}", expected, got)
            }
            Self::NotCallable { ty, .. } => {
                format!("Type '{}' is not callable", ty.display_name())
            }
            Self::ImmutableAssignment { name, .. } => {
                format!("Cannot assign to immutable variable '{}'", name)
            }
            Self::BreakOutsideLoop { .. } => "break outside of loop".to_string(),
            Self::ContinueOutsideLoop { .. } => "continue outside of loop".to_string(),
            Self::TraitNotImplemented {
                trait_name,
                type_name,
                ..
            } => {
                format!(
                    "Trait '{}' is not implemented for type '{}'",
                    trait_name, type_name
                )
            }
            Self::MissingSelfParameter { method, .. } => {
                format!("Method '{}' must have 'self' as first parameter", method)
            }
            Self::TypeMismatchContext {
                context,
                expected,
                got,
                ..
            } => {
                let e = expected.display_name();
                let g = got.display_name();
                match context {
                    TypeContext::VarInit => {
                        format!("Type mismatch: expected '{}', got '{}'", e, g)
                    }
                    TypeContext::Return => {
                        format!("Return type mismatch: expected '{}', got '{}'", e, g)
                    }
                    TypeContext::Assignment => {
                        format!("Assignment type mismatch: expected '{}', got '{}'", e, g)
                    }
                    TypeContext::Argument => {
                        format!("Argument type mismatch: expected '{}', got '{}'", e, g)
                    }
                    TypeContext::ArrayElement => {
                        format!("Array element type mismatch: expected '{}', got '{}'", e, g)
                    }
                    TypeContext::MatchArm => {
                        format!("Match arm type mismatch: expected '{}', got '{}'", e, g)
                    }
                    TypeContext::DefaultValue { param } => format!(
                        "Default value type mismatch: parameter '{}' expects '{}', got '{}'",
                        param, e, g
                    ),
                }
            }
            Self::ConditionMustBeBool { got, .. } => match got {
                Some(t) => format!("Condition must be bool, got '{}'", t.display_name()),
                None => "If condition must be bool".to_string(),
            },
            Self::MustBeInteger { context, .. } => {
                let what = match context {
                    IntContext::ArrayIndex => "Array index",
                    IntContext::StringIndex => "String index",
                    IntContext::RangeStart => "Range start",
                    IntContext::RangeEnd => "Range end",
                };
                format!("{} must be integer", what)
            }
            Self::RequiresBoolOperand { arity, .. } => match arity {
                OperandArity::Multiple => "Logical operators require bool operands".to_string(),
                OperandArity::Single => "Logical not requires bool operand".to_string(),
            },
            Self::RequiresIntegerOperand { arity, .. } => match arity {
                OperandArity::Multiple => "Bitwise operators require integer operands".to_string(),
                OperandArity::Single => "Bitwise not requires integer operand".to_string(),
            },
            Self::MapEntryTypeMismatch { entry, .. } => match entry {
                MapEntry::Value => "Map value type mismatch".to_string(),
                MapEntry::Key => "Map key type mismatch".to_string(),
            },
            Self::CompoundAssignmentTypeMismatch { .. } => {
                "Compound assignment type mismatch".to_string()
            }
            Self::NotAFunction { ty, .. } => {
                format!("Cannot call non-function type: '{}'", ty.display_name())
            }
            Self::CannotIndex { ty, .. } => {
                format!("Cannot index type: '{}'", ty.display_name())
            }
            Self::CannotAccessField { ty, .. } => {
                format!("Cannot access field on type: '{}'", ty.display_name())
            }
            Self::UnknownEnum { name, .. } => format!("Unknown enum: {}", name),
            Self::NonExhaustiveMatch {
                enum_name, missing, ..
            } => {
                format!(
                    "Match on enum '{}' is not exhaustive: missing variant(s) {}. Add the missing arm(s) or a wildcard '_' arm.",
                    enum_name,
                    missing.join(", ")
                )
            }
            Self::GenericError { message, .. } => message.clone(),
        }
    }

    /// Format as a human-readable error message prefixed with its source
    /// location ("line:column: message").
    pub fn message(&self) -> String {
        let span = self.span();
        format!("{}:{}: {}", span.line, span.column, self.body())
    }
}

impl fmt::Display for CheckerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// Structured error from the code generator
#[derive(Debug, Clone, PartialEq)]
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
