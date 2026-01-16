//! Lira Documentation Generator Library
//!
//! This library generates Markdown documentation from Lira source files.
//! It extracts documentation comments and correlates them with AST
//! declarations to produce structured documentation.

pub mod extractor;
pub mod generator;
pub mod types;

use std::fs;
use std::path::Path;

/// Options for documentation generation
#[derive(Default)]
pub struct DocOptions {
    /// Generate mdBook-compatible output
    pub mdbook: bool,
    /// Book title for mdBook
    pub title: Option<String>,
}

/// Generate documentation for a single Lira source file
pub fn generate_file(input: &str, output: &str) -> Result<(), String> {
    let source =
        fs::read_to_string(input).map_err(|e| format!("Failed to read {}: {}", input, e))?;

    let markdown = generate_markdown(input, &source)?;

    fs::write(output, markdown).map_err(|e| format!("Failed to write {}: {}", output, e))?;

    Ok(())
}

/// Generate Markdown documentation from source code
pub fn generate_markdown(source_path: &str, source: &str) -> Result<String, String> {
    let module_doc = extractor::extract_docs(source_path, source)?;
    Ok(generator::generate_markdown(&module_doc))
}

/// Generate documentation for a directory of Lira source files
pub fn generate_dir(input_dir: &str, output_dir: &str) -> Result<usize, String> {
    let input_path = Path::new(input_dir);
    let output_path = Path::new(output_dir);

    if !input_path.is_dir() {
        return Err(format!("{} is not a directory", input_dir));
    }

    // Create output directory if it doesn't exist
    fs::create_dir_all(output_path)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    let mut count = 0;

    // Find all .li files
    let entries = fs::read_dir(input_path)
        .map_err(|e| format!("Failed to read directory {}: {}", input_dir, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("li") {
            let file_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            let output_file = output_path.join(format!("{}.md", file_name));

            match generate_file(
                path.to_str().unwrap_or(""),
                output_file.to_str().unwrap_or(""),
            ) {
                Ok(()) => {
                    count += 1;
                }
                Err(e) => {
                    eprintln!("Warning: Failed to generate docs for {:?}: {}", path, e);
                }
            }
        }
    }

    Ok(count)
}

/// Generate an index page for a directory of documentation
pub fn generate_index(docs_dir: &str, title: &str) -> Result<String, String> {
    let path = Path::new(docs_dir);

    if !path.is_dir() {
        return Err(format!("{} is not a directory", docs_dir));
    }

    let mut output = format!("# {}\n\n", title);
    output.push_str("## Modules\n\n");

    let mut entries: Vec<_> = fs::read_dir(path)
        .map_err(|e| format!("Failed to read directory: {}", e))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().and_then(|e| e.to_str()) == Some("md")
                && e.file_name() != "index.md"
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let file_name = entry
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        output.push_str(&format!("- [{}]({}.md)\n", file_name, file_name));
    }

    Ok(output)
}

