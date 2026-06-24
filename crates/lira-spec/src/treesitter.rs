//! Tree-sitter Grammar Comparison
//!
//! Compares the formal EBNF specification against the tree-sitter grammar
//! to ensure they remain in sync.

use crate::ebnf::EbnfGrammar;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during tree-sitter grammar parsing
#[derive(Error, Debug)]
pub enum TreeSitterError {
    #[error("Failed to read grammar file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse grammar: {0}")]
    ParseError(String),
}

/// A rule extracted from tree-sitter grammar.js
#[derive(Debug, Clone)]
pub struct TreeSitterRule {
    /// Rule name
    pub name: String,
    /// Whether it's a hidden rule (starts with _)
    pub is_hidden: bool,
    /// Rule definition (as raw string for now)
    pub definition: String,
    /// Keywords found in the rule
    pub keywords: Vec<String>,
    /// Operators found in the rule
    pub operators: Vec<String>,
    /// References to other rules
    pub references: Vec<String>,
}

/// Tree-sitter grammar extracted from grammar.js
#[derive(Debug, Clone)]
pub struct TreeSitterGrammar {
    /// All rules
    pub rules: HashMap<String, TreeSitterRule>,
    /// Order of rules (for stable iteration)
    pub order: Vec<String>,
    /// Precedence constants
    pub precedences: HashMap<String, i32>,
    /// Conflicts defined
    pub conflicts: Vec<Vec<String>>,
    /// Keywords extracted
    pub keywords: HashSet<String>,
    /// Operators extracted
    pub operators: HashSet<String>,
}

