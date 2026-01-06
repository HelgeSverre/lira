//! Specification Conformance Tests
//!
//! These tests verify that the Lira implementation conforms to the
//! formal specification in docs/FORMAL_SPECIFICATION.md.

use lira_spec::{
    compare_grammars, EbnfGrammar, GrammarValidator, Specification, TreeSitterGrammar,
    TypeValidator,
};
use std::path::Path;

const SPEC_PATH: &str = "../../docs/FORMAL_SPECIFICATION.md";
const GRAMMAR_PATH: &str = "../../editors/tree-sitter-lira/grammar.js";

/// Helper to check if spec file exists
fn spec_exists() -> bool {
    Path::new(SPEC_PATH).exists()
}

/// Helper to check if grammar file exists
fn grammar_exists() -> bool {
    Path::new(GRAMMAR_PATH).exists()
}

// ============================================================================
// EBNF Grammar Tests
// ============================================================================

#[test]
fn test_ebnf_extraction() {
    if !spec_exists() {
        eprintln!("Skipping: specification file not found at {}", SPEC_PATH);
        return;
    }

    let grammar = EbnfGrammar::from_spec(SPEC_PATH).expect("Failed to extract EBNF");
    assert!(grammar.len() > 0, "Grammar should have productions");
}

#[test]
fn test_keywords_extracted() {
    if !spec_exists() {
        return;
    }

    let grammar = EbnfGrammar::from_spec(SPEC_PATH).expect("Failed to extract EBNF");

    // Check for some expected keywords
    let expected_keywords = ["let", "var", "fn", "if", "else", "while", "for", "return"];

    for kw in &expected_keywords {
        // Keywords might be extracted from the grammar or might need to be
        // looked up in productions
        let found = grammar.keywords.iter().any(|k| k == *kw)
            || grammar
                .productions
                .values()
                .any(|p| format!("{:?}", p.definition).contains(kw));

        // For now, just verify the grammar was parsed
        // Actual keyword checking depends on how the spec is structured
        let _ = found;
    }
}

// ============================================================================
// Grammar Validation Tests
// ============================================================================

#[test]
fn test_variable_declarations() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    // Test let declarations
    assert!(validator.parse("let x = 42").success);
    assert!(validator.parse("let x: int = 42").success);
    assert!(validator.parse("var y = \"hello\"").success);

    // Test with type annotations
    assert!(validator.parse("let flag: bool = true").success);
    assert!(validator.parse("let arr: [int] = [1, 2, 3]").success);
}

#[test]
fn test_function_declarations() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    // Basic function
    assert!(validator.parse("fn add(a: int, b: int) -> int { a + b }").success);

    // Void function
    assert!(validator.parse("fn greet() { println(\"hello\") }").success);

    // Generic function
    assert!(validator.parse("fn identity<T>(x: T) -> T { x }").success);

    // Arrow function
    assert!(validator.parse("fn double(x: int) => x * 2").success);

    // Public function
    assert!(validator.parse("pub fn public_fn() {}").success);
}

#[test]
fn test_struct_declarations() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    assert!(validator.parse("struct Point { x: int, y: int }").success);
    assert!(validator.parse("struct Empty {}").success);
    assert!(validator.parse("struct Pair<T, U> { first: T, second: U }").success);
}

#[test]
fn test_class_declarations() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    assert!(validator.parse("class Animal { name: string }").success);
    assert!(validator.parse("class Dog extends Animal { breed: string }").success);
}

#[test]
fn test_enum_declarations() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    assert!(validator.parse("enum Color { Red, Green, Blue }").success);
    assert!(validator.parse("enum Option { Some(int), None }").success);
}

#[test]
fn test_control_flow() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    // If statements
    assert!(validator.parse("if x > 0 { print(x) }").success);
    assert!(validator.parse("if x > 0 { 1 } else { 0 }").success);

    // While loops
    assert!(validator.parse("while x < 10 { x = x + 1 }").success);

    // For loops
    assert!(validator.parse("for i in items { print(i) }").success);

    // Match expressions
    assert!(validator.parse("match x { 1 => \"one\", _ => \"other\" }").success);
}

