//! Selection range support for Lira
//!
//! Provides smart expand/shrink selection (Ctrl+Shift+Arrow).

use crate::utils;
use tower_lsp::lsp_types::*;

/// Get selection ranges for the given positions
pub fn get_selection_ranges(content: &str, positions: &[Position]) -> Vec<SelectionRange> {
    positions
        .iter()
        .map(|pos| compute_selection_range(content, *pos))
        .collect()
}

fn compute_selection_range(content: &str, position: Position) -> SelectionRange {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return single_position_range(position);
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Build nested selection ranges from innermost to outermost:
    // 1. Word at cursor
    // 2. Current line (trimmed)
    // 3. Current block (braces)
    // 4. Current function/struct
    // 5. Entire document

    let mut ranges: Vec<Range> = Vec::new();

    // 1. Word at cursor
    if let Some(word_info) = utils::get_word_at_position(line, col) {
        ranges.push(Range {
            start: Position {
                line: position.line,
                character: word_info.start_col as u32,
            },
            end: Position {
                line: position.line,
                character: word_info.end_col as u32,
            },
        });
    }

    // 2. Current line (trimmed content)
    let trimmed_start = line.len() - line.trim_start().len();
    let trimmed_end = line.trim_end().len();
    if trimmed_end > trimmed_start {
        ranges.push(Range {
            start: Position {
                line: position.line,
                character: trimmed_start as u32,
            },
            end: Position {
                line: position.line,
                character: trimmed_end as u32,
            },
        });
    }

    // 3. Full line
    ranges.push(Range {
        start: Position {
            line: position.line,
            character: 0,
        },
        end: Position {
            line: position.line,
            character: line.len() as u32,
        },
    });

    // 4. Current block (find enclosing braces)
    if let Some(block_range) = find_enclosing_block(&lines, line_idx) {
        ranges.push(block_range);
    }

    // 5. Current function/struct/etc.
    if let Some(decl_range) = find_enclosing_declaration(&lines, line_idx) {
        ranges.push(decl_range);
    }

    // 6. Entire document
    let last_line = lines.len().saturating_sub(1);
    let last_col = lines.get(last_line).map_or(0, |l| l.len());
    ranges.push(Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: last_line as u32,
            character: last_col as u32,
        },
    });

    // Remove duplicates and sort by size (smallest first)
    ranges.sort_by_key(|r| range_size(r, &lines));
    ranges.dedup();

    // Build nested SelectionRange from innermost to outermost
    build_nested_selection_range(&ranges)
}

fn single_position_range(position: Position) -> SelectionRange {
    SelectionRange {
        range: Range {
            start: position,
            end: position,
        },
        parent: None,
    }
}

fn range_size(range: &Range, lines: &[&str]) -> usize {
    if range.start.line == range.end.line {
        (range.end.character - range.start.character) as usize
    } else {
        // Approximate size for multi-line ranges
        let mut size = 0;
        for line_idx in range.start.line..=range.end.line {
            if let Some(line) = lines.get(line_idx as usize) {
                size += line.len() + 1; // +1 for newline
            }
        }
        size
    }
}

fn build_nested_selection_range(ranges: &[Range]) -> SelectionRange {
    if ranges.is_empty() {
        return SelectionRange {
            range: Range::default(),
            parent: None,
        };
    }

    // Start from outermost and work inward
    let mut current: Option<Box<SelectionRange>> = None;

    for range in ranges.iter().rev() {
        current = Some(Box::new(SelectionRange {
            range: *range,
            parent: current,
        }));
    }

    *current.unwrap()
}

