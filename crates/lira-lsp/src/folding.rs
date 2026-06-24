//! Folding range support for Lira
//!
//! Provides code folding for blocks, functions, structs, and other constructs.

use tower_lsp::lsp_types::*;

/// Get folding ranges for a document
pub fn get_folding_ranges(content: &str) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Track brace-based blocks
    ranges.extend(find_brace_blocks(&lines));

    // Track multi-line comments
    ranges.extend(find_comment_blocks(&lines));

    // Track consecutive import groups
    ranges.extend(find_import_groups(&lines));

    ranges
}

/// Find foldable blocks delimited by braces
fn find_brace_blocks(lines: &[&str]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut stack: Vec<(u32, Option<FoldingRangeKind>)> = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx as u32;
        let trimmed = line.trim();

        // Determine the kind of block starting on this line
        let starts_foldable = trimmed.starts_with("fn ")
            || trimmed.contains(" fn ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("if ")
            || trimmed.starts_with("else ")
            || trimmed.starts_with("while ")
            || trimmed.starts_with("for ")
            || trimmed.starts_with("loop ")
            || trimmed.starts_with("match ");
        let kind = if starts_foldable {
            Some(FoldingRangeKind::Region)
        } else {
            None
        };

        // Count braces on this line
        let mut in_string = false;
        let mut in_char = false;
        let mut escape_next = false;
        let chars: Vec<char> = line.chars().collect();

        for i in 0..chars.len() {
            let c = chars[i];

            if escape_next {
                escape_next = false;
                continue;
            }

            if c == '\\' {
                escape_next = true;
                continue;
            }

            if c == '"' && !in_char {
                in_string = !in_string;
                continue;
            }

            if c == '\'' && !in_string {
                in_char = !in_char;
                continue;
            }

            if in_string || in_char {
                continue;
            }

            // Check for line comments
            if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                break; // Rest of line is comment
            }

            if c == '{' {
                // Push the line number and kind onto the stack
                stack.push((line_num, kind.clone()));
            } else if c == '}' {
                if let Some((start_line, block_kind)) = stack.pop() {
                    // Only create a fold if it spans multiple lines
                    if line_num > start_line {
                        ranges.push(FoldingRange {
                            start_line,
                            start_character: None,
                            end_line: line_num,
                            end_character: None,
                            kind: block_kind,
                            collapsed_text: None,
                        });
                    }
                }
            }
        }
    }

    ranges
}

/// Find multi-line comment blocks
fn find_comment_blocks(lines: &[&str]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut in_block_comment = false;
    let mut comment_start: u32 = 0;

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx as u32;

        if !in_block_comment {
            if let Some(start_pos) = line.find("/*") {
                // Check if comment ends on same line
                if let Some(end_pos) = line[start_pos + 2..].find("*/") {
                    // Single line block comment, skip
                    let _ = end_pos;
                } else {
                    in_block_comment = true;
                    comment_start = line_num;
                }
            }
        } else if line.contains("*/") {
            in_block_comment = false;
            if line_num > comment_start {
                ranges.push(FoldingRange {
                    start_line: comment_start,
                    start_character: None,
                    end_line: line_num,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Comment),
                    collapsed_text: None,
                });
            }
        }
    }

    ranges
}

/// Find consecutive import statement groups
fn find_import_groups(lines: &[&str]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut import_start: Option<u32> = None;
    let mut last_import_line: u32 = 0;

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx as u32;
        let trimmed = line.trim();

        let is_import = trimmed.starts_with("import ") || trimmed.starts_with("use ");

        if is_import {
            if import_start.is_none() {
                import_start = Some(line_num);
            }
            last_import_line = line_num;
        } else if !trimmed.is_empty() && import_start.is_some() {
            // End of import group (non-empty, non-import line)
            let start = import_start.unwrap();
            if last_import_line > start {
                ranges.push(FoldingRange {
                    start_line: start,
                    start_character: None,
                    end_line: last_import_line,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Imports),
                    collapsed_text: None,
                });
            }
            import_start = None;
        }
    }

    // Handle import group at end of file
    if let Some(start) = import_start {
        if last_import_line > start {
            ranges.push(FoldingRange {
                start_line: start,
                start_character: None,
                end_line: last_import_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Imports),
                collapsed_text: None,
            });
        }
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_folding() {
        let content = r#"fn main() {
    println("hello")
}

fn other() {
    let x = 1
}"#;
        let ranges = get_folding_ranges(content);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 2);
        assert_eq!(ranges[1].start_line, 4);
        assert_eq!(ranges[1].end_line, 6);
    }

    #[test]
    fn test_nested_folding() {
        let content = r#"fn main() {
    if true {
        println("yes")
    }
}"#;
        let ranges = get_folding_ranges(content);
        assert_eq!(ranges.len(), 2);
        // Inner if block
        assert_eq!(ranges[0].start_line, 1);
        assert_eq!(ranges[0].end_line, 3);
        // Outer function
        assert_eq!(ranges[1].start_line, 0);
        assert_eq!(ranges[1].end_line, 4);
    }

    #[test]
    fn test_import_group_folding() {
        let content = r#"import std.io
import std.fs
import std.math

fn main() {
}"#;
        let ranges = get_folding_ranges(content);
        // Should have import group fold and function fold
        let import_fold = ranges
            .iter()
            .find(|r| r.kind == Some(FoldingRangeKind::Imports));
        assert!(import_fold.is_some());
        let import_fold = import_fold.unwrap();
        assert_eq!(import_fold.start_line, 0);
        assert_eq!(import_fold.end_line, 2);
    }

    #[test]
    fn test_block_comment_folding() {
        let content = r#"/*
 * Multi-line
 * comment
 */
fn main() {
}"#;
        let ranges = get_folding_ranges(content);
        let comment_fold = ranges
            .iter()
            .find(|r| r.kind == Some(FoldingRangeKind::Comment));
        assert!(comment_fold.is_some());
        let comment_fold = comment_fold.unwrap();
        assert_eq!(comment_fold.start_line, 0);
        assert_eq!(comment_fold.end_line, 3);
    }

    #[test]
    fn test_struct_folding() {
        let content = r#"struct Point {
    x: float,
    y: float,
}"#;
        let ranges = get_folding_ranges(content);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 3);
    }

    #[test]
    fn test_brace_in_string_ignored() {
        let content = r#"fn main() {
    let s = "{ not a block }"
}"#;
        let ranges = get_folding_ranges(content);
        // Should only fold the function, not the braces in string
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 2);
    }
}