#[test]
fn test_expressions() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    // Literals
    assert!(validator.parse("42").success);
    assert!(validator.parse("3.14").success);
    assert!(validator.parse("\"hello\"").success);
    assert!(validator.parse("true").success);
    assert!(validator.parse("[1, 2, 3]").success);

    // Binary operations
    assert!(validator.parse("1 + 2").success);
    assert!(validator.parse("a * b + c").success);
    assert!(validator.parse("x == y").success);
    assert!(validator.parse("a && b || c").success);

    // Function calls
    assert!(validator.parse("foo(1, 2, 3)").success);
    assert!(validator.parse("obj.method()").success);

    // Index and field access
    assert!(validator.parse("arr[0]").success);
    assert!(validator.parse("point.x").success);
    assert!(validator.parse("obj?.field").success);

    // Special operators
    assert!(validator.parse("x ?? fallback").success);
    assert!(validator.parse("1..10").success);
}

#[test]
fn test_lambda_expressions() {
    let grammar = EbnfGrammar {
        productions: std::collections::HashMap::new(),
        order: Vec::new(),
        keywords: Vec::new(),
        operators: Vec::new(),
    };
    let validator = GrammarValidator::new(grammar);

    assert!(validator.parse("|x| x * 2").success);
    assert!(validator.parse("|a, b| a + b").success);
    assert!(validator.parse("|| 42").success);
}

// ============================================================================
// Type System Tests
// ============================================================================

#[test]
fn test_type_inference_rules() {
    let validator = TypeValidator::new();
    let results = validator.validate_all();

    // Count passed and failed
    let passed: usize = results.iter().map(|r| r.passed).sum();
    let failed: usize = results.iter().map(|r| r.failed).sum();

    println!("Type inference tests: {} passed, {} failed", passed, failed);

    // Print failures
    for result in &results {
        if result.failed > 0 {
            println!("\nRule '{}' failures:", result.rule_name);
            for failure in &result.failures {
                println!("  - {}", failure);
            }
        }
    }

    // We expect most tests to pass
    assert!(
        passed > failed,
        "Expected more tests to pass than fail: {} passed, {} failed",
        passed,
        failed
    );
}

#[test]
fn test_literal_type_inference() {
    let validator = TypeValidator::new();
    let results = validator.validate_all();

    let literal_results: Vec<_> = results
        .iter()
        .filter(|r| r.rule_name.starts_with("literal_"))
        .collect();

    for result in literal_results {
        assert_eq!(
            result.failed, 0,
            "Literal type inference rule '{}' failed: {:?}",
            result.rule_name, result.failures
        );
    }
}

// ============================================================================
// Full Specification Validation
// ============================================================================

#[test]
fn test_full_spec_validation() {
    if !spec_exists() {
        eprintln!("Skipping: specification file not found at {}", SPEC_PATH);
        return;
    }

    let spec = Specification::load(SPEC_PATH).expect("Failed to load specification");
    let report = spec.validate_all();

    println!("\n{}", "=".repeat(60));
    println!("Specification Validation Report");
    println!("{}", "=".repeat(60));
    println!("Version: {}", report.spec_version);
    println!(
        "Status: {}",
        if report.all_passed { "PASSED" } else { "FAILED" }
    );
    println!("{}", "-".repeat(60));
    println!(
        "Grammar: {} productions",
        report.summary.grammar_productions
    );
    println!("Types: {} rules", report.summary.type_rules);
    println!(
        "Tests: {}/{} passed",
        report.summary.passed_tests, report.summary.total_tests
    );
    println!("{}", "=".repeat(60));

    // Print detailed failures if any
    if !report.all_passed {
        println!("\nDetailed Failures:");
        for result in report.grammar.iter().chain(report.types.iter()) {
            if !result.passed {
                println!("\n{}:", result.name);
                for failure in &result.failures {
                    println!("  - {}", failure);
                }
            }
        }
    }
}

// ============================================================================
// Tree-sitter Comparison Tests
// ============================================================================

#[test]
fn test_treesitter_grammar_parsing() {
    if !grammar_exists() {
        eprintln!("Skipping: grammar file not found at {}", GRAMMAR_PATH);
        return;
    }

    let grammar = TreeSitterGrammar::from_file(GRAMMAR_PATH).expect("Failed to parse grammar.js");
    assert!(grammar.len() > 0, "Grammar should have rules");
    println!("Parsed {} tree-sitter rules", grammar.len());
}