/// Find the enclosing brace block
fn find_enclosing_block(lines: &[&str], line_idx: usize) -> Option<Range> {
    // Search backward for opening brace
    let mut brace_depth = 0;
    let mut block_start: Option<(usize, usize)> = None;

    // First, scan from current line backward to find unmatched {
    for idx in (0..=line_idx).rev() {
        let line = lines[idx];
        let mut in_string = false;
        let mut prev_char = '\0';

        for (char_idx, ch) in line.chars().enumerate() {
            if ch == '"' && prev_char != '\\' {
                in_string = !in_string;
            }

            if !in_string {
                match ch {
                    '}' => brace_depth += 1,
                    '{' => {
                        if brace_depth == 0 {
                            block_start = Some((idx, char_idx));
                        } else {
                            brace_depth -= 1;
                        }
                    }
                    _ => {}
                }
            }
            prev_char = ch;
        }

        if block_start.is_some() {
            break;
        }
    }

    let (start_line, _start_char) = block_start?;

    // Now find matching closing brace
    brace_depth = 0;
    let mut in_string = false;
    let mut prev_char = '\0';

    for idx in start_line..lines.len() {
        let line = lines[idx];

        for (char_idx, ch) in line.chars().enumerate() {
            if ch == '"' && prev_char != '\\' {
                in_string = !in_string;
            }

            if !in_string {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            return Some(Range {
                                start: Position {
                                    line: start_line as u32,
                                    character: 0,
                                },
                                end: Position {
                                    line: idx as u32,
                                    character: (char_idx + 1) as u32,
                                },
                            });
                        }
                    }
                    _ => {}
                }
            }
            prev_char = ch;
        }
    }

    None
}

/// Find enclosing function/struct/class/etc.
fn find_enclosing_declaration(lines: &[&str], line_idx: usize) -> Option<Range> {
    // Search backward for declaration keywords at start of line
    let keywords = ["fn ", "struct ", "class ", "enum ", "trait ", "impl "];

    for idx in (0..=line_idx).rev() {
        let trimmed = lines[idx].trim_start();

        let is_declaration = keywords.iter().any(|kw| trimmed.starts_with(kw));
        let is_pub_declaration = trimmed.starts_with("pub ")
            && keywords
                .iter()
                .any(|kw| trimmed[4..].trim_start().starts_with(kw));

        if is_declaration || is_pub_declaration {
            // Found declaration start, now find the end (matching brace)
            if let Some(end_range) = find_enclosing_block(lines, idx) {
                return Some(Range {
                    start: Position {
                        line: idx as u32,
                        character: 0,
                    },
                    end: end_range.end,
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_selection() {
        let content = r#"fn main() {
    let x = 42
}"#;
        let ranges = get_selection_ranges(
            content,
            &[Position {
                line: 1,
                character: 8,
            }],
        );

        assert_eq!(ranges.len(), 1);
        let range = &ranges[0];

        // Should start with "x" (the word)
        assert_eq!(range.range.start.character, 8);
        assert_eq!(range.range.end.character, 9);

        // Should have parent (line, then block, then function, then document)
        assert!(range.parent.is_some());
    }

    #[test]
    fn test_nested_blocks() {
        let content = r#"fn main() {
    if true {
        let x = 1
    }
}"#;
        let ranges = get_selection_ranges(
            content,
            &[Position {
                line: 2,
                character: 12,
            }],
        );

        assert_eq!(ranges.len(), 1);
        // Should have multiple nesting levels
        let mut depth = 0;
        let mut current = Some(&ranges[0]);
        while let Some(r) = current {
            depth += 1;
            current = r.parent.as_ref().map(|b| b.as_ref());
        }

        // Word -> line trimmed -> line full -> if block -> fn block -> document
        assert!(depth >= 4);
    }

    #[test]
    fn test_multiple_positions() {
        let content = "let x = 1\nlet y = 2";
        let positions = vec![
            Position {
                line: 0,
                character: 4,
            },
            Position {
                line: 1,
                character: 4,
            },
        ];

        let ranges = get_selection_ranges(content, &positions);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn test_empty_content() {
        let content = "";
        let ranges = get_selection_ranges(
            content,
            &[Position {
                line: 0,
                character: 0,
            }],
        );

        assert_eq!(ranges.len(), 1);
    }
}
