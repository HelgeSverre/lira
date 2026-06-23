//! Lira Compiler Library
//!
//! This library provides the core compilation pipeline for Lira:
//! - Lexer: Tokenizes source code
//! - Parser: Builds AST from tokens
//! - Type Checker: Validates types and infers missing annotations
//! - Code Generator: Produces bytecode for the VM
//! - Module Loader: Handles imports and multi-file compilation

pub mod ast;
pub mod checker;
pub mod codegen;
pub mod errors;
pub mod ids;
pub mod lexer;
pub mod module_loader;
pub mod parser;
pub mod sema;

use std::fs;

/// Compile a Lira source file to bytecode
pub fn compile_file(input: &str, output: &str) -> Result<(), String> {
    // Read source file
    let source =
        fs::read_to_string(input).map_err(|e| format!("Failed to read {}: {}", input, e))?;

    // Compile source with module loading support
    let bytecode = compile_with_imports(input, &source)?;

    // Write bytecode
    fs::write(output, bytecode).map_err(|e| format!("Failed to write {}: {}", output, e))?;

    Ok(())
}

/// Check a Lira source file for errors without generating bytecode
pub fn check_file(input: &str) -> Result<(), String> {
    let source =
        fs::read_to_string(input).map_err(|e| format!("Failed to read {}: {}", input, e))?;

    check_with_imports(input, &source)
}

/// Compile Lira source code to bytecode (without import resolution)
pub fn compile(source: &str) -> Result<Vec<u8>, String> {
    // Phase 1: Lexing
    let tokens = lexer::tokenize(source)?;

    // Phase 2: Parsing
    let ast = parser::parse(&tokens)?;

    // Phase 3: Type checking
    let checked = checker::check(&ast)?;

    // Phase 4: Code generation
    let bytecode = codegen::generate(&checked)?;

    Ok(bytecode)
}

/// Compile Lira source code with import resolution
pub fn compile_with_imports(source_file: &str, source: &str) -> Result<Vec<u8>, String> {
    // Phase 1: Lexing
    let tokens = lexer::tokenize(source)?;

    // Phase 2: Parsing
    let ast = parser::parse(&tokens)?;

    // Phase 2.5: Process imports
    let mut loader = module_loader::ModuleLoader::new(source_file);
    let merged_ast = loader.process_imports(&ast)?;

    // Phase 3: Type checking
    let checked = checker::check(&merged_ast)?;

    // Phase 4: Code generation
    let bytecode = codegen::generate(&checked)?;

    Ok(bytecode)
}

/// Check Lira source code for errors
pub fn check(source: &str) -> Result<(), String> {
    // Phase 1: Lexing
    let tokens = lexer::tokenize(source)?;

    // Phase 2: Parsing
    let ast = parser::parse(&tokens)?;

    // Phase 3: Type checking
    checker::check(&ast)?;

    Ok(())
}

/// Check Lira source code for errors with import resolution
pub fn check_with_imports(source_file: &str, source: &str) -> Result<(), String> {
    // Phase 1: Lexing
    let tokens = lexer::tokenize(source)?;

    // Phase 2: Parsing
    let ast = parser::parse(&tokens)?;

    // Phase 2.5: Process imports
    let mut loader = module_loader::ModuleLoader::new(source_file);
    let merged_ast = loader.process_imports(&ast)?;

    // Phase 3: Type checking
    checker::check(&merged_ast)?;

    Ok(())
}

/// A diagnostic produced while checking a document: a 1-indexed source position
/// and a message with no embedded location prefix. Positions of 0 mean the
/// origin phase (lexer/parser/module loader) did not report a usable location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// Check Lira source (with import resolution) and return structured diagnostics
/// instead of a flattened error string. Each checker error keeps its span, so
/// consumers do not have to re-parse "line:column: message" text.
pub fn check_with_imports_diagnostics(source_file: &str, source: &str) -> Vec<Diagnostic> {
    // Lexing, parsing and import resolution still surface a single string error;
    // recover a leading "line:column: " prefix where present.
    let tokens = match lexer::tokenize(source) {
        Ok(tokens) => tokens,
        Err(e) => return vec![string_to_diagnostic(&e)],
    };
    let ast = match parser::parse(&tokens) {
        Ok(ast) => ast,
        Err(e) => return vec![string_to_diagnostic(&e)],
    };
    let mut loader = module_loader::ModuleLoader::new(source_file);
    let merged_ast = match loader.process_imports(&ast) {
        Ok(ast) => ast,
        Err(e) => return vec![string_to_diagnostic(&e)],
    };

    let (_checked, structured) = checker::check_collecting(&merged_ast);
    let diagnostics: Vec<Diagnostic> = structured
        .iter()
        .map(|err| {
            let span = err.span();
            Diagnostic {
                line: span.line,
                column: span.column,
                message: err.body(),
            }
        })
        .collect();
    diagnostics
}

/// Turn a legacy string error into a [`Diagnostic`], recovering a leading
/// "line:column: " position prefix when present.
fn string_to_diagnostic(error: &str) -> Diagnostic {
    let mut parts = error.splitn(3, ':');
    if let (Some(line), Some(col), Some(rest)) = (parts.next(), parts.next(), parts.next()) {
        if let (Ok(line), Ok(column)) = (line.trim().parse(), col.trim().parse()) {
            return Diagnostic {
                line,
                column,
                message: rest.trim().to_string(),
            };
        }
    }
    Diagnostic {
        line: 0,
        column: 0,
        message: error.to_string(),
    }
}

/// Parse a Lira source file and return the AST
pub fn parse_file(input: &str) -> Result<ast::Program, String> {
    let source =
        fs::read_to_string(input).map_err(|e| format!("Failed to read {}: {}", input, e))?;

    parse_source(&source)
}

/// Parse Lira source code and return the AST
pub fn parse_source(source: &str) -> Result<ast::Program, String> {
    let tokens = lexer::tokenize(source)?;
    parser::parse(&tokens)
}

/// Parse a Lira source file and return the AST as JSON
#[cfg(feature = "serde")]
pub fn parse_file_json(input: &str) -> Result<String, String> {
    let ast = parse_file(input)?;
    serde_json::to_string_pretty(&ast).map_err(|e| format!("Failed to serialize AST: {}", e))
}
