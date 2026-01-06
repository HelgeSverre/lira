//! Grammar Validation
//!
//! Validates that the Lira parser conforms to the specification grammar.

use crate::ebnf::{EbnfGrammar, Symbol};
use lirac::lexer;
use lirac::parser;
use std::collections::HashMap;

/// Result of parsing a test input
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Whether parsing succeeded
    pub success: bool,
    /// Error message if parsing failed
    pub error: Option<String>,
    /// Tokens produced by lexer
    pub token_count: usize,
    /// AST node count (if successful)
    pub node_count: usize,
}

/// Grammar validation result for a single production
#[derive(Debug)]
pub struct ProductionValidation {
    /// Production name
    pub name: String,
    /// Number of test cases
    pub test_count: usize,
    /// Number of passed tests
    pub passed: usize,
    /// Number of failed tests
    pub failed: usize,
    /// Failure details
    pub failures: Vec<String>,
}

/// Grammar validator
pub struct GrammarValidator {
    grammar: EbnfGrammar,
    test_cases: HashMap<String, Vec<TestCase>>,
}

/// A test case for a production
#[derive(Debug, Clone)]
pub struct TestCase {
    /// Input source code
    pub input: String,
    /// Whether the input should be accepted
    pub should_accept: bool,
    /// Description of the test
    pub description: String,
}

impl GrammarValidator {
    /// Create a new grammar validator
    pub fn new(grammar: EbnfGrammar) -> Self {
        let test_cases = Self::generate_test_cases(&grammar);
        GrammarValidator { grammar, test_cases }
    }

    /// Generate test cases for all productions
    fn generate_test_cases(grammar: &EbnfGrammar) -> HashMap<String, Vec<TestCase>> {
        let mut cases = HashMap::new();

        // Generate test cases for each production
        for production in grammar.iter() {
            let mut production_cases = Vec::new();

            // Generate positive test cases from the grammar
            let positive_examples = Self::generate_examples(&production.definition, grammar);
            for (i, example) in positive_examples.into_iter().enumerate() {
                production_cases.push(TestCase {
                    input: example,
                    should_accept: true,
                    description: format!("{}:positive:{}", production.name, i),
                });
            }

            cases.insert(production.name.clone(), production_cases);
        }

        // Add known test cases for important productions
        Self::add_known_test_cases(&mut cases);

        cases
    }

    /// Generate example strings from a symbol (with depth limit to prevent stack overflow)
    fn generate_examples(symbol: &Symbol, grammar: &EbnfGrammar) -> Vec<String> {
        Self::generate_examples_with_depth(symbol, grammar, 0)
    }

    /// Generate examples with recursion depth tracking
    fn generate_examples_with_depth(
        symbol: &Symbol,
        grammar: &EbnfGrammar,
        depth: usize,
    ) -> Vec<String> {
        const MAX_DEPTH: usize = 10;

        if depth > MAX_DEPTH {
            return vec!["...".to_string()];
        }

        match symbol {
            Symbol::Terminal(t) => vec![t.clone()],

            Symbol::NonTerminal(name) => {
                if let Some(prod) = grammar.get(name) {
                    Self::generate_examples_with_depth(&prod.definition, grammar, depth + 1)
                } else {
                    // Return placeholder for undefined non-terminals
                    vec![format!("<{}>", name)]
                }
            }

            Symbol::Sequence(symbols) => {
                let mut result = vec![String::new()];
                for sym in symbols {
                    let examples = Self::generate_examples_with_depth(sym, grammar, depth + 1);
                    if let Some(first) = examples.first() {
                        for r in &mut result {
                            if !r.is_empty() {
                                r.push(' ');
                            }
                            r.push_str(first);
                        }
                    }
                }
                result
            }

            Symbol::Choice(alternatives) => {
                let mut result = Vec::new();
                for alt in alternatives {
                    result.extend(Self::generate_examples_with_depth(alt, grammar, depth + 1));
                }
                result
            }

            Symbol::Optional(inner) => {
                let mut result = vec![String::new()]; // Empty case
                result.extend(Self::generate_examples_with_depth(inner, grammar, depth + 1));
                result
            }

            Symbol::Repetition(inner) => {
                let mut result = vec![String::new()]; // Zero repetitions
                let examples = Self::generate_examples_with_depth(inner, grammar, depth + 1);
                if let Some(first) = examples.first() {
                    result.push(first.clone()); // One repetition
                }
                result
            }

            Symbol::OneOrMore(inner) => {
                Self::generate_examples_with_depth(inner, grammar, depth + 1)
            }

            Symbol::Group(inner) => Self::generate_examples_with_depth(inner, grammar, depth + 1),

            Symbol::Exception(base, _) => {
                Self::generate_examples_with_depth(base, grammar, depth + 1)
            }
        }
    }

