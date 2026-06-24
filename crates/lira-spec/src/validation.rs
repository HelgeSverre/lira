//! Specification Validation
//!
//! Main entry point for validating the Lira implementation against
//! the formal specification.

use crate::ebnf::{EbnfError, EbnfGrammar};
use crate::grammar::{GrammarStats, GrammarValidator};
use crate::types::TypeValidator;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

/// Errors during validation
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Failed to load specification: {0}")]
    LoadError(String),

    #[error("EBNF extraction failed: {0}")]
    EbnfError(#[from] EbnfError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result of a single validation check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Name of the check
    pub name: String,
    /// Whether the check passed
    pub passed: bool,
    /// Number of tests run
    pub tests_run: usize,
    /// Number of tests passed
    pub tests_passed: usize,
    /// Details about failures
    pub failures: Vec<String>,
}

impl ValidationResult {
    /// Create a passed result
    pub fn pass(name: &str, tests: usize) -> Self {
        ValidationResult {
            name: name.to_string(),
            passed: true,
            tests_run: tests,
            tests_passed: tests,
            failures: Vec::new(),
        }
    }

    /// Create a failed result
    pub fn fail(name: &str, tests_run: usize, tests_passed: usize, failures: Vec<String>) -> Self {
        ValidationResult {
            name: name.to_string(),
            passed: false,
            tests_run,
            tests_passed,
            failures,
        }
    }
}

/// Complete validation report
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Specification version
    pub spec_version: String,
    /// Whether all validations passed
    pub all_passed: bool,
    /// Grammar validation results
    pub grammar: Vec<ValidationResult>,
    /// Type system validation results
    pub types: Vec<ValidationResult>,
    /// Summary statistics
    pub summary: ValidationSummary,
}

/// Summary of validation results
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub total_checks: usize,
    pub passed_checks: usize,
    pub failed_checks: usize,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub grammar_productions: usize,
    pub type_rules: usize,
}

/// The specification loader and validator
pub struct Specification {
    /// Raw specification content
    pub content: String,
    /// Extracted EBNF grammar
    pub grammar: EbnfGrammar,
    /// Specification version
    pub version: String,
}

impl Specification {
    /// Load specification from file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ValidationError> {
        let content = fs::read_to_string(path)?;
        Self::from_content(content)
    }

    /// Create specification from content string
    pub fn from_content(content: String) -> Result<Self, ValidationError> {
        let grammar = EbnfGrammar::parse(&content)?;
        let version = Self::extract_version(&content);

        Ok(Specification {
            content,
            grammar,
            version,
        })
    }

    /// Extract version from specification content
    fn extract_version(content: &str) -> String {
        for line in content.lines() {
            if line.starts_with("**Version**:") {
                return line
                    .strip_prefix("**Version**:")
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
            }
        }
        "unknown".to_string()
    }

    /// Validate grammar conformance
    pub fn validate_grammar(&self) -> Vec<ValidationResult> {
        let validator = GrammarValidator::new(self.grammar.clone());
        let validations = validator.validate_all();

        validations
            .into_iter()
            .map(|v| {
                if v.failed == 0 {
                    ValidationResult::pass(&v.name, v.test_count)
                } else {
                    ValidationResult::fail(&v.name, v.test_count, v.passed, v.failures)
                }
            })
            .collect()
    }

    /// Validate type system conformance
    pub fn validate_types(&self) -> Vec<ValidationResult> {
        let validator = TypeValidator::new();
        let validations = validator.validate_all();

        validations
            .into_iter()
            .map(|v| {
                if v.failed == 0 {
                    ValidationResult::pass(&v.rule_name, v.passed + v.failed)
                } else {
                    ValidationResult::fail(&v.rule_name, v.passed + v.failed, v.passed, v.failures)
                }
            })
            .collect()
    }

    /// Run all validations
    pub fn validate_all(&self) -> ValidationReport {
        let grammar_results = self.validate_grammar();
        let type_results = self.validate_types();

        let grammar_passed = grammar_results.iter().all(|r| r.passed);
        let types_passed = type_results.iter().all(|r| r.passed);

        let total_checks = grammar_results.len() + type_results.len();
        let passed_checks = grammar_results.iter().filter(|r| r.passed).count()
            + type_results.iter().filter(|r| r.passed).count();

        let total_tests: usize = grammar_results.iter().map(|r| r.tests_run).sum::<usize>()
            + type_results.iter().map(|r| r.tests_run).sum::<usize>();
        let passed_tests: usize = grammar_results
            .iter()
            .map(|r| r.tests_passed)
            .sum::<usize>()
            + type_results.iter().map(|r| r.tests_passed).sum::<usize>();

        ValidationReport {
            spec_version: self.version.clone(),
            all_passed: grammar_passed && types_passed,
            grammar: grammar_results,
            types: type_results,
            summary: ValidationSummary {
                total_checks,
                passed_checks,
                failed_checks: total_checks - passed_checks,
                total_tests,
                passed_tests,
                failed_tests: total_tests - passed_tests,
                grammar_productions: self.grammar.len(),
                type_rules: TypeValidator::new().rule_count(),
            },
        }
    }

    /// Get grammar statistics
    pub fn grammar_stats(&self) -> GrammarStats {
        let validator = GrammarValidator::new(self.grammar.clone());
        validator.stats()
    }

    /// Print validation report to stdout
    pub fn print_report(report: &ValidationReport) {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║           Lira Specification Validation Report               ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║ Spec Version: {:<47} ║", report.spec_version);
        println!(
            "║ Overall:      {:<47} ║",
            if report.all_passed {
                "✓ PASSED"
            } else {
                "✗ FAILED"
            }
        );
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║ Grammar Productions: {:<40} ║",
            report.summary.grammar_productions
        );
        println!("║ Type Rules:          {:<40} ║", report.summary.type_rules);
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║ Checks: {}/{} passed                                      ║",
            report.summary.passed_checks, report.summary.total_checks
        );
        println!(
            "║ Tests:  {}/{} passed                                      ║",
            report.summary.passed_tests, report.summary.total_tests
        );
        println!("╚══════════════════════════════════════════════════════════════╝");

        if !report.all_passed {
            println!("\nFailures:");
            println!("─────────");

            for result in &report.grammar {
                if !result.passed {
                    println!("\n[Grammar] {}:", result.name);
                    for failure in &result.failures {
                        println!("  ✗ {}", failure);
                    }
                }
            }

            for result in &report.types {
                if !result.passed {
                    println!("\n[Types] {}:", result.name);
                    for failure in &result.failures {
                        println!("  ✗ {}", failure);
                    }
                }
            }
        }
    }
}

/// Run validation from CLI
pub fn run_validation(spec_path: &str) -> Result<ValidationReport, ValidationError> {
    let spec = Specification::load(spec_path)?;
    let report = spec.validate_all();
    Specification::print_report(&report);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_SPEC: &str = r#"
# Lira Language Specification

**Version**: 1.0.0

## Lexical Structure

```ebnf
keyword = "let" | "var" | "fn" ;
```

## Types

All types are checked.
"#;

    #[test]
    fn test_load_minimal_spec() {
        let spec = Specification::from_content(MINIMAL_SPEC.to_string()).unwrap();
        assert_eq!(spec.version, "1.0.0");
    }

    #[test]
    fn test_extract_grammar() {
        let spec = Specification::from_content(MINIMAL_SPEC.to_string()).unwrap();
        assert!(spec.grammar.len() > 0);
    }
}
