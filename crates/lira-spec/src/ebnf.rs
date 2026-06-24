//! EBNF Grammar Extraction from Specification
//!
//! Parses EBNF grammar rules from the formal specification markdown.

use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during EBNF extraction
#[derive(Error, Debug)]
pub enum EbnfError {
    #[error("Failed to read specification file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse EBNF: {0}")]
    ParseError(String),

    #[error("Invalid production rule: {0}")]
    InvalidProduction(String),
}

/// A symbol in an EBNF grammar
#[derive(Debug, Clone, PartialEq)]
pub enum Symbol {
    /// Non-terminal symbol (references another production)
    NonTerminal(String),
    /// Terminal symbol (literal string)
    Terminal(String),
    /// Sequence of symbols
    Sequence(Vec<Symbol>),
    /// Choice between alternatives
    Choice(Vec<Symbol>),
    /// Optional (zero or one)
    Optional(Box<Symbol>),
    /// Repetition (zero or more)
    Repetition(Box<Symbol>),
    /// One or more repetition
    OneOrMore(Box<Symbol>),
    /// Grouping
    Group(Box<Symbol>),
    /// Exception (set difference)
    Exception(Box<Symbol>, Box<Symbol>),
}

/// A single EBNF production rule
#[derive(Debug, Clone)]
pub struct Production {
    /// Name of the production (left-hand side)
    pub name: String,
    /// Definition (right-hand side)
    pub definition: Symbol,
    /// Source location in spec (line number)
    pub line: usize,
    /// Section in the specification
    pub section: String,
}

/// Complete EBNF grammar extracted from specification
#[derive(Debug, Clone)]
pub struct EbnfGrammar {
    /// All production rules
    pub productions: HashMap<String, Production>,
    /// Order of productions (for stable iteration)
    pub order: Vec<String>,
    /// Keywords extracted from keyword productions
    pub keywords: Vec<String>,
    /// Operators extracted from operator productions
    pub operators: Vec<String>,
}

impl EbnfGrammar {
    /// Extract EBNF grammar from specification file
    pub fn from_spec<P: AsRef<Path>>(path: P) -> Result<Self, EbnfError> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// Parse EBNF grammar from specification content
    pub fn parse(content: &str) -> Result<Self, EbnfError> {
        let mut productions = HashMap::new();
        let mut order = Vec::new();
        let mut keywords = Vec::new();
        let mut operators = Vec::new();
        let mut current_section = String::new();

        // Match EBNF code blocks
        let code_block_re = Regex::new(r"```ebnf\n([\s\S]*?)```").unwrap();
        // Match section headers
        let section_re = Regex::new(r"^##+ (.+)$").unwrap();
        // Match production rules
        let production_re = Regex::new(r"^(\w+)\s*=\s*(.+?)\s*;?\s*$").unwrap();

        for line in content.lines() {
            // Track current section
            if let Some(caps) = section_re.captures(line) {
                current_section = caps.get(1).unwrap().as_str().to_string();
            }
        }

        // Extract from code blocks
        for caps in code_block_re.captures_iter(content) {
            let block = caps.get(1).unwrap().as_str();
            let block_line = content[..caps.get(0).unwrap().start()].lines().count();

            for (i, line) in block.lines().enumerate() {
                let trimmed = line.trim();

                // Skip comments and empty lines
                if trimmed.is_empty() || trimmed.starts_with("(*") {
                    continue;
                }

                // Parse production rule
                if let Some(caps) = production_re.captures(trimmed) {
                    let name = caps.get(1).unwrap().as_str().to_string();
                    let definition_str = caps.get(2).unwrap().as_str();

                    let definition = Self::parse_definition(definition_str)?;

                    // Extract keywords
                    if name.ends_with("_keyword") || name == "keyword" {
                        Self::extract_terminals(&definition, &mut keywords);
                    }

                    // Extract operators
                    if name.ends_with("_op") || name == "operator" {
                        Self::extract_terminals(&definition, &mut operators);
                    }

                    let production = Production {
                        name: name.clone(),
                        definition,
                        line: block_line + i,
                        section: current_section.clone(),
                    };

                    if !productions.contains_key(&name) {
                        order.push(name.clone());
                    }
                    productions.insert(name, production);
                }
            }
        }

        Ok(EbnfGrammar {
            productions,
            order,
            keywords,
            operators,
        })
    }

