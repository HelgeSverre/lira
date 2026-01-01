//! Lira Documentation Generator (lira-doc)
//!
//! Generates Markdown documentation from Lira source files.
//!
//! Usage:
//!   lira-doc generate <input.li> [-o <output.md>]
//!   lira-doc generate <dir/> [-o <output_dir/>] [--mdbook]
//!   lira-doc book [-o <output_dir/>]
//!   lira-doc --help

use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "book" => {
            // Generate combined book with stdlib and examples
            let mut output = String::from("docs/book");
            let mut title = String::from("Lira Programming Language");

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "-o" | "--output" => {
                        if i + 1 < args.len() {
                            output = args[i + 1].clone();
                            i += 2;
                        } else {
                            eprintln!("Error: -o requires an argument");
                            process::exit(1);
                        }
                    }
                    "--title" => {
                        if i + 1 < args.len() {
                            title = args[i + 1].clone();
                            i += 2;
                        } else {
                            eprintln!("Error: --title requires an argument");
                            process::exit(1);
                        }
                    }
                    arg => {
                        eprintln!("Error: Unknown option: {}", arg);
                        print_usage();
                        process::exit(1);
                    }
                }
            }

            println!("Generating Lira documentation book -> {}", output);

            // Define sections: (directory, section_name)
            let inputs: Vec<(&str, &str)> =
                vec![("stdlib", "Standard Library"), ("examples", "Examples")];

            match lira_doc::generate_mdbook_multi(&inputs, &output, &title) {
                Ok(count) => {
                    println!("Generated book with {} files", count);
                    println!("\nTo build the book:");
                    println!("  cd {} && mdbook build", output);
                    println!("\nTo serve locally:");
                    println!("  cd {} && mdbook serve", output);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        "generate" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input file or directory");
                print_usage();
                process::exit(1);
            }

            let input = &args[2];
            let input_path = Path::new(input);

            // Parse options
            let mut output: Option<String> = None;
            let mut mdbook = false;
            let mut title: Option<String> = None;

            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "-o" | "--output" => {
                        if i + 1 < args.len() {
                            output = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            eprintln!("Error: -o requires an argument");
                            process::exit(1);
                        }
                    }
                    "--mdbook" => {
                        mdbook = true;
                        i += 1;
                    }
                    "--title" => {
                        if i + 1 < args.len() {
                            title = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            eprintln!("Error: --title requires an argument");
                            process::exit(1);
                        }
                    }
                    arg => {
                        eprintln!("Error: Unknown option: {}", arg);
                        print_usage();
                        process::exit(1);
                    }
                }
            }

            if input_path.is_dir() {
                // Generate docs for entire directory
                let default_output = if mdbook {
                    format!("{}_book", input.trim_end_matches('/'))
                } else {
                    format!("{}_docs", input.trim_end_matches('/'))
                };
                let output_dir = output.unwrap_or(default_output);

                let dir_name = input_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Documentation");

                let book_title =
                    title.unwrap_or_else(|| format!("Lira {} Documentation", dir_name));

                if mdbook {
                    println!(
                        "Generating mdBook documentation for {} -> {}",
                        input, output_dir
                    );

                    match lira_doc::generate_mdbook(input, &output_dir, &book_title) {
                        Ok(count) => {
                            println!("Generated mdBook with {} modules", count);
                            println!("\nTo build the book:");
                            println!("  cd {} && mdbook build", output_dir);
                            println!("\nTo serve locally:");
                            println!("  cd {} && mdbook serve", output_dir);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            process::exit(1);
                        }
                    }
                } else {
                    println!("Generating documentation for {} -> {}", input, output_dir);

                    match lira_doc::generate_dir(input, &output_dir) {
                        Ok(count) => {
                            // Generate index
                            let index_path = Path::new(&output_dir).join("index.md");

                            if let Ok(index) = lira_doc::generate_index(&output_dir, dir_name) {
                                let _ = fs::write(&index_path, index);
                            }

                            println!("Generated documentation for {} modules", count);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            process::exit(1);
                        }
                    }
                }
            } else if input_path.is_file() {
                if mdbook {
                    eprintln!("Error: --mdbook requires a directory input");
                    process::exit(1);
                }

                // Generate docs for single file
                let output_file = output.unwrap_or_else(|| {
                    format!(
                        "{}.md",
                        input_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("output")
                    )
                });

                println!("Generating documentation for {} -> {}", input, output_file);

                match lira_doc::generate_file(input, &output_file) {
                    Ok(()) => println!("Documentation generated successfully"),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        process::exit(1);
                    }
                }
            } else {
                eprintln!("Error: {} does not exist", input);
                process::exit(1);
            }
        }
        "--help" | "-h" => print_usage(),
        "--version" | "-V" => {
            println!("lira-doc {}", env!("CARGO_PKG_VERSION"));
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
        r#"Lira Documentation Generator (lira-doc)

Usage:
  lira-doc generate <input.li> [-o <output.md>]   Generate docs for a file
  lira-doc generate <dir/> [-o <output_dir/>]     Generate docs for a directory
  lira-doc generate <dir/> --mdbook [-o <out/>]   Generate mdBook structure
  lira-doc book [-o <output_dir/>]                Generate combined book (stdlib + examples)
  lira-doc --help                                 Show this help
  lira-doc --version                              Show version

Options:
  -o, --output <path>    Output file or directory
  --mdbook               Generate mdBook-compatible output (directory only)
  --title <title>        Book title (default: "Lira Programming Language")

Examples:
  lira-doc generate stdlib/strings.li -o docs/strings.md
  lira-doc generate stdlib/ -o docs/stdlib/
  lira-doc generate stdlib/ --mdbook -o docs/lira-book/
  lira-doc book -o docs/book/
"#
    );
}
