//! Lira Compiler (lirac)
//!
//! Compiles Lira source files (.li) to bytecode (.lic).
//!
//! Usage:
//!   lirac compile <input.li> -o <output.lic>
//!   lirac check <input.li>
//!   lirac --help

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "compile" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                print_usage();
                process::exit(1);
            }

            let input = &args[2];
            let output = if args.len() >= 5 && args[3] == "-o" {
                args[4].clone()
            } else {
                format!("{}.lic", input.trim_end_matches(".li"))
            };

            println!("Compiling {} -> {}", input, output);

            match lirac::compile_file(input, &output) {
                Ok(()) => println!("Compilation successful"),
                Err(e) => {
                    eprintln!("Compilation failed: {}", e);
                    process::exit(1);
                }
            }
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file");
                print_usage();
                process::exit(1);
            }

            let input = &args[2];
            println!("Checking {}", input);

            match lirac::check_file(input) {
                Ok(()) => println!("No errors found"),
                Err(e) => {
                    eprintln!("Check failed: {}", e);
                    process::exit(1);
                }
            }
        }
        "--help" | "-h" => print_usage(),
        "--version" | "-V" => {
            println!("lirac {}", env!("CARGO_PKG_VERSION"));
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
        r#"Lira Compiler (lirac)

Usage:
  lirac compile <input.li> [-o <output.lic>]   Compile source to bytecode
  lirac check <input.li>                       Check source for errors
  lirac --help                                 Show this help
  lirac --version                              Show version

Examples:
  lirac compile hello.li -o hello.lic
  lirac check main.li
"#
    );
}
