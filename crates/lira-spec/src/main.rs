//! Lira Specification Validator CLI
//!
//! Validates the Lira implementation against the formal specification.
//!
//! Usage:
//!   lira-spec [validate] [SPEC_PATH]     Validate implementation against spec
//!   lira-spec compare [SPEC] [GRAMMAR]   Compare EBNF with tree-sitter grammar
//!
//! If no paths are provided, uses default locations.

use lira_spec::{
    compare_grammars, generate_report, run_validation, EbnfGrammar, TreeSitterGrammar, SPEC_PATH,
};
use std::env;
use std::process;

const DEFAULT_GRAMMAR_PATH: &str = "editors/tree-sitter-lira/grammar.js";

fn print_usage() {
    eprintln!("Lira Specification Validator");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  lira-spec [validate] [SPEC_PATH]");
    eprintln!("    Validate implementation against specification");
    eprintln!();
    eprintln!("  lira-spec compare [SPEC_PATH] [GRAMMAR_PATH]");
    eprintln!("    Compare EBNF specification with tree-sitter grammar");
    eprintln!();
    eprintln!("Defaults:");
    eprintln!("  SPEC_PATH:    {}", SPEC_PATH);
    eprintln!("  GRAMMAR_PATH: {}", DEFAULT_GRAMMAR_PATH);
}

fn run_compare(spec_path: &str, grammar_path: &str) -> Result<bool, String> {
    println!("Comparing grammars...");
    println!("  EBNF Spec:    {}", spec_path);
    println!("  Tree-sitter:  {}\n", grammar_path);

    // Load EBNF grammar
    let ebnf = EbnfGrammar::from_spec(spec_path)
        .map_err(|e| format!("Failed to load EBNF spec: {}", e))?;

    // Load tree-sitter grammar
    let ts = TreeSitterGrammar::from_file(grammar_path)
        .map_err(|e| format!("Failed to load tree-sitter grammar: {}", e))?;

    // Compare
    let comparison = compare_grammars(&ebnf, &ts);
    let report = generate_report(&comparison);

    println!("{}", report);

    // Summary stats
    println!("{}", "-".repeat(60));
    println!("EBNF productions:     {}", ebnf.len());
    println!("Tree-sitter rules:    {} ({} hidden)", ts.len(), ts.rules.values().filter(|r| r.is_hidden).count());
    println!("EBNF keywords:        {}", ebnf.keywords.len());
    println!("Tree-sitter keywords: {}", ts.keywords.len());
    println!("{}", "-".repeat(60));

    Ok(comparison.is_in_sync())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        print_usage();
        process::exit(0);
    }

    // Determine command
    let command = if args.len() > 1 && args[1] == "compare" {
        "compare"
    } else if args.len() > 1 && args[1] == "validate" {
        "validate"
    } else if args.len() > 1 && !args[1].starts_with('-') {
        // Assume validate if a path is given
        "validate"
    } else if args.len() == 1 {
        "validate"
    } else {
        print_usage();
        process::exit(1);
    };

    match command {
        "compare" => {
            let spec_path = if args.len() > 2 { &args[2] } else { SPEC_PATH };
            let grammar_path = if args.len() > 3 {
                &args[3]
            } else {
                DEFAULT_GRAMMAR_PATH
            };

            match run_compare(spec_path, grammar_path) {
                Ok(in_sync) => {
                    if in_sync {
                        println!("\n✓ Grammars are in sync!");
                        process::exit(0);
                    } else {
                        println!("\n✗ Grammars have discrepancies.");
                        process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        }
        "validate" => {
            let spec_path = if args.len() > 1 && args[1] != "validate" {
                &args[1]
            } else if args.len() > 2 {
                &args[2]
            } else {
                SPEC_PATH
            };

            println!("Validating Lira implementation against specification...");
            println!("Specification: {}\n", spec_path);

            match run_validation(spec_path) {
                Ok(report) => {
                    if report.all_passed {
                        println!("\n✓ All validation checks passed!");
                        process::exit(0);
                    } else {
                        println!("\n✗ Some validation checks failed.");
                        process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        }
        _ => unreachable!(),
    }
}
