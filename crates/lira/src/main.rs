//! Lira CLI - Unified interface for Lira programming language
//!
//! Commands:
//!   lira run <file.li>                 Compile and execute Lira source
//!   lira compile <file.li> [-o <out>]  Compile to bytecode
//!   lira check <file.li>               Type check without compiling
//!   lira --help                        Show help
//!   lira --version                     Show version

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let result = match args[1].as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira run <file.li>");
                process::exit(1);
            }
            run_command(&args[2])
        }
        "compile" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira compile <file.li> [-o <output.lic>]");
                process::exit(1);
            }
            let output = parse_output_arg(&args, &args[2]);
            compile_command(&args[2], &output).map(|_| 0)
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                eprintln!("Usage: lira check <file.li>");
                process::exit(1);
            }
            check_command(&args[2]).map(|_| 0)
        }
        "repl" => {
            eprintln!("The REPL is currently under development.");
            eprintln!("See docs/60-lira-repl.md for the specification and roadmap.");
            eprintln!();
            eprintln!("In the meantime, use 'lira run <file.li>' to execute Lira programs.");
            process::exit(1);
        }
        "--help" | "-h" | "help" => {
            print_usage();
            process::exit(0);
        }
        "--version" | "-V" | "version" => {
            println!("lira {}", env!("CARGO_PKG_VERSION"));
            process::exit(0);
        }
        cmd => {
            eprintln!("Unknown command: {}", cmd);
            eprintln!();
            print_usage();
            process::exit(1);
        }
    };

    match result {
        Ok(exit_code) => process::exit(exit_code),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}

/// Compile and execute a Lira source file
fn run_command(file: &str) -> Result<i32, String> {
    // Read source
    let source = fs::read_to_string(file).map_err(|e| format!("Failed to read {}: {}", file, e))?;

    // Compile in-memory with import resolution
    let bytecode = lirac::compile_with_imports(file, &source)?;

    // Execute and return exit code
    liravm::run(&bytecode)
}

/// Compile a Lira source file to bytecode
fn compile_command(input: &str, output: &str) -> Result<(), String> {
    lirac::compile_file(input, output)?;
    println!("Compiled {} -> {}", input, output);
    Ok(())
}

/// Type check a Lira source file
fn check_command(file: &str) -> Result<(), String> {
    lirac::check_file(file)?;
    println!("No errors found in {}", file);
    Ok(())
}

/// Parse -o/--output argument, defaulting to input file with .lic extension
fn parse_output_arg(args: &[String], input: &str) -> String {
    for i in 3..args.len() {
        if (args[i] == "-o" || args[i] == "--output") && i + 1 < args.len() {
            return args[i + 1].clone();
        }
    }
    // Default: replace .li with .lic
    if input.ends_with(".li") {
        format!("{}c", input)
    } else {
        format!("{}.lic", input)
    }
}

fn print_usage() {
    println!(
        r#"Lira Programming Language CLI

USAGE:
    lira <COMMAND> [OPTIONS]

COMMANDS:
    run <file.li>              Compile and execute a Lira program
    compile <file.li> [OPTS]   Compile source to bytecode
    check <file.li>            Type check source without compiling
    repl                       Interactive REPL (coming soon)
    help                       Show this help message
    version                    Show version information

COMPILE OPTIONS:
    -o, --output <file.lic>    Output bytecode file (default: <input>.lic)

EXAMPLES:
    lira run examples/hello.li
    lira compile main.li -o app.lic
    lira check src/main.li"#
    );
}