    /// Add known test cases for important language constructs
    fn add_known_test_cases(cases: &mut HashMap<String, Vec<TestCase>>) {
        // Variable declarations
        cases.insert(
            "variable_decl".to_string(),
            vec![
                TestCase {
                    input: "let x = 42".to_string(),
                    should_accept: true,
                    description: "simple let binding".to_string(),
                },
                TestCase {
                    input: "let x: int = 42".to_string(),
                    should_accept: true,
                    description: "let with type annotation".to_string(),
                },
                TestCase {
                    input: "var y = \"hello\"".to_string(),
                    should_accept: true,
                    description: "mutable var binding".to_string(),
                },
                TestCase {
                    input: "let (a, b) = (1, 2)".to_string(),
                    should_accept: true,
                    description: "tuple destructuring".to_string(),
                },
            ],
        );

        // Function declarations
        cases.insert(
            "function_decl".to_string(),
            vec![
                TestCase {
                    input: "fn add(a: int, b: int) -> int { a + b }".to_string(),
                    should_accept: true,
                    description: "simple function".to_string(),
                },
                TestCase {
                    input: "fn identity<T>(x: T) -> T { x }".to_string(),
                    should_accept: true,
                    description: "generic function".to_string(),
                },
                TestCase {
                    input: "fn greet() { println(\"hello\") }".to_string(),
                    should_accept: true,
                    description: "void function".to_string(),
                },
                TestCase {
                    input: "pub fn public_fn() {}".to_string(),
                    should_accept: true,
                    description: "public function".to_string(),
                },
                TestCase {
                    input: "fn arrow() => 42".to_string(),
                    should_accept: true,
                    description: "arrow function".to_string(),
                },
            ],
        );

        // Struct declarations
        cases.insert(
            "struct_decl".to_string(),
            vec![
                TestCase {
                    input: "struct Point { x: int, y: int }".to_string(),
                    should_accept: true,
                    description: "simple struct".to_string(),
                },
                TestCase {
                    input: "struct Pair<T, U> { first: T, second: U }".to_string(),
                    should_accept: true,
                    description: "generic struct".to_string(),
                },
            ],
        );

        // Class declarations
        cases.insert(
            "class_decl".to_string(),
            vec![
                TestCase {
                    input: "class Animal { name: string }".to_string(),
                    should_accept: true,
                    description: "simple class".to_string(),
                },
                TestCase {
                    input: "class Dog extends Animal { breed: string }".to_string(),
                    should_accept: true,
                    description: "class with inheritance".to_string(),
                },
            ],
        );

        // Enum declarations
        cases.insert(
            "enum_decl".to_string(),
            vec![
                TestCase {
                    input: "enum Color { Red, Green, Blue }".to_string(),
                    should_accept: true,
                    description: "simple enum".to_string(),
                },
                TestCase {
                    input: "enum Option { Some(int), None }".to_string(),
                    should_accept: true,
                    description: "enum with data".to_string(),
                },
            ],
        );

        // Control flow
        cases.insert(
            "if_stmt".to_string(),
            vec![
                TestCase {
                    input: "if x > 0 { print(x) }".to_string(),
                    should_accept: true,
                    description: "simple if".to_string(),
                },
                TestCase {
                    input: "if x > 0 { print(x) } else { print(0) }".to_string(),
                    should_accept: true,
                    description: "if-else".to_string(),
                },
                TestCase {
                    input: "if a { 1 } else if b { 2 } else { 3 }".to_string(),
                    should_accept: true,
                    description: "if-else-if chain".to_string(),
                },
            ],
        );

        // Match expressions
        cases.insert(
            "match_expr".to_string(),
            vec![
                TestCase {
                    input: "match x { 1 => \"one\", _ => \"other\" }".to_string(),
                    should_accept: true,
                    description: "simple match".to_string(),
                },
                TestCase {
                    input: "match opt { Some(v) => v, None => 0 }".to_string(),
                    should_accept: true,
                    description: "match with destructuring".to_string(),
                },
            ],
        );

        // Lambda expressions
        cases.insert(
            "lambda_expr".to_string(),
            vec![
                TestCase {
                    input: "|x| x * 2".to_string(),
                    should_accept: true,
                    description: "simple lambda".to_string(),
                },
                TestCase {
                    input: "|a, b| a + b".to_string(),
                    should_accept: true,
                    description: "multi-param lambda".to_string(),
                },
                TestCase {
                    input: "|x: int| -> int { x + 1 }".to_string(),
                    should_accept: true,
                    description: "typed lambda".to_string(),
                },
            ],
        );

        // Expressions
        cases.insert(
            "expression".to_string(),
            vec![
                TestCase {
                    input: "42".to_string(),
                    should_accept: true,
                    description: "integer literal".to_string(),
                },
                TestCase {
                    input: "3.14".to_string(),
                    should_accept: true,
                    description: "float literal".to_string(),
                },
                TestCase {
                    input: "\"hello\"".to_string(),
                    should_accept: true,
                    description: "string literal".to_string(),
                },
                TestCase {
                    input: "a + b * c".to_string(),
                    should_accept: true,
                    description: "binary expression".to_string(),
                },
                TestCase {
                    input: "foo(1, 2, 3)".to_string(),
                    should_accept: true,
                    description: "function call".to_string(),
                },
                TestCase {
                    input: "obj.method()".to_string(),
                    should_accept: true,
                    description: "method call".to_string(),
                },
                TestCase {
                    input: "arr[0]".to_string(),
                    should_accept: true,
                    description: "index access".to_string(),
                },
                TestCase {
                    input: "[1, 2, 3]".to_string(),
                    should_accept: true,
                    description: "array literal".to_string(),
                },
                TestCase {
                    input: "(1, \"two\", 3.0)".to_string(),
                    should_accept: true,
                    description: "tuple literal".to_string(),
                },
                TestCase {
                    input: "x ?? default".to_string(),
                    should_accept: true,
                    description: "null coalescing".to_string(),
                },
                TestCase {
                    input: "obj?.field".to_string(),
                    should_accept: true,
                    description: "optional chaining".to_string(),
                },
                TestCase {
                    input: "1..10".to_string(),
                    should_accept: true,
                    description: "range expression".to_string(),
                },
            ],
        );
    }