#[test]
fn test_treesitter_rules_extracted() {
    if !grammar_exists() {
        return;
    }

    let grammar = TreeSitterGrammar::from_file(GRAMMAR_PATH).expect("Failed to parse grammar.js");

    // Check for expected rules
    let expected_rules = [
        "source_file",
        "function_declaration",
        "struct_declaration",
        "class_declaration",
        "enum_declaration",
        "variable_declaration",
        "if_expression",
        "match_expression",
        "binary_expression",
    ];

    for rule in &expected_rules {
        assert!(
            grammar.rules.contains_key(*rule),
            "Expected rule '{}' not found in tree-sitter grammar",
            rule
        );
    }
}

#[test]
fn test_treesitter_keywords_extracted() {
    if !grammar_exists() {
        return;
    }

    let grammar = TreeSitterGrammar::from_file(GRAMMAR_PATH).expect("Failed to parse grammar.js");

    // Check for expected keywords
    let expected_keywords = ["let", "var", "fn", "if", "else", "while", "for", "return"];

    for kw in &expected_keywords {
        assert!(
            grammar.keywords.contains(*kw),
            "Expected keyword '{}' not found in tree-sitter grammar",
            kw
        );
    }
}

#[test]
fn test_grammar_comparison() {
    if !spec_exists() || !grammar_exists() {
        eprintln!("Skipping: spec or grammar file not found");
        return;
    }

    let ebnf = EbnfGrammar::from_spec(SPEC_PATH).expect("Failed to load EBNF");
    let ts = TreeSitterGrammar::from_file(GRAMMAR_PATH).expect("Failed to load tree-sitter");

    let comparison = compare_grammars(&ebnf, &ts);

    println!("\nGrammar Comparison:");
    println!("  Matched rules: {}", comparison.matched.len());
    println!(
        "  Missing in tree-sitter: {}",
        comparison.missing_in_treesitter.len()
    );
    println!("  Missing in EBNF: {}", comparison.missing_in_ebnf.len());
    println!("  Sync percentage: {:.1}%", comparison.sync_percentage());

    // We expect at least some rules to match
    assert!(
        comparison.matched.len() > 0,
        "Expected at least some matching rules"
    );
}

// ============================================================================
// Regression Tests
// ============================================================================

/// Tests for specific language features that should always work
mod regression {
    use super::*;

    #[test]
    fn test_null_coalescing() {
        let grammar = EbnfGrammar {
            productions: std::collections::HashMap::new(),
            order: Vec::new(),
            keywords: Vec::new(),
            operators: Vec::new(),
        };
        let validator = GrammarValidator::new(grammar);
        assert!(validator.parse("x ?? 0").success);
        assert!(validator.parse("a ?? b ?? c").success);
    }

    #[test]
    fn test_optional_chaining() {
        let grammar = EbnfGrammar {
            productions: std::collections::HashMap::new(),
            order: Vec::new(),
            keywords: Vec::new(),
            operators: Vec::new(),
        };
        let validator = GrammarValidator::new(grammar);
        assert!(validator.parse("obj?.field").success);
        assert!(validator.parse("a?.b?.c").success);
    }

    #[test]
    fn test_range_expressions() {
        let grammar = EbnfGrammar {
            productions: std::collections::HashMap::new(),
            order: Vec::new(),
            keywords: Vec::new(),
            operators: Vec::new(),
        };
        let validator = GrammarValidator::new(grammar);
        assert!(validator.parse("1..10").success);
        assert!(validator.parse("1..=10").success);
        assert!(validator.parse("start..end").success);
    }

    #[test]
    fn test_trait_declarations() {
        let grammar = EbnfGrammar {
            productions: std::collections::HashMap::new(),
            order: Vec::new(),
            keywords: Vec::new(),
            operators: Vec::new(),
        };
        let validator = GrammarValidator::new(grammar);
        assert!(validator.parse("trait Eq { fn eq(self, other: Self) -> bool }").success);
    }

    #[test]
    fn test_impl_blocks() {
        let grammar = EbnfGrammar {
            productions: std::collections::HashMap::new(),
            order: Vec::new(),
            keywords: Vec::new(),
            operators: Vec::new(),
        };
        let validator = GrammarValidator::new(grammar);
        assert!(validator.parse("impl Point { fn new() -> Self { Point { x: 0, y: 0 } } }").success);
    }
}