    /// Parse a production definition into a Symbol tree
    fn parse_definition(def: &str) -> Result<Symbol, EbnfError> {
        let def = def.trim();

        // Handle choice (|)
        if def.contains('|') && !Self::is_inside_quotes(def, '|') {
            let alternatives: Vec<&str> = Self::split_at_top_level(def, '|');
            if alternatives.len() > 1 {
                let symbols: Result<Vec<Symbol>, _> = alternatives
                    .into_iter()
                    .map(|alt| Self::parse_definition(alt.trim()))
                    .collect();
                return Ok(Symbol::Choice(symbols?));
            }
        }

        // Handle sequence (space-separated)
        let parts = Self::split_sequence(def);
        if parts.len() > 1 {
            let symbols: Result<Vec<Symbol>, _> = parts
                .into_iter()
                .map(|p| Self::parse_definition(p.trim()))
                .collect();
            return Ok(Symbol::Sequence(symbols?));
        }

        // Handle single element
        let def = def.trim();

        // Optional: [ ... ]
        if def.starts_with('[') && def.ends_with(']') {
            let inner = &def[1..def.len() - 1];
            return Ok(Symbol::Optional(Box::new(Self::parse_definition(inner)?)));
        }

        // Repetition: { ... }
        if def.starts_with('{') && def.ends_with('}') {
            let inner = &def[1..def.len() - 1];
            return Ok(Symbol::Repetition(Box::new(Self::parse_definition(inner)?)));
        }

        // Grouping: ( ... )
        if def.starts_with('(') && def.ends_with(')') {
            let inner = &def[1..def.len() - 1];
            return Ok(Symbol::Group(Box::new(Self::parse_definition(inner)?)));
        }

        // Terminal: "..." or '...'
        if (def.starts_with('"') && def.ends_with('"'))
            || (def.starts_with('\'') && def.ends_with('\''))
        {
            let inner = &def[1..def.len() - 1];
            return Ok(Symbol::Terminal(inner.to_string()));
        }

        // Non-terminal: identifier
        if def.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Ok(Symbol::NonTerminal(def.to_string()));
        }

        // Fallback: treat as terminal
        Ok(Symbol::Terminal(def.to_string()))
    }

    /// Check if a character is inside quotes
    fn is_inside_quotes(s: &str, target: char) -> bool {
        let mut in_single = false;
        let mut in_double = false;

        for c in s.chars() {
            match c {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                c if c == target && (in_single || in_double) => return true,
                _ => {}
            }
        }
        false
    }

    /// Split at top-level occurrences of a character
    fn split_at_top_level(s: &str, delim: char) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut start = 0;
        let mut depth = 0;
        let mut in_single = false;
        let mut in_double = false;

        for (i, c) in s.char_indices() {
            match c {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '(' | '[' | '{' if !in_single && !in_double => depth += 1,
                ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
                c if c == delim && depth == 0 && !in_single && !in_double => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(&s[start..]);
        parts
    }

    /// Split a sequence (space-separated, respecting grouping)
    fn split_sequence(s: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut start = 0;
        let mut depth = 0;
        let mut in_single = false;
        let mut in_double = false;
        let mut last_was_space = true;

        for (i, c) in s.char_indices() {
            match c {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '(' | '[' | '{' if !in_single && !in_double => depth += 1,
                ')' | ']' | '}' if !in_single && !in_double => depth -= 1,
                ' ' | '\t' if depth == 0 && !in_single && !in_double => {
                    if !last_was_space && start < i {
                        parts.push(&s[start..i]);
                    }
                    start = i + 1;
                    last_was_space = true;
                    continue;
                }
                ',' if depth == 0 && !in_single && !in_double => {
                    // Skip commas in sequences
                    if !last_was_space && start < i {
                        parts.push(&s[start..i]);
                    }
                    start = i + 1;
                    last_was_space = true;
                    continue;
                }
                _ => {}
            }
            last_was_space = false;
        }

        if start < s.len() {
            let remaining = s[start..].trim();
            if !remaining.is_empty() {
                parts.push(remaining);
            }
        }

        parts
    }

    /// Extract terminal symbols from a definition
    fn extract_terminals(symbol: &Symbol, terminals: &mut Vec<String>) {
        match symbol {
            Symbol::Terminal(t) => {
                if !terminals.contains(t) {
                    terminals.push(t.clone());
                }
            }
            Symbol::Sequence(syms) | Symbol::Choice(syms) => {
                for s in syms {
                    Self::extract_terminals(s, terminals);
                }
            }
            Symbol::Optional(s)
            | Symbol::Repetition(s)
            | Symbol::OneOrMore(s)
            | Symbol::Group(s) => {
                Self::extract_terminals(s, terminals);
            }
            Symbol::Exception(a, _) => {
                Self::extract_terminals(a, terminals);
            }
            Symbol::NonTerminal(_) => {}
        }
    }

    /// Get a production by name
    pub fn get(&self, name: &str) -> Option<&Production> {
        self.productions.get(name)
    }

    /// Iterate over all productions in definition order
    pub fn iter(&self) -> impl Iterator<Item = &Production> {
        self.order
            .iter()
            .filter_map(|name| self.productions.get(name))
    }

    /// Count of productions
    pub fn len(&self) -> usize {
        self.productions.len()
    }

    /// Check if grammar is empty
    pub fn is_empty(&self) -> bool {
        self.productions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_terminal() {
        let def = EbnfGrammar::parse_definition("\"let\"").unwrap();
        assert_eq!(def, Symbol::Terminal("let".to_string()));
    }

    #[test]
    fn test_parse_non_terminal() {
        let def = EbnfGrammar::parse_definition("expression").unwrap();
        assert_eq!(def, Symbol::NonTerminal("expression".to_string()));
    }

    #[test]
    fn test_parse_optional() {
        let def = EbnfGrammar::parse_definition("[ expression ]").unwrap();
        assert!(matches!(def, Symbol::Optional(_)));
    }

    #[test]
    fn test_parse_choice() {
        let def = EbnfGrammar::parse_definition("\"let\" | \"var\"").unwrap();
        assert!(matches!(def, Symbol::Choice(_)));
    }

    #[test]
    fn test_parse_sequence() {
        let def = EbnfGrammar::parse_definition("\"let\" identifier").unwrap();
        assert!(matches!(def, Symbol::Sequence(_)));
    }
}