    /// Validate all productions
    pub fn validate_all(&self) -> Vec<ProductionValidation> {
        let mut results = Vec::new();

        for (name, cases) in &self.test_cases {
            let validation = self.validate_production(name, cases);
            results.push(validation);
        }

        results
    }

    /// Validate a single production
    pub fn validate_production(
        &self,
        name: &str,
        cases: &[TestCase],
    ) -> ProductionValidation {
        let mut passed = 0;
        let mut failed = 0;
        let mut failures = Vec::new();

        for case in cases {
            let result = self.parse(&case.input);

            let test_passed = if case.should_accept {
                result.success
            } else {
                !result.success
            };

            if test_passed {
                passed += 1;
            } else {
                failed += 1;
                let expected = if case.should_accept { "accept" } else { "reject" };
                let actual = if result.success { "accepted" } else { "rejected" };
                failures.push(format!(
                    "{}: expected to {}, but {} - {}",
                    case.description, expected, actual,
                    result.error.unwrap_or_default()
                ));
            }
        }

        ProductionValidation {
            name: name.to_string(),
            test_count: cases.len(),
            passed,
            failed,
            failures,
        }
    }

    /// Parse a source string
    pub fn parse(&self, source: &str) -> ParseResult {
        // Wrap in a function if needed for expression tests
        let wrapped = if !source.contains("fn ")
            && !source.contains("struct ")
            && !source.contains("class ")
            && !source.contains("enum ")
            && !source.contains("trait ")
            && !source.contains("impl ")
        {
            format!("fn __test__() {{ {} }}", source)
        } else {
            source.to_string()
        };

        // Tokenize
        let tokens = match lexer::tokenize(&wrapped) {
            Ok(t) => t,
            Err(e) => {
                return ParseResult {
                    success: false,
                    error: Some(format!("Lexer error: {}", e)),
                    token_count: 0,
                    node_count: 0,
                };
            }
        };
        let token_count = tokens.len();

        // Parse
        match parser::parse(&tokens) {
            Ok(ast) => ParseResult {
                success: true,
                error: None,
                token_count,
                node_count: ast.statements.len(),
            },
            Err(e) => ParseResult {
                success: false,
                error: Some(e),
                token_count,
                node_count: 0,
            },
        }
    }

    /// Get grammar statistics
    pub fn stats(&self) -> GrammarStats {
        GrammarStats {
            production_count: self.grammar.len(),
            keyword_count: self.grammar.keywords.len(),
            operator_count: self.grammar.operators.len(),
            test_case_count: self.test_cases.values().map(|v| v.len()).sum(),
        }
    }
}

/// Grammar statistics
#[derive(Debug)]
pub struct GrammarStats {
    pub production_count: usize,
    pub keyword_count: usize,
    pub operator_count: usize,
    pub test_case_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_expression() {
        let grammar = EbnfGrammar {
            productions: HashMap::new(),
            order: Vec::new(),
            keywords: Vec::new(),
            operators: Vec::new(),
        };
        let validator = GrammarValidator::new(grammar);
        let result = validator.parse("42");
        assert!(result.success);
    }

    #[test]
    fn test_parse_function() {
        let grammar = EbnfGrammar {
            productions: HashMap::new(),
            order: Vec::new(),
            keywords: Vec::new(),
            operators: Vec::new(),
        };
        let validator = GrammarValidator::new(grammar);
        let result = validator.parse("fn add(a: int, b: int) -> int { a + b }");
        assert!(result.success);
    }
}
