use crate::ast::Span;
use crate::checker::Type;
use std::fmt;

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