impl TreeSitterGrammar {
    /// Parse tree-sitter grammar from grammar.js file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, TreeSitterError> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// Parse tree-sitter grammar from content
    pub fn parse(content: &str) -> Result<Self, TreeSitterError> {
        let mut rules = HashMap::new();
        let mut order = Vec::new();
        let mut precedences = HashMap::new();
        let mut conflicts = Vec::new();
        let mut keywords = HashSet::new();
        let mut operators = HashSet::new();

        // Extract PREC constants
        let prec_re = Regex::new(r"(\w+):\s*(\d+)").unwrap();
        if let Some(prec_block) = Self::extract_block(content, "const PREC = {", "}") {
            for caps in prec_re.captures_iter(&prec_block) {
                let name = caps.get(1).unwrap().as_str().to_string();
                let value: i32 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
                precedences.insert(name, value);
            }
        }

        // Extract conflicts
        if let Some(conflicts_block) = Self::extract_block(content, "conflicts: $ => [", "]") {
            let conflict_re = Regex::new(r"\[([^\]]+)\]").unwrap();
            for caps in conflict_re.captures_iter(&conflicts_block) {
                let items: Vec<String> = caps
                    .get(1)
                    .unwrap()
                    .as_str()
                    .split(',')
                    .map(|s| s.trim().trim_start_matches("$.").to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !items.is_empty() {
                    conflicts.push(items);
                }
            }
        }

        // Extract rules from rules: { ... }
        if let Some(rules_block) = Self::extract_rules_block(content) {
            let rule_re = Regex::new(r"(\w+):\s*\$\s*=>").unwrap();
            // Extract keywords (strings in quotes)
            let kw_re = Regex::new(r"'([a-zA-Z_][a-zA-Z0-9_]*)'").unwrap();
            // Extract operators (special chars in quotes)
            let op_re = Regex::new(r"'([+\-*/%&|^~!<>=.,:;?@#]+)'").unwrap();
            // Extract rule references ($._name or $.name)
            let ref_re = Regex::new(r"\$\.(_?\w+)").unwrap();
            for caps in rule_re.captures_iter(&rules_block) {
                let name = caps.get(1).unwrap().as_str().to_string();

                // Find the definition - look for the next rule or end
                let def_start = caps.get(0).unwrap().end();
                let def_end = Self::find_rule_end(&rules_block[def_start..]);
                let definition = rules_block[def_start..def_start + def_end]
                    .trim()
                    .to_string();

                let rule_keywords: Vec<String> = kw_re
                    .captures_iter(&definition)
                    .map(|c| c.get(1).unwrap().as_str().to_string())
                    .collect();

                let rule_operators: Vec<String> = op_re
                    .captures_iter(&definition)
                    .map(|c| c.get(1).unwrap().as_str().to_string())
                    .collect();

                let references: Vec<String> = ref_re
                    .captures_iter(&definition)
                    .map(|c| c.get(1).unwrap().as_str().to_string())
                    .collect();

                for kw in &rule_keywords {
                    keywords.insert(kw.clone());
                }
                for op in &rule_operators {
                    operators.insert(op.clone());
                }

                let is_hidden = name.starts_with('_');

                let rule = TreeSitterRule {
                    name: name.clone(),
                    is_hidden,
                    definition,
                    keywords: rule_keywords,
                    operators: rule_operators,
                    references,
                };

                if !rules.contains_key(&name) {
                    order.push(name.clone());
                }
                rules.insert(name, rule);
            }
        }

        Ok(TreeSitterGrammar {
            rules,
            order,
            precedences,
            conflicts,
            keywords,
            operators,
        })
    }

    /// Extract a block between start and end markers
    fn extract_block(content: &str, start: &str, _end: &str) -> Option<String> {
        let start_idx = content.find(start)?;
        let search_start = start_idx + start.len();
        let mut depth = 1;
        let mut end_idx = search_start;

        for (i, c) in content[search_start..].char_indices() {
            match c {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end_idx = search_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        Some(content[search_start..end_idx].to_string())
    }

    /// Extract the rules block from grammar.js
    fn extract_rules_block(content: &str) -> Option<String> {
        let start = content.find("rules: {")?;
        let search_start = start + "rules: {".len();
        let mut depth = 1;
        let mut end_idx = search_start;

        for (i, c) in content[search_start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_idx = search_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        Some(content[search_start..end_idx].to_string())
    }

    /// Find where a rule definition ends
    fn find_rule_end(content: &str) -> usize {
        let mut depth = 0;
        let mut in_string = false;
        let mut prev_char = ' ';

        for (i, c) in content.char_indices() {
            if c == '\'' && prev_char != '\\' {
                in_string = !in_string;
            }

            if !in_string {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => {
                        if depth == 0 {
                            return i;
                        }
                        depth -= 1;
                    }
                    ',' if depth == 0 => return i,
                    _ => {}
                }
            }

            prev_char = c;
        }

        content.len()
    }

    /// Get a rule by name
    pub fn get(&self, name: &str) -> Option<&TreeSitterRule> {
        self.rules.get(name)
    }

    /// Iterate over all rules in definition order
    pub fn iter(&self) -> impl Iterator<Item = &TreeSitterRule> {
        self.order.iter().filter_map(|name| self.rules.get(name))
    }

    /// Count of rules
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Check if grammar is empty
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Result of comparing EBNF and tree-sitter grammars
#[derive(Debug)]
pub struct GrammarComparison {
    /// Productions in EBNF but not in tree-sitter
    pub missing_in_treesitter: Vec<String>,
    /// Rules in tree-sitter but not in EBNF
    pub missing_in_ebnf: Vec<String>,
    /// Rules present in both (matched)
    pub matched: Vec<String>,
    /// Keywords in EBNF but not in tree-sitter
    pub missing_keywords_in_ts: Vec<String>,
    /// Keywords in tree-sitter but not in EBNF
    pub missing_keywords_in_ebnf: Vec<String>,
    /// Operators in EBNF but not in tree-sitter
    pub missing_operators_in_ts: Vec<String>,
    /// Operators in tree-sitter but not in EBNF
    pub missing_operators_in_ebnf: Vec<String>,
}

impl GrammarComparison {
    /// Check if grammars are in sync
    pub fn is_in_sync(&self) -> bool {
        self.missing_in_treesitter.is_empty()
            && self.missing_in_ebnf.is_empty()
            && self.missing_keywords_in_ts.is_empty()
            && self.missing_keywords_in_ebnf.is_empty()
    }

    /// Get sync percentage (matched / total unique rules)
    pub fn sync_percentage(&self) -> f64 {
        let total =
            self.matched.len() + self.missing_in_treesitter.len() + self.missing_in_ebnf.len();
        if total == 0 {
            100.0
        } else {
            (self.matched.len() as f64 / total as f64) * 100.0
        }
    }
}

/// Compare EBNF grammar with tree-sitter grammar
pub fn compare_grammars(ebnf: &EbnfGrammar, ts: &TreeSitterGrammar) -> GrammarComparison {
    let ebnf_names: HashSet<_> = ebnf.productions.keys().cloned().collect();
    let ts_names: HashSet<_> = ts
        .rules
        .keys()
        .filter(|n| !n.starts_with('_')) // Exclude hidden rules
        .cloned()
        .collect();

    // Normalize names for comparison (snake_case vs camelCase, etc.)
    let normalize = |s: &str| s.to_lowercase().replace('-', "_");

    let ebnf_normalized: HashMap<String, String> = ebnf_names
        .iter()
        .map(|n| (normalize(n), n.clone()))
        .collect();

    let ts_normalized: HashMap<String, String> =
        ts_names.iter().map(|n| (normalize(n), n.clone())).collect();

    let mut missing_in_treesitter = Vec::new();
    let mut missing_in_ebnf = Vec::new();
    let mut matched = Vec::new();

    // Find EBNF productions missing in tree-sitter
    for (norm, orig) in &ebnf_normalized {
        if ts_normalized.contains_key(norm) {
            matched.push(orig.clone());
        } else {
            missing_in_treesitter.push(orig.clone());
        }
    }

    // Find tree-sitter rules missing in EBNF
    for (norm, orig) in &ts_normalized {
        if !ebnf_normalized.contains_key(norm) {
            missing_in_ebnf.push(orig.clone());
        }
    }

    // Compare keywords
    let ebnf_keywords: HashSet<_> = ebnf.keywords.iter().cloned().collect();
    let ts_keywords: HashSet<_> = ts.keywords.iter().cloned().collect();

    let missing_keywords_in_ts: Vec<_> = ebnf_keywords.difference(&ts_keywords).cloned().collect();
    let missing_keywords_in_ebnf: Vec<_> =
        ts_keywords.difference(&ebnf_keywords).cloned().collect();

    // Compare operators
    let ebnf_operators: HashSet<_> = ebnf.operators.iter().cloned().collect();
    let ts_operators: HashSet<_> = ts.operators.iter().cloned().collect();

    let missing_operators_in_ts: Vec<_> =
        ebnf_operators.difference(&ts_operators).cloned().collect();
    let missing_operators_in_ebnf: Vec<_> =
        ts_operators.difference(&ebnf_operators).cloned().collect();

    // Sort for consistent output
    let mut result = GrammarComparison {
        missing_in_treesitter,
        missing_in_ebnf,
        matched,
        missing_keywords_in_ts,
        missing_keywords_in_ebnf,
        missing_operators_in_ts,
        missing_operators_in_ebnf,
    };

    result.missing_in_treesitter.sort();
    result.missing_in_ebnf.sort();
    result.matched.sort();
    result.missing_keywords_in_ts.sort();
    result.missing_keywords_in_ebnf.sort();
    result.missing_operators_in_ts.sort();
    result.missing_operators_in_ebnf.sort();

    result
}

/// Generate a comparison report
pub fn generate_report(comparison: &GrammarComparison) -> String {
    let mut report = String::new();

    report.push_str(&format!(
        "Grammar Comparison Report\n{}\n\n",
        "=".repeat(60)
    ));

    report.push_str(&format!(
        "Sync Status: {:.1}% ({} matched, {} total unique rules)\n\n",
        comparison.sync_percentage(),
        comparison.matched.len(),
        comparison.matched.len()
            + comparison.missing_in_treesitter.len()
            + comparison.missing_in_ebnf.len()
    ));

    if !comparison.missing_in_treesitter.is_empty() {
        report.push_str(&format!(
            "Productions in EBNF but missing in tree-sitter ({}):\n",
            comparison.missing_in_treesitter.len()
        ));
        for name in &comparison.missing_in_treesitter {
            report.push_str(&format!("  - {}\n", name));
        }
        report.push('\n');
    }

    if !comparison.missing_in_ebnf.is_empty() {
        report.push_str(&format!(
            "Rules in tree-sitter but missing in EBNF ({}):\n",
            comparison.missing_in_ebnf.len()
        ));
        for name in &comparison.missing_in_ebnf {
            report.push_str(&format!("  - {}\n", name));
        }
        report.push('\n');
    }

    if !comparison.missing_keywords_in_ts.is_empty() {
        report.push_str(&format!(
            "Keywords in EBNF but missing in tree-sitter ({}):\n",
            comparison.missing_keywords_in_ts.len()
        ));
        for kw in &comparison.missing_keywords_in_ts {
            report.push_str(&format!("  - {}\n", kw));
        }
        report.push('\n');
    }

    if !comparison.missing_keywords_in_ebnf.is_empty() {
        report.push_str(&format!(
            "Keywords in tree-sitter but missing in EBNF ({}):\n",
            comparison.missing_keywords_in_ebnf.len()
        ));
        for kw in &comparison.missing_keywords_in_ebnf {
            report.push_str(&format!("  - {}\n", kw));
        }
        report.push('\n');
    }

    if comparison.is_in_sync() {
        report.push_str("Grammars are in sync!\n");
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_grammar() {
        let content = r#"
const PREC = {
  ADD: 1,
  MUL: 2,
};

module.exports = grammar({
  name: 'test',
  rules: {
    source_file: $ => repeat($._item),
    _item: $ => choice($.decl, $.stmt),
    decl: $ => seq('let', $.identifier),
    identifier: $ => /[a-z]+/,
  },
});
"#;
        let grammar = TreeSitterGrammar::parse(content).unwrap();
        assert_eq!(grammar.rules.len(), 4);
        assert!(grammar.rules.contains_key("source_file"));
        assert!(grammar.rules.contains_key("_item"));
        assert!(grammar.rules.get("_item").unwrap().is_hidden);
        assert_eq!(grammar.precedences.get("ADD"), Some(&1));
        assert_eq!(grammar.precedences.get("MUL"), Some(&2));
    }

    #[test]
    fn test_extract_keywords() {
        let content = r#"
module.exports = grammar({
  name: 'test',
  rules: {
    stmt: $ => choice('let', 'var', 'const'),
  },
});
"#;
        let grammar = TreeSitterGrammar::parse(content).unwrap();
        assert!(grammar.keywords.contains("let"));
        assert!(grammar.keywords.contains("var"));
        assert!(grammar.keywords.contains("const"));
    }

    #[test]
    fn test_compare_grammars() {
        use std::collections::HashMap;

        let mut ebnf_prods = HashMap::new();
        ebnf_prods.insert(
            "expression".to_string(),
            crate::ebnf::Production {
                name: "expression".to_string(),
                definition: crate::ebnf::Symbol::Terminal("x".to_string()),
                line: 0,
                section: String::new(),
            },
        );
        ebnf_prods.insert(
            "statement".to_string(),
            crate::ebnf::Production {
                name: "statement".to_string(),
                definition: crate::ebnf::Symbol::Terminal("s".to_string()),
                line: 0,
                section: String::new(),
            },
        );

        let ebnf = EbnfGrammar {
            productions: ebnf_prods,
            order: vec!["expression".to_string(), "statement".to_string()],
            keywords: vec!["let".to_string()],
            operators: vec!["+".to_string()],
        };

        let mut ts_rules = HashMap::new();
        ts_rules.insert(
            "expression".to_string(),
            TreeSitterRule {
                name: "expression".to_string(),
                is_hidden: false,
                definition: String::new(),
                keywords: vec![],
                operators: vec![],
                references: vec![],
            },
        );
        ts_rules.insert(
            "block".to_string(),
            TreeSitterRule {
                name: "block".to_string(),
                is_hidden: false,
                definition: String::new(),
                keywords: vec![],
                operators: vec![],
                references: vec![],
            },
        );

        let ts = TreeSitterGrammar {
            rules: ts_rules,
            order: vec!["expression".to_string(), "block".to_string()],
            precedences: HashMap::new(),
            conflicts: vec![],
            keywords: HashSet::from(["let".to_string(), "var".to_string()]),
            operators: HashSet::from(["+".to_string()]),
        };

        let comparison = compare_grammars(&ebnf, &ts);
        assert!(comparison
            .missing_in_treesitter
            .contains(&"statement".to_string()));
        assert!(comparison.missing_in_ebnf.contains(&"block".to_string()));
        assert!(comparison.matched.contains(&"expression".to_string()));
    }
}