/// Generate documentation with mdBook structure
pub fn generate_mdbook(input_dir: &str, output_dir: &str, title: &str) -> Result<usize, String> {
    let input_path = Path::new(input_dir);
    let output_path = Path::new(output_dir);
    let src_path = output_path.join("src");

    if !input_path.is_dir() {
        return Err(format!("{} is not a directory", input_dir));
    }

    // Create output directories
    fs::create_dir_all(&src_path)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    // Generate book.toml
    let book_toml = generate_book_toml(title);
    fs::write(output_path.join("book.toml"), book_toml)
        .map_err(|e| format!("Failed to write book.toml: {}", e))?;

    // Collect module names for SUMMARY.md
    let mut modules: Vec<String> = Vec::new();

    // Find all .li files and generate docs
    let entries = fs::read_dir(input_path)
        .map_err(|e| format!("Failed to read directory {}: {}", input_dir, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("li") {
            let file_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let output_file = src_path.join(format!("{}.md", file_name));

            match generate_file(
                path.to_str().unwrap_or(""),
                output_file.to_str().unwrap_or(""),
            ) {
                Ok(()) => {
                    modules.push(file_name);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to generate docs for {:?}: {}", path, e);
                }
            }
        }
    }

    // Sort modules alphabetically
    modules.sort();

    // Generate SUMMARY.md
    let summary = generate_summary(title, &modules);
    fs::write(src_path.join("SUMMARY.md"), summary)
        .map_err(|e| format!("Failed to write SUMMARY.md: {}", e))?;

    // Generate introduction page
    let intro = generate_intro(title, &modules);
    fs::write(src_path.join("introduction.md"), intro)
        .map_err(|e| format!("Failed to write introduction.md: {}", e))?;

    Ok(modules.len())
}

/// Generate book.toml for mdBook
fn generate_book_toml(title: &str) -> String {
    format!(
        r#"[book]
title = "{}"
authors = ["Lira Contributors"]
language = "en"

[build]
build-dir = "book"

[output.html]
default-theme = "ayu"
preferred-dark-theme = "ayu"
git-repository-url = "https://github.com/HelgeSverre/lira"
additional-css = ["theme/lira.css"]
"#,
        title
    )
}

/// Generate SUMMARY.md for mdBook
fn generate_summary(title: &str, modules: &[String]) -> String {
    let mut output = format!("# {}\n\n", title);
    output.push_str("[Introduction](introduction.md)\n\n");
    output.push_str("# Modules\n\n");

    for module in modules {
        output.push_str(&format!("- [{}]({}.md)\n", module, module));
    }

    output
}

/// Generate introduction page for mdBook
fn generate_intro(title: &str, modules: &[String]) -> String {
    let mut output = format!("# {}\n\n", title);
    output.push_str("Welcome to the Lira documentation.\n\n");
    output.push_str("## Available Modules\n\n");

    for module in modules {
        output.push_str(&format!("- [{}]({}.md)\n", module, module));
    }

    output.push_str("\n## Building the Documentation\n\n");
    output.push_str("This documentation was generated with `lira-doc` and can be built with [mdBook](https://rust-lang.github.io/mdBook/):\n\n");
    output.push_str("```bash\n");
    output.push_str("mdbook build\n");
    output.push_str("mdbook serve  # For local preview\n");
    output.push_str("```\n");

    output
}

/// A section of documentation with a name and list of modules
#[derive(Debug, Clone)]
pub struct DocSection {
    pub name: String,
    pub modules: Vec<String>,
}

/// Generate mdBook from multiple input directories
pub fn generate_mdbook_multi(
    inputs: &[(&str, &str)], // (directory, section_name)
    output_dir: &str,
    title: &str,
) -> Result<usize, String> {
    let output_path = Path::new(output_dir);
    let src_path = output_path.join("src");

    // Create output directories
    fs::create_dir_all(&src_path)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    // Generate book.toml
    let book_toml = generate_book_toml(title);
    fs::write(output_path.join("book.toml"), book_toml)
        .map_err(|e| format!("Failed to write book.toml: {}", e))?;

    // Collect sections
    let mut sections: Vec<DocSection> = Vec::new();
    let mut total_count = 0;

    for (input_dir, section_name) in inputs {
        let input_path = Path::new(input_dir);

        if !input_path.is_dir() {
            eprintln!("Warning: {} is not a directory, skipping", input_dir);
            continue;
        }

        // Create section subdirectory
        let section_slug = section_name.to_lowercase().replace(' ', "-");
        let section_path = src_path.join(&section_slug);
        fs::create_dir_all(&section_path)
            .map_err(|e| format!("Failed to create section directory: {}", e))?;

        let mut modules: Vec<String> = Vec::new();

        // Find all .li files
        let entries = fs::read_dir(input_path)
            .map_err(|e| format!("Failed to read directory {}: {}", input_dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("li") {
                let file_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let output_file = section_path.join(format!("{}.md", file_name));

                match generate_file(
                    path.to_str().unwrap_or(""),
                    output_file.to_str().unwrap_or(""),
                ) {
                    Ok(()) => {
                        modules.push(file_name);
                        total_count += 1;
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to generate docs for {:?}: {}", path, e);
                    }
                }
            }
        }

        modules.sort();

        if !modules.is_empty() {
            sections.push(DocSection {
                name: section_name.to_string(),
                modules,
            });
        }
    }

    // Generate SUMMARY.md with sections
    let summary = generate_summary_multi(title, &sections);
    fs::write(src_path.join("SUMMARY.md"), summary)
        .map_err(|e| format!("Failed to write SUMMARY.md: {}", e))?;

    // Generate introduction page
    let intro = generate_intro_multi(title, &sections);
    fs::write(src_path.join("introduction.md"), intro)
        .map_err(|e| format!("Failed to write introduction.md: {}", e))?;

    Ok(total_count)
}

/// Generate SUMMARY.md with multiple sections
fn generate_summary_multi(title: &str, sections: &[DocSection]) -> String {
    let mut output = format!("# {}\n\n", title);
    output.push_str("[Introduction](introduction.md)\n\n");

    for section in sections {
        let section_slug = section.name.to_lowercase().replace(' ', "-");
        output.push_str(&format!("# {}\n\n", section.name));

        for module in &section.modules {
            output.push_str(&format!("- [{}]({}/{}.md)\n", module, section_slug, module));
        }
        output.push('\n');
    }

    output
}

/// Generate introduction page with multiple sections
fn generate_intro_multi(title: &str, sections: &[DocSection]) -> String {
    let mut output = format!("# {}\n\n", title);
    output.push_str("Welcome to the Lira programming language documentation.\n\n");
    output.push_str("Lira is a modern systems programming language with fiber concurrency, ");
    output.push_str("pattern matching, and a custom bytecode VM.\n\n");

    for section in sections {
        let section_slug = section.name.to_lowercase().replace(' ', "-");
        output.push_str(&format!("## {}\n\n", section.name));

        for module in &section.modules {
            output.push_str(&format!("- [{}]({}/{}.md)\n", module, section_slug, module));
        }
        output.push('\n');
    }

    output.push_str("## Building the Documentation\n\n");
    output.push_str("```bash\n");
    output.push_str("mdbook build\n");
    output.push_str("mdbook serve  # For local preview\n");
    output.push_str("```\n\n");

    output.push_str("## Rust API Documentation\n\n");
    output.push_str("For the Rust implementation details, generate API docs with:\n\n");
    output.push_str("```bash\n");
    output.push_str("cargo doc --open\n");
    output.push_str("```\n");

    output
}
