//! Lira Virtual Machine (liravm)
//!
//! Executes Lira bytecode files (.lic).
//!
//! Usage:
//!   liravm run <file.lic>
//!   liravm --help

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: Missing bytecode file");
                print_usage();
                process::exit(1);
            }

            let file = &args[2];
            println!("Running {}", file);

            match liravm::run_file(file) {
                Ok(exit_code) => process::exit(exit_code),
                Err(e) => {
                    eprintln!("Runtime error: {}", e);
                    process::exit(1);
                }
            }
        }
        "run-debug" => {
            if args.len() < 3 {
                eprintln!("Error: Missing bytecode file");
                print_usage();
                process::exit(1);
            }

            let file = &args[2];
            println!("Running {} (debug mode)", file);

            let bytecode = std::fs::read(file).expect("Failed to read bytecode");
            match liravm::run_with_debug(&bytecode) {
                Ok(exit_code) => process::exit(exit_code),
                Err(e) => {
                    eprintln!("Runtime error: {}", e);
                    process::exit(1);
                }
            }
        }
        "--help" | "-h" => print_usage(),
        "--version" | "-V" => {
            println!("liravm {}", env!("CARGO_PKG_VERSION"));
        }
        cmd => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            process::exit(1);
        }
    }
}

fn print_usage() {
    println!(
        r#"Lira Virtual Machine (liravm)

Usage:
  liravm run <file.lic>    Execute bytecode file
  liravm --help             Show this help
  liravm --version          Show version

Examples:
  liravm run hello.lic
"#
    );
}
