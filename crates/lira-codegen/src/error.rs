//! Errors produced while lowering Lira to native code.

use std::fmt;

use lirac::ast::Span;

/// Why native code generation stopped.
#[derive(Debug, Clone, PartialEq)]
pub enum CodegenError {
    /// The construct is valid Lira, but the native backend cannot lower it yet.
    /// The bytecode VM remains the complete implementation.
    Unsupported { message: String, span: Option<Span> },
    /// Something the checker should have caught, or an internal inconsistency.
    Internal(String),
    /// The object file could not be written, or the system linker failed.
    Link(String),
}

impl CodegenError {
    pub fn unsupported(message: impl Into<String>) -> Self {
        CodegenError::Unsupported {
            message: message.into(),
            span: None,
        }
    }

    pub fn unsupported_at(message: impl Into<String>, span: &Span) -> Self {
        CodegenError::Unsupported {
            message: message.into(),
            span: Some(span.clone()),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        CodegenError::Internal(message.into())
    }

    pub fn link(message: impl Into<String>) -> Self {
        CodegenError::Link(message.into())
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::Unsupported {
                message,
                span: Some(span),
            } => write!(
                f,
                "{}:{}: native backend: {} (this program still runs under `lira run`)",
                span.line, span.column, message
            ),
            CodegenError::Unsupported {
                message,
                span: None,
            } => write!(
                f,
                "native backend: {} (this program still runs under `lira run`)",
                message
            ),
            CodegenError::Internal(message) => {
                write!(f, "native backend: internal error: {}", message)
            }
            CodegenError::Link(message) => write!(f, "native backend: {}", message),
        }
    }
}

impl std::error::Error for CodegenError {}

impl From<CodegenError> for String {
    fn from(err: CodegenError) -> String {
        err.to_string()
    }
}

pub type CodegenResult<T> = Result<T, CodegenError>;
