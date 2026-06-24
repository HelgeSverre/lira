//! Type System Validation
//!
//! Validates that type inference and type checking conform to specification rules.

use lirac::checker;
use lirac::lexer;
use lirac::parser;

/// A type inference rule from the specification
#[derive(Debug, Clone)]
pub struct TypeRule {
    /// Rule name/identifier
    pub name: String,
    /// Description of the rule
    pub description: String,
    /// Test cases for this rule
    pub test_cases: Vec<TypeTestCase>,
}

/// A test case for a type rule
#[derive(Debug, Clone)]
pub struct TypeTestCase {
    /// Source code to type check
    pub source: String,
    /// Expected type (as string)
    pub expected_type: Option<String>,
    /// Whether type checking should succeed
    pub should_succeed: bool,
    /// Expected error substring (if should_succeed is false)
    pub expected_error: Option<String>,
}

/// Result of validating a type rule
#[derive(Debug)]
pub struct TypeRuleValidation {
    pub rule_name: String,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<String>,
}

/// Type system validator
pub struct TypeValidator {
    rules: Vec<TypeRule>,
}

impl TypeValidator {
    /// Create a new type validator with standard rules
    pub fn new() -> Self {
        let rules = Self::standard_rules();
        TypeValidator { rules }
    }

    /// Define standard type inference rules from specification
    fn standard_rules() -> Vec<TypeRule> {
        vec![
            // Literal type inference
            TypeRule {
                name: "literal_int".to_string(),
                description: "Integer literals have type int".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x = 42".to_string(),
                        expected_type: Some("int".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = -123".to_string(),
                        expected_type: Some("int".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = 0xFF".to_string(),
                        expected_type: Some("int".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            TypeRule {
                name: "literal_float".to_string(),
                description: "Float literals have type float".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x = 3.14".to_string(),
                        expected_type: Some("float".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = 1e10".to_string(),
                        expected_type: Some("float".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            TypeRule {
                name: "literal_string".to_string(),
                description: "String literals have type string".to_string(),
                test_cases: vec![TypeTestCase {
                    source: "let x = \"hello\"".to_string(),
                    expected_type: Some("string".to_string()),
                    should_succeed: true,
                    expected_error: None,
                }],
            },
            TypeRule {
                name: "literal_bool".to_string(),
                description: "Boolean literals have type bool".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x = true".to_string(),
                        expected_type: Some("bool".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = false".to_string(),
                        expected_type: Some("bool".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            // Type annotation
            TypeRule {
                name: "type_annotation".to_string(),
                description: "Explicit type annotations are respected".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x: int = 42".to_string(),
                        expected_type: Some("int".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x: float = 3.14".to_string(),
                        expected_type: Some("float".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            // Type mismatch errors
            TypeRule {
                name: "type_mismatch".to_string(),
                description: "Type mismatches produce errors".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x: int = \"hello\"".to_string(),
                        expected_type: None,
                        should_succeed: false,
                        expected_error: Some("mismatch".to_string()),
                    },
                    TypeTestCase {
                        source: "let x: string = 42".to_string(),
                        expected_type: None,
                        should_succeed: false,
                        expected_error: Some("mismatch".to_string()),
                    },
                ],
            },
            // Binary operations
            TypeRule {
                name: "binary_arithmetic".to_string(),
                description: "Arithmetic operations on numeric types".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x = 1 + 2".to_string(),
                        expected_type: Some("int".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = 1.0 + 2.0".to_string(),
                        expected_type: Some("float".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = 1 * 2 + 3".to_string(),
                        expected_type: Some("int".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            TypeRule {
                name: "binary_comparison".to_string(),
                description: "Comparison operations return bool".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x = 1 < 2".to_string(),
                        expected_type: Some("bool".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = 1 == 2".to_string(),
                        expected_type: Some("bool".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            TypeRule {
                name: "binary_logical".to_string(),
                description: "Logical operations on bool return bool".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x = true && false".to_string(),
                        expected_type: Some("bool".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = true || false".to_string(),
                        expected_type: Some("bool".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            // Optional types
            TypeRule {
                name: "optional_promotion".to_string(),
                description: "Non-optional values can be assigned to optional types".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x: int? = 42".to_string(),
                        expected_type: Some("int?".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x: int? = null".to_string(),
                        expected_type: Some("int?".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            // Array types
            TypeRule {
                name: "array_inference".to_string(),
                description: "Array type inferred from elements".to_string(),
                test_cases: vec![
                    TypeTestCase {
                        source: "let x = [1, 2, 3]".to_string(),
                        expected_type: Some("[int]".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                    TypeTestCase {
                        source: "let x = [\"a\", \"b\"]".to_string(),
                        expected_type: Some("[string]".to_string()),
                        should_succeed: true,
                        expected_error: None,
                    },
                ],
            },
            // Tuple types
            TypeRule {
                name: "tuple_inference".to_string(),
                description: "Tuple type inferred from elements".to_string(),
                test_cases: vec![TypeTestCase {
                    source: "let x = (1, \"two\", true)".to_string(),
                    expected_type: Some("(int, string, bool)".to_string()),
                    should_succeed: true,
                    expected_error: None,
                }],
            },
            // Function return type inference
            TypeRule {
                name: "function_return_inference".to_string(),
                description: "Function return type inferred from body".to_string(),
                test_cases: vec![TypeTestCase {
                    source: "fn add(a: int, b: int) { a + b }".to_string(),
                    expected_type: None, // Checking the function, not a let binding
                    should_succeed: true,
                    expected_error: None,
                }],
            },
            // Undefined variable errors
            TypeRule {
                name: "undefined_variable".to_string(),
                description: "Undefined variables produce errors".to_string(),
                test_cases: vec![TypeTestCase {
                    source: "let x = undefined_var".to_string(),
                    expected_type: None,
                    should_succeed: false,
                    expected_error: Some("undefined".to_string().to_lowercase()),
                }],
            },
            // Immutability
            TypeRule {
                name: "immutability".to_string(),
                description: "Cannot assign to immutable bindings".to_string(),
                test_cases: vec![TypeTestCase {
                    source: "let x = 1\nx = 2".to_string(),
                    expected_type: None,
                    should_succeed: false,
                    expected_error: Some("immutable".to_string()),
                }],
            },
        ]
    }

    /// Validate all type rules
    pub fn validate_all(&self) -> Vec<TypeRuleValidation> {
        self.rules
            .iter()
            .map(|rule| self.validate_rule(rule))
            .collect()
    }

    /// Validate a single type rule
    pub fn validate_rule(&self, rule: &TypeRule) -> TypeRuleValidation {
        let mut passed = 0;
        let mut failed = 0;
        let mut failures = Vec::new();

        for case in &rule.test_cases {
            let result = self.check_type(&case.source);

            let test_passed = if case.should_succeed {
                result.is_ok()
            } else {
                match &result {
                    Err(errors) => {
                        if let Some(expected) = &case.expected_error {
                            errors
                                .iter()
                                .any(|e| e.to_lowercase().contains(&expected.to_lowercase()))
                        } else {
                            true
                        }
                    }
                    Ok(_) => false,
                }
            };

            if test_passed {
                passed += 1;
            } else {
                failed += 1;
                let msg = match result {
                    Ok(_) => "succeeded unexpectedly".to_string(),
                    Err(errors) => format!("errors: {}", errors.join("; ")),
                };
                failures.push(format!("{}: {}", case.source, msg));
            }
        }

        TypeRuleValidation {
            rule_name: rule.name.clone(),
            passed,
            failed,
            failures,
        }
    }

    /// Type check a source string
    fn check_type(&self, source: &str) -> Result<(), Vec<String>> {
        // Tokenize
        let tokens = lexer::tokenize(source).map_err(|e| vec![format!("Lexer error: {}", e)])?;

        // Parse
        let ast = parser::parse(&tokens).map_err(|e| vec![e])?;

        // Type check
        match checker::check(&ast) {
            Ok(_) => Ok(()),
            Err(e) => Err(vec![e]),
        }
    }

    /// Get rule count
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Get total test case count
    pub fn test_case_count(&self) -> usize {
        self.rules.iter().map(|r| r.test_cases.len()).sum()
    }
}

impl Default for TypeValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_validator_creation() {
        let validator = TypeValidator::new();
        assert!(validator.rule_count() > 0);
        assert!(validator.test_case_count() > 0);
    }
}
