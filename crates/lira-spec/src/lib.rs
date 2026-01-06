//! Lira Language Specification Validation
//!
//! This crate provides tools for validating the Lira implementation
//! against the formal specification in `docs/FORMAL_SPECIFICATION.md`.
//!
//! # Features
//!
//! - **EBNF Extraction**: Parse EBNF grammar from the specification
//! - **Grammar Validation**: Verify parser accepts/rejects per grammar rules
//! - **Type System Validation**: Verify type inference rules
//! - **Conformance Testing**: Run specification conformance tests
//!
//! # Example
//!
//! ```no_run
//! use lira_spec::{Specification, ValidationResult};
//!
//! let spec = Specification::load("docs/FORMAL_SPECIFICATION.md").unwrap();
//! let result = spec.validate_grammar();
//! println!("Grammar validation: {:?}", result);
//! ```

pub mod ebnf;
pub mod grammar;
pub mod treesitter;
pub mod types;
pub mod validation;

pub use ebnf::{EbnfGrammar, Production, Symbol};
pub use grammar::{GrammarValidator, ParseResult};
pub use treesitter::{compare_grammars, generate_report, GrammarComparison, TreeSitterGrammar};
pub use types::{TypeRule, TypeValidator};
pub use validation::{run_validation, Specification, ValidationReport, ValidationResult};

/// Version of the specification this crate validates against
pub const SPEC_VERSION: &str = "1.0.0";

/// Path to the formal specification (relative to workspace root)
pub const SPEC_PATH: &str = "docs/FORMAL_SPECIFICATION.md";
