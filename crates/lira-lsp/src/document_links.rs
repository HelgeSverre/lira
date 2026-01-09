//! Document link support for Lira
//!
//! Makes import paths clickable to navigate to the imported file.

use regex::Regex;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::*;

/// Get document links for import statements
pub fn get_document_links(uri: &Url, content: &str) -> Vec<DocumentLink> {
    let mut links = Vec::new();

    // Get the directory of the current file
    let file_path = uri.to_file_path().ok();
    let base_dir = file_path.as_ref().and_then(|p| p.parent());

    // Find stdlib directory by walking up from current file
    let stdlib_dir = find_stdlib_dir(base_dir);

    let lines: Vec<&str> = content.lines().collect();

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx as u32;

        // Match import statements: import std.fs, import std.io.{File}
        if let Some(link) = parse_import_statement(line, line_num, base_dir, stdlib_dir.as_deref())
        {
            links.push(link);
        }

        // Match use statements: use std::fs, use std::io::{File}
        if let Some(link) = parse_use_statement(line, line_num, base_dir, stdlib_dir.as_deref()) {
            links.push(link);
        }
    }

    links
}

/// Parse an import statement and create a document link
fn parse_import_statement(
    line: &str,
    line_num: u32,
    base_dir: Option<&Path>,
    stdlib_dir: Option<&Path>,
) -> Option<DocumentLink> {
    // Match: import std.fs or import std.io.{File, Dir}
    let re = Regex::new(r"^\s*import\s+([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*)").ok()?;

    let caps = re.captures(line)?;
    let path_match = caps.get(1)?;
    let path_str = path_match.as_str();

    // Parse the path components
    let components: Vec<&str> = path_str.split('.').collect();

    // Resolve to file path
    let file_path = resolve_module_path(&components, base_dir, stdlib_dir)?;

    // Create the range for the import path
    let start_char = path_match.start() as u32;
    let end_char = path_match.end() as u32;

    let target_uri = Url::from_file_path(&file_path).ok()?;

    Some(DocumentLink {
        range: Range {
            start: Position {
                line: line_num,
                character: start_char,
            },
            end: Position {
                line: line_num,
                character: end_char,
            },
        },
        target: Some(target_uri),
        tooltip: Some(format!("Open {}", file_path.display())),
        data: None,
    })
}

/// Parse a use statement and create a document link
fn parse_use_statement(
    line: &str,
    line_num: u32,
    base_dir: Option<&Path>,
    stdlib_dir: Option<&Path>,
) -> Option<DocumentLink> {
    // Match: use std::fs or use std::io::{File}
    let re = Regex::new(r"^\s*use\s+([a-zA-Z_][a-zA-Z0-9_]*(?:::[a-zA-Z_][a-zA-Z0-9_]*)*)").ok()?;

    let caps = re.captures(line)?;
    let path_match = caps.get(1)?;
    let path_str = path_match.as_str();

    // Parse the path components (:: separated)
    let components: Vec<&str> = path_str.split("::").collect();

    // Resolve to file path
    let file_path = resolve_module_path(&components, base_dir, stdlib_dir)?;

    // Create the range for the use path
    let start_char = path_match.start() as u32;
    let end_char = path_match.end() as u32;

    let target_uri = Url::from_file_path(&file_path).ok()?;

    Some(DocumentLink {
        range: Range {
            start: Position {
                line: line_num,
                character: start_char,
            },
            end: Position {
                line: line_num,
                character: end_char,
            },
        },
        target: Some(target_uri),
        tooltip: Some(format!("Open {}", file_path.display())),
        data: None,
    })
}

/// Resolve a module path to a file path
fn resolve_module_path(
    components: &[&str],
    base_dir: Option<&Path>,
    stdlib_dir: Option<&Path>,
) -> Option<PathBuf> {
    if components.is_empty() {
        return None;
    }

    // Check if this is a stdlib import
    if components[0] == "std" {
        if let Some(stdlib) = stdlib_dir {
            return resolve_in_directory(stdlib, &components[1..]);
        }
        return None;
    }

    // Otherwise, resolve relative to base directory
    if let Some(base) = base_dir {
        return resolve_in_directory(base, components);
    }

    None
}

/// Resolve module components within a directory
fn resolve_in_directory(dir: &Path, components: &[&str]) -> Option<PathBuf> {
    if components.is_empty() {
        return None;
    }

    // Build the path from components
    let mut path = dir.to_path_buf();
    for (i, component) in components.iter().enumerate() {
        if i == components.len() - 1 {
            // Last component - try as file
            path.push(component);
        } else {
            // Intermediate component - directory
            path.push(component);
        }
    }

    // Try: path.li
    let mut li_path = path.clone();
    li_path.set_extension("li");
    if li_path.exists() {
        return Some(li_path);
    }

    // Try: path/mod.li
    let mod_path = path.join("mod.li");
    if mod_path.exists() {
        return Some(mod_path);
    }

    // Return the .li path even if it doesn't exist (for better UX)
    // The editor will show an error if the file doesn't exist
    Some(li_path)
}

/// Find the stdlib directory by walking up from the current file
fn find_stdlib_dir(start_dir: Option<&Path>) -> Option<PathBuf> {
    let mut current = start_dir?.to_path_buf();

    // Walk up looking for stdlib/ directory or Cargo.toml (project root)
    for _ in 0..10 {
        // Limit search depth
        // Check for stdlib directory
        let stdlib = current.join("stdlib");
        if stdlib.is_dir() {
            return Some(stdlib);
        }

        // Check for Cargo.toml (project root marker)
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            // Check if stdlib exists relative to Cargo.toml
            let stdlib = current.join("stdlib");
            if stdlib.is_dir() {
                return Some(stdlib);
            }
        }

        // Move up one directory
        if !current.pop() {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_import_path() {
        let re =
            Regex::new(r"^\s*import\s+([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*)").unwrap();

        let line = "import std.fs";
        let caps = re.captures(line).unwrap();
        assert_eq!(caps.get(1).unwrap().as_str(), "std.fs");

        let line = "import std.io.File";
        let caps = re.captures(line).unwrap();
        assert_eq!(caps.get(1).unwrap().as_str(), "std.io.File");
    }

    #[test]
    fn test_parse_use_path() {
        let re =
            Regex::new(r"^\s*use\s+([a-zA-Z_][a-zA-Z0-9_]*(?:::[a-zA-Z_][a-zA-Z0-9_]*)*)").unwrap();

        let line = "use std::fs";
        let caps = re.captures(line).unwrap();
        assert_eq!(caps.get(1).unwrap().as_str(), "std::fs");
    }

    #[test]
    fn test_resolve_in_directory() {
        // This test would need actual file system, so just test path construction
        let dir = Path::new("/fake/stdlib");
        let components = vec!["fs"];
        let result = resolve_in_directory(dir, &components);
        assert!(result.is_some());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("fs.li"));
    }
}
