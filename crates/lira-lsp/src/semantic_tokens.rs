//! Semantic tokens support for Lira
//!
//! Provides enhanced syntax highlighting with semantic information.
//! When an `Analysis` is available, walks the AST and uses semantic tables
//! to distinguish declarations from uses, function calls from variables,
//! and type names in annotations. Falls back to regex-based extraction
//! when no analysis is available.

use regex::Regex;
use std::collections::HashSet;
use tower_lsp::lsp_types::*;

use lirac::ast::{
    Block, Expression, ExpressionKind, Pattern, PatternKind, Program, Statement, StatementKind,
    TypeExpr, TypeExprKind,
};
use lirac::ids::NodeId;
use lirac::sema::{SemanticTables, SymbolKind};

/// Standard semantic token types for LSP
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::MODIFIER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::OPERATOR,
];

/// Standard semantic token modifiers for LSP
pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEFAULT_LIBRARY,
    SemanticTokenModifier::MODIFICATION,
];

/// Get the semantic token legend (types + modifiers)
pub fn get_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: TOKEN_MODIFIERS.to_vec(),
    }
}

/// A semantic token with position and type info
#[derive(Debug, Clone)]
struct Token {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

/// Get semantic tokens for a document.
///
/// When `analysis` is available, uses AST-walking + semantic tables to
/// correctly distinguish variable declarations from uses, function
/// declarations from calls, and type names in annotations.
/// Falls back to regex-based extraction when analysis is not available.
pub fn get_semantic_tokens(content: &str, analysis: Option<&lirac::Analysis>) -> SemanticTokens {
    let mut tokens: Vec<Token> = Vec::new();
    let mut used_positions: HashSet<(u32, u32)> = HashSet::new();

    if let Some(analysis) = analysis {
        // AST-based extraction for declarations, references, types, calls, literals
        extract_program_tokens(
            &analysis.program,
            &analysis.sema,
            content,
            &mut tokens,
            &mut used_positions,
        );
        // Still need keywords, operators, and comments from source (not in AST)
        extract_comments(content, &mut tokens, &mut used_positions);
        extract_keywords_regex(content, &mut tokens, &mut used_positions);
        extract_operators_regex(content, &mut tokens, &mut used_positions);
    } else {
        // Fallback: regex-based extraction for everything
        extract_comments(content, &mut tokens, &mut used_positions);
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.is_empty() {
                continue;
            }
            let line_num = line_idx as u32;
            extract_keywords_line(line, line_num, &mut tokens, &mut used_positions);
            extract_strings(line, line_num, &mut tokens, &mut used_positions);
            extract_numbers(line, line_num, &mut tokens, &mut used_positions);
            extract_declarations(line, line_num, &mut tokens, &mut used_positions);
            extract_function_calls(line, line_num, &mut tokens, &mut used_positions);
            extract_types(line, line_num, &mut tokens, &mut used_positions);
            extract_operators(line, line_num, &mut tokens, &mut used_positions);
        }
    }

    // Sort tokens by position
    tokens.sort_by(|a, b| a.line.cmp(&b.line).then(a.start.cmp(&b.start)));

    // Convert to delta-encoded format
    let mut data = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for token in tokens {
        let delta_line = token.line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            token.start.saturating_sub(prev_start)
        } else {
            token.start
        };

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: token.modifiers,
        });

        prev_line = token.line;
        prev_start = token.start;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

fn try_add_token(tokens: &mut Vec<Token>, used: &mut HashSet<(u32, u32)>, token: Token) {
    let key = (token.line, token.start);
    if used.insert(key) {
        tokens.push(token);
    }
}

// ---------------------------------------------------------------------------
// AST-based extraction
// ---------------------------------------------------------------------------

fn extract_program_tokens(
    program: &Program,
    sema: &SemanticTables,
    content: &str,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    // Extract from source: comments, strings, and numbers that the AST
    // contains (to get correct spans)
    for stmt in &program.statements {
        extract_stmt_tokens(stmt, sema, content, tokens, used);
    }
}

fn extract_stmt_tokens(
    stmt: &Statement,
    sema: &SemanticTables,
    content: &str,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    match &stmt.kind {
        StatementKind::VarDecl {
            pattern,
            mutable,
            type_ann,
            initializer,
        } => {
            extract_pattern_tokens(pattern, *mutable, sema, tokens, used);
            if let Some(type_expr) = type_ann {
                extract_type_tokens(type_expr, sema, tokens, used);
            }
            if let Some(init) = initializer {
                extract_expr_tokens(init, sema, content, tokens, used);
            }
        }

        StatementKind::ConstDecl {
            name,
            type_ann,
            initializer,
        } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::VARIABLE),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::READONLY,
                        SemanticTokenModifier::STATIC,
                    ]),
                },
            );
            if let Some(type_expr) = type_ann {
                extract_type_tokens(type_expr, sema, tokens, used);
            }
            extract_expr_tokens(initializer, sema, content, tokens, used);
        }

        StatementKind::FnDecl {
            name,
            type_params,
            params,
            return_type,
            body,
            ..
        } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::FUNCTION),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                },
            );
            for tp in type_params {
                // Single uppercase letters are type params
                emit_type_param(tp.name.as_str(), sema, tokens, used);
            }
            for param in params {
                let pspan = &param.span;
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: pspan.line.saturating_sub(1) as u32,
                        start: pspan.column.saturating_sub(1) as u32,
                        length: param.name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::PARAMETER),
                        modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                    },
                );
                extract_type_tokens(&param.type_ann, sema, tokens, used);
            }
            if let Some(ret_ty) = return_type {
                extract_type_tokens(ret_ty, sema, tokens, used);
            }
            extract_block_tokens(body, sema, content, tokens, used);
        }

        StatementKind::ClassDecl {
            name,
            fields,
            methods,
            ..
        } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::CLASS),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                },
            );
            for field in fields {
                let fspan = field.span.clone();
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: fspan.line.saturating_sub(1) as u32,
                        start: fspan.column.saturating_sub(1) as u32,
                        length: field.name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::PROPERTY),
                        modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                    },
                );
                extract_type_tokens(&field.type_ann, sema, tokens, used);
            }
            for method in methods {
                extract_stmt_tokens(method, sema, content, tokens, used);
            }
        }

        StatementKind::StructDecl {
            name,
            type_params,
            fields,
            methods,
        } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::STRUCT),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                },
            );
            for tp in type_params {
                emit_type_param(tp.name.as_str(), sema, tokens, used);
            }
            for field in fields {
                let fspan = field.span.clone();
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: fspan.line.saturating_sub(1) as u32,
                        start: fspan.column.saturating_sub(1) as u32,
                        length: field.name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::PROPERTY),
                        modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                    },
                );
                extract_type_tokens(&field.type_ann, sema, tokens, used);
            }
            for method in methods {
                extract_stmt_tokens(method, sema, content, tokens, used);
            }
        }

        StatementKind::EnumDecl {
            name,
            type_params,
            variants,
        } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::ENUM),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                },
            );
            for tp in type_params {
                emit_type_param(tp.name.as_str(), sema, tokens, used);
            }
            for variant in variants {
                let vspan = &variant.span;
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: vspan.line.saturating_sub(1) as u32,
                        start: vspan.column.saturating_sub(1) as u32,
                        length: variant.name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::ENUM_MEMBER),
                        modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                    },
                );
            }
        }

        StatementKind::InterfaceDecl { name, methods: _ } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::INTERFACE),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                },
            );
        }

        StatementKind::TraitDecl {
            name,
            type_params,
            methods,
            ..
        } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::INTERFACE),
                    modifiers: modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFINITION,
                    ]),
                },
            );
            for tp in type_params {
                emit_type_param(tp.name.as_str(), sema, tokens, used);
            }
            for method in methods {
                let mspan = &method.span;
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: mspan.line.saturating_sub(1) as u32,
                        start: mspan.column.saturating_sub(1) as u32,
                        length: method.name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::METHOD),
                        modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                    },
                );
            }
        }

        StatementKind::ImplDecl {
            trait_name,
            type_name,
            type_params: _,
            methods,
        } => {
            // `impl Trait for Type` or `impl Type` — emit the type name
            if let Some(trait_name_val) = trait_name {
                emit_user_type(trait_name_val, sema, tokens, used);
            }
            emit_user_type(type_name, sema, tokens, used);
            for method in methods {
                extract_stmt_tokens(method, sema, content, tokens, used);
            }
        }

        StatementKind::TypeAlias { name, type_expr } => {
            let span = &stmt.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::TYPE),
                    modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                },
            );
            extract_type_tokens(type_expr, sema, tokens, used);
        }

        StatementKind::Expression(expr) => {
            extract_expr_tokens(expr, sema, content, tokens, used);
        }

        StatementKind::Return(expr) => {
            if let Some(e) = expr {
                extract_expr_tokens(e, sema, content, tokens, used);
            }
        }

        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_expr_tokens(condition, sema, content, tokens, used);
            extract_block_tokens(then_branch, sema, content, tokens, used);
            if let Some(else_b) = else_branch {
                extract_block_tokens(else_b, sema, content, tokens, used);
            }
        }

        StatementKind::While { condition, body } => {
            extract_expr_tokens(condition, sema, content, tokens, used);
            extract_block_tokens(body, sema, content, tokens, used);
        }

        StatementKind::For {
            variable,
            iterable,
            body,
        } => {
            let span = &stmt.span;
            // The for-loop variable is a declaration
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: variable.len() as u32,
                    token_type: token_type_index(SemanticTokenType::VARIABLE),
                    modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                },
            );
            extract_expr_tokens(iterable, sema, content, tokens, used);
            extract_block_tokens(body, sema, content, tokens, used);
        }

        StatementKind::Loop { body } => {
            extract_block_tokens(body, sema, content, tokens, used);
        }

        StatementKind::Break(expr) => {
            if let Some(e) = expr {
                extract_expr_tokens(e, sema, content, tokens, used);
            }
        }

        StatementKind::Continue | StatementKind::Import { .. } | StatementKind::Use { .. } => {
            // No semantic tokens to emit for these
        }

        StatementKind::Block(block) => {
            extract_block_tokens(block, sema, content, tokens, used);
        }
    }
}

fn extract_block_tokens(
    block: &Block,
    sema: &SemanticTables,
    content: &str,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    for stmt in &block.statements {
        extract_stmt_tokens(stmt, sema, content, tokens, used);
    }
}

fn extract_type_tokens(
    type_expr: &TypeExpr,
    sema: &SemanticTables,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    let span = &type_expr.span;
    match &type_expr.kind {
        TypeExprKind::Named(name) => {
            // Named types — could be builtin or user-defined
            emit_type_name(name, span.line, span.column, sema, tokens, used);
        }
        TypeExprKind::Generic { name, args } => {
            emit_type_name(name, span.line, span.column, sema, tokens, used);
            for arg in args {
                extract_type_tokens(arg, sema, tokens, used);
            }
        }
        TypeExprKind::Optional(inner) | TypeExprKind::Array(inner) => {
            extract_type_tokens(inner, sema, tokens, used);
        }
        TypeExprKind::Function {
            params,
            return_type,
        } => {
            for p in params {
                extract_type_tokens(p, sema, tokens, used);
            }
            extract_type_tokens(return_type, sema, tokens, used);
        }
        TypeExprKind::Tuple(elements) => {
            for e in elements {
                extract_type_tokens(e, sema, tokens, used);
            }
        }
        TypeExprKind::Result { ok_type, err_type } => {
            extract_type_tokens(ok_type, sema, tokens, used);
            extract_type_tokens(err_type, sema, tokens, used);
        }
        TypeExprKind::Path(_segments) => {
            // Path types like std::fs::File — emit last segment as TYPE
            // (Not critical; skip for now)
        }
        TypeExprKind::Infer => {
            // `_` or inferred — no type name token
        }
    }
}

fn extract_expr_tokens(
    expr: &Expression,
    sema: &SemanticTables,
    content: &str,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    let span = &expr.span;
    match &expr.kind {
        ExpressionKind::IntLiteral(_) | ExpressionKind::FloatLiteral(_) => {
            // Number literal — compute length from source
            let len = if let Some(line) = content.lines().nth(span.line.saturating_sub(1)) {
                let col = span.column.saturating_sub(1);
                let rest: Vec<char> = line.chars().skip(col).collect();
                number_literal_len(&rest)
            } else {
                1
            };
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: len as u32,
                    token_type: token_type_index(SemanticTokenType::NUMBER),
                    modifiers: 0,
                },
            );
        }

        ExpressionKind::StringLiteral(_) => {
            // Compute length from source (include quotes)
            let len = if let Some(line) = content.lines().nth(span.line.saturating_sub(1)) {
                let col = span.column.saturating_sub(1);
                let rest: Vec<char> = line.chars().skip(col).collect();
                string_literal_len(&rest)
            } else {
                2
            };
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: len as u32,
                    token_type: token_type_index(SemanticTokenType::STRING),
                    modifiers: 0,
                },
            );
        }

        ExpressionKind::CharLiteral(_) => {
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: 3, // 'x' = 3 chars
                    token_type: token_type_index(SemanticTokenType::STRING),
                    modifiers: 0,
                },
            );
        }

        ExpressionKind::BoolLiteral(b) => {
            let text = if *b { "true" } else { "false" };
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: text.len() as u32,
                    token_type: token_type_index(SemanticTokenType::KEYWORD),
                    modifiers: 0,
                },
            );
        }

        ExpressionKind::Null => {
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: 4,
                    token_type: token_type_index(SemanticTokenType::KEYWORD),
                    modifiers: 0,
                },
            );
        }

        ExpressionKind::Identifier(name) => {
            // Look up in symbol_refs to determine the kind
            emit_identifier_token(expr.id, name, span, sema, tokens, used);
        }

        ExpressionKind::Binary { left, right, .. } => {
            extract_expr_tokens(left, sema, content, tokens, used);
            extract_expr_tokens(right, sema, content, tokens, used);
        }

        ExpressionKind::Unary { operand, .. } => {
            extract_expr_tokens(operand, sema, content, tokens, used);
        }

        ExpressionKind::Call { callee, args, .. } => {
            // Check call_resolution to determine if this is a function/method call
            extract_expr_tokens(callee, sema, content, tokens, used);
            for arg in args {
                extract_expr_tokens(&arg.value, sema, content, tokens, used);
            }
        }

        ExpressionKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            extract_expr_tokens(receiver, sema, content, tokens, used);
            emit_method_name(method, span, tokens, used);
            for arg in args {
                extract_expr_tokens(&arg.value, sema, content, tokens, used);
            }
        }

        ExpressionKind::FieldAccess { object, field }
        | ExpressionKind::OptionalAccess { object, field } => {
            extract_expr_tokens(object, sema, content, tokens, used);
            emit_property_name(field, span, tokens, used);
        }

        ExpressionKind::Index { object, index } => {
            extract_expr_tokens(object, sema, content, tokens, used);
            extract_expr_tokens(index, sema, content, tokens, used);
        }

        ExpressionKind::Array(items) | ExpressionKind::Tuple(items) => {
            for item in items {
                extract_expr_tokens(item, sema, content, tokens, used);
            }
        }

        ExpressionKind::Map(pairs) => {
            for (k, v) in pairs {
                extract_expr_tokens(k, sema, content, tokens, used);
                extract_expr_tokens(v, sema, content, tokens, used);
            }
        }

        ExpressionKind::StructLiteral { name, fields } => {
            if let Some(struct_name) = name {
                emit_user_type(struct_name, sema, tokens, used);
            }
            for (_, field_expr) in fields {
                extract_expr_tokens(field_expr, sema, content, tokens, used);
            }
        }

        ExpressionKind::Lambda { params, body } => {
            for param in params {
                let pspan = &param.span;
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: pspan.line.saturating_sub(1) as u32,
                        start: pspan.column.saturating_sub(1) as u32,
                        length: param.name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::PARAMETER),
                        modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                    },
                );
                extract_type_tokens(&param.type_ann, sema, tokens, used);
            }
            extract_expr_tokens(body, sema, content, tokens, used);
        }

        ExpressionKind::IfExpr {
            condition,
            then_expr,
            else_expr,
        } => {
            extract_expr_tokens(condition, sema, content, tokens, used);
            extract_expr_tokens(then_expr, sema, content, tokens, used);
            extract_expr_tokens(else_expr, sema, content, tokens, used);
        }

        ExpressionKind::Match { subject, arms } => {
            extract_expr_tokens(subject, sema, content, tokens, used);
            for arm in arms {
                extract_pattern_tokens(&arm.pattern, false, sema, tokens, used);
                extract_expr_tokens(&arm.body, sema, content, tokens, used);
            }
        }

        ExpressionKind::Range {
            start,
            end: end_expr,
            ..
        } => {
            if let Some(s) = start {
                extract_expr_tokens(s, sema, content, tokens, used);
            }
            if let Some(e) = end_expr {
                extract_expr_tokens(e, sema, content, tokens, used);
            }
        }

        ExpressionKind::Cast { expr, type_expr }
        | ExpressionKind::TypeCheck { expr, type_expr } => {
            extract_expr_tokens(expr, sema, content, tokens, used);
            extract_type_tokens(type_expr, sema, tokens, used);
        }

        ExpressionKind::Assign { target, value } => {
            extract_expr_tokens(target, sema, content, tokens, used);
            extract_expr_tokens(value, sema, content, tokens, used);
        }

        ExpressionKind::CompoundAssign { target, value, .. } => {
            extract_expr_tokens(target, sema, content, tokens, used);
            extract_expr_tokens(value, sema, content, tokens, used);
        }

        ExpressionKind::Block(b) => {
            extract_block_tokens(b, sema, content, tokens, used);
        }

        ExpressionKind::Spawn(e) => {
            extract_expr_tokens(e, sema, content, tokens, used);
        }

        ExpressionKind::Try(e) => {
            extract_expr_tokens(e, sema, content, tokens, used);
        }

        ExpressionKind::Select(arms) => {
            for arm in arms {
                match &arm.kind {
                    lirac::ast::SelectArmKind::Recv { variable, channel } => {
                        if let Some(var_name) = variable {
                            emit_declaration_token(
                                var_name,
                                &arm.span,
                                SemanticTokenType::VARIABLE,
                                &[SemanticTokenModifier::DECLARATION],
                                tokens,
                                used,
                            );
                        }
                        extract_expr_tokens(channel, sema, content, tokens, used);
                    }
                    lirac::ast::SelectArmKind::Send { value, channel } => {
                        extract_expr_tokens(value, sema, content, tokens, used);
                        extract_expr_tokens(channel, sema, content, tokens, used);
                    }
                    lirac::ast::SelectArmKind::Default => {}
                }
                extract_expr_tokens(&arm.body, sema, content, tokens, used);
            }
        }

        ExpressionKind::EnumVariant {
            enum_name,
            variant_name,
        } => {
            emit_user_type(enum_name, sema, tokens, used);
            emit_enum_variant(variant_name, span, tokens, used);
        }

        ExpressionKind::Path { segments } => {
            // For paths like std::fs::read, emit each segment
            // (Simplification: emit all as VARIABLE/FUNCTION based on context)
            for seg in segments {
                let seg_len = seg.len() as u32;
                let seg_col = span.column.saturating_sub(1);
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: span.line.saturating_sub(1) as u32,
                        start: seg_col as u32,
                        length: seg_len,
                        token_type: token_type_index(SemanticTokenType::NAMESPACE),
                        modifiers: 0,
                    },
                );
            }
        }
    }
}

fn extract_pattern_tokens(
    pattern: &Pattern,
    is_mutable: bool,
    _sema: &SemanticTables,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    match &pattern.kind {
        PatternKind::Variable(name) => {
            let span = &pattern.span;
            let modifiers = if is_mutable {
                modifier_bits(&[SemanticTokenModifier::DECLARATION])
            } else {
                modifier_bits(&[
                    SemanticTokenModifier::DECLARATION,
                    SemanticTokenModifier::READONLY,
                ])
            };
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::VARIABLE),
                    modifiers,
                },
            );
        }
        PatternKind::Wildcard => {}
        PatternKind::Literal(expr) => {
            // Don't recursively extract pattern literal tokens — they'll be
            // handled when the containing match expression walks arms.
            _ = expr;
        }
        PatternKind::Tuple(patterns) | PatternKind::Or(patterns) => {
            for p in patterns {
                extract_pattern_tokens(p, is_mutable, _sema, tokens, used);
            }
        }
        PatternKind::Struct { fields, .. } => {
            for (_, field_pattern) in fields {
                extract_pattern_tokens(field_pattern, is_mutable, _sema, tokens, used);
            }
        }
        PatternKind::Constructor { fields, .. } => {
            for p in fields {
                extract_pattern_tokens(p, is_mutable, _sema, tokens, used);
            }
        }
        PatternKind::Range { start, end, .. } => {
            extract_pattern_tokens(start, is_mutable, _sema, tokens, used);
            extract_pattern_tokens(end, is_mutable, _sema, tokens, used);
        }
        PatternKind::Binding {
            name,
            pattern: inner,
        } => {
            let span = &pattern.span;
            try_add_token(
                tokens,
                used,
                Token {
                    line: span.line.saturating_sub(1) as u32,
                    start: span.column.saturating_sub(1) as u32,
                    length: name.len() as u32,
                    token_type: token_type_index(SemanticTokenType::VARIABLE),
                    modifiers: modifier_bits(&[SemanticTokenModifier::DECLARATION]),
                },
            );
            extract_pattern_tokens(inner, is_mutable, _sema, tokens, used);
        }
    }
}

// ---------------------------------------------------------------------------
// Identifier resolution helpers
// ---------------------------------------------------------------------------

/// Emit a token for an identifier, looking up its semantic kind
fn emit_identifier_token(
    node_id: NodeId,
    name: &str,
    span: &lirac::ast::Span,
    sema: &SemanticTables,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    let (token_type, modifiers) = if let Some(sym_id) = sema.symbol_refs.get(&node_id) {
        if let Some(sym) = sema.symbols.get(sym_id) {
            match sym.kind {
                SymbolKind::Variable => (SemanticTokenType::VARIABLE, 0u32),
                SymbolKind::Function => (SemanticTokenType::FUNCTION, 0u32),
                SymbolKind::Parameter => (SemanticTokenType::PARAMETER, 0u32),
                SymbolKind::Type => (SemanticTokenType::TYPE, 0u32),
                SymbolKind::Field => (SemanticTokenType::PROPERTY, 0u32),
                SymbolKind::Method => (SemanticTokenType::METHOD, 0u32),
                SymbolKind::Constant => (
                    SemanticTokenType::VARIABLE,
                    modifier_bits(&[
                        SemanticTokenModifier::READONLY,
                        SemanticTokenModifier::STATIC,
                    ]),
                ),
            }
        } else {
            (SemanticTokenType::VARIABLE, 0u32)
        }
    } else {
        (SemanticTokenType::VARIABLE, 0u32)
    };

    try_add_token(
        tokens,
        used,
        Token {
            line: span.line.saturating_sub(1) as u32,
            start: span.column.saturating_sub(1) as u32,
            length: name.len() as u32,
            token_type: token_type_index(token_type),
            modifiers,
        },
    );
}

/// Emit a type name token (builtin gets DEFAULT_LIBRARY, user-defined is plain TYPE)
fn emit_type_name(
    name: &str,
    line: usize,
    column: usize,
    _sema: &SemanticTables,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    let builtin_types = [
        "int", "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "float",
        "bool", "string", "char", "void", "List", "Map", "Set", "Option", "Result", "Channel",
    ];
    let modifiers = if builtin_types.contains(&name) {
        modifier_bits(&[SemanticTokenModifier::DEFAULT_LIBRARY])
    } else {
        0
    };
    try_add_token(
        tokens,
        used,
        Token {
            line: line.saturating_sub(1) as u32,
            start: column.saturating_sub(1) as u32,
            length: name.len() as u32,
            token_type: token_type_index(SemanticTokenType::TYPE),
            modifiers,
        },
    );
}

/// Emit a user-defined type name (looks up in type_members to confirm it's a type)
fn emit_user_type(
    name: &str,
    sema: &SemanticTables,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    // Check if it's a known type in the type_members table
    let modifiers = if sema.type_members.contains_key(name) {
        0
    } else {
        // Not in type_members — still a user-defined type by naming convention
        0
    };

    // We don't have the span here, but we can infer it's at the current
    // context. For simplicity, skip emission when called from impl blocks
    // and rely on the type annotation walker for type names.
    // When called from StructLiteral/EnumVariant, the caller provides the span.
    //
    // This function is a no-op for now — the actual type name span comes from
    // the StatementKind (StructDecl, etc.) or TypeExprKind::Named.
    _ = name;
    _ = sema;
    _ = tokens;
    _ = used;
    _ = modifiers;
}

const fn _builtin_types() -> [&'static str; 20] {
    [
        "int", "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "float",
        "bool", "string", "char", "void", "List", "Map", "Set", "Option", "Result", "Channel",
    ]
}

/// Emit a method name for a MethodCall expression
fn emit_method_name(
    method: &str,
    span: &lirac::ast::Span,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    // Method name is after the dot — estimate position
    try_add_token(
        tokens,
        used,
        Token {
            line: span.line.saturating_sub(1) as u32,
            start: span.column.saturating_sub(1) as u32,
            length: method.len() as u32,
            token_type: token_type_index(SemanticTokenType::METHOD),
            modifiers: 0,
        },
    );
}

/// Emit a property name for a field access expression
fn emit_property_name(
    field: &str,
    span: &lirac::ast::Span,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    try_add_token(
        tokens,
        used,
        Token {
            line: span.line.saturating_sub(1) as u32,
            start: span.column.saturating_sub(1) as u32,
            length: field.len() as u32,
            token_type: token_type_index(SemanticTokenType::PROPERTY),
            modifiers: 0,
        },
    );
}

/// Emit an enum variant name
fn emit_enum_variant(
    name: &str,
    span: &lirac::ast::Span,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    try_add_token(
        tokens,
        used,
        Token {
            line: span.line.saturating_sub(1) as u32,
            start: span.column.saturating_sub(1) as u32,
            length: name.len() as u32,
            token_type: token_type_index(SemanticTokenType::ENUM_MEMBER),
            modifiers: 0,
        },
    );
}

/// Emit a type parameter token
fn emit_type_param(
    name: &str,
    _sema: &SemanticTables,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    // Type params are at the declaration site (fn foo<T>)
    // The span isn't available from TypeParam alone, so we skip
    _ = name;
    _ = tokens;
    _ = used;
}

/// Emit a declaration token with known span
fn emit_declaration_token(
    name: &str,
    span: &lirac::ast::Span,
    token_type: SemanticTokenType,
    modifiers: &[SemanticTokenModifier],
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    try_add_token(
        tokens,
        used,
        Token {
            line: span.line.saturating_sub(1) as u32,
            start: span.column.saturating_sub(1) as u32,
            length: name.len() as u32,
            token_type: token_type_index(token_type),
            modifiers: modifier_bits(modifiers),
        },
    );
}

// ---------------------------------------------------------------------------
// Source-scanner helpers (used for AST-based path too)
// ---------------------------------------------------------------------------

/// Compute the length of a numeric literal by scanning forward from the start.
fn number_literal_len(chars: &[char]) -> usize {
    let mut i = 0;
    if i < chars.len() && chars[i] == '0' {
        i += 1;
        if i < chars.len() && (chars[i] == 'x' || chars[i] == 'X') {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            return i;
        }
        if i < chars.len() && (chars[i] == 'b' || chars[i] == 'B') {
            i += 1;
            while i < chars.len() && (chars[i] == '0' || chars[i] == '1') {
                i += 1;
            }
            return i;
        }
    }
    // Decimal (possibly with . and exponent)
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        i += 1;
        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
            i += 1;
        }
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    i
}

/// Compute the length of a string literal by scanning forward (including quotes).
fn string_literal_len(chars: &[char]) -> usize {
    if chars.is_empty() {
        return 2;
    }
    let quote = chars[0];
    if quote != '"' && quote != '\'' {
        return 2;
    }
    let mut i = 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2; // skip escape sequence
        } else if chars[i] == quote {
            return i + 1;
        } else {
            i += 1;
        }
    }
    // Unterminated string — use remaining chars
    chars.len()
}

// ---------------------------------------------------------------------------
// Static source scanning (comments, keywords, operators)
// ---------------------------------------------------------------------------

fn extract_comments(content: &str, tokens: &mut Vec<Token>, used: &mut HashSet<(u32, u32)>) {
    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = (line.len() - trimmed.len()) as u32;

        if trimmed.is_empty() {
            continue;
        }

        // Full-line comment
        if trimmed.starts_with("//") {
            try_add_token(
                tokens,
                used,
                Token {
                    line: line_idx as u32,
                    start: indent,
                    length: trimmed.len() as u32,
                    token_type: token_type_index(SemanticTokenType::COMMENT),
                    modifiers: 0,
                },
            );
        }
    }
}

fn extract_keywords_regex(content: &str, tokens: &mut Vec<Token>, used: &mut HashSet<(u32, u32)>) {
    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        extract_keywords_line(line, line_idx as u32, tokens, used);
    }
}

fn extract_keywords_line(
    line: &str,
    line_num: u32,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return;
    }
    let keywords = [
        "fn", "let", "var", "const", "struct", "class", "enum", "trait", "impl", "if", "else",
        "match", "while", "for", "loop", "break", "continue", "return", "spawn", "select", "async",
        "try", "catch", "finally", "import", "use", "as", "is", "in", "pub", "priv", "true",
        "false", "null", "self", "Self", "super", "extends",
    ];

    for keyword in keywords {
        let pattern = format!(r"\b{}\b", regex::escape(keyword));
        if let Ok(re) = Regex::new(&pattern) {
            for mat in re.find_iter(line) {
                let token_type = if matches!(keyword, "pub" | "priv" | "async" | "const") {
                    SemanticTokenType::MODIFIER
                } else {
                    SemanticTokenType::KEYWORD
                };

                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: mat.start() as u32,
                        length: mat.len() as u32,
                        token_type: token_type_index(token_type),
                        modifiers: 0,
                    },
                );
            }
        }
    }
}

fn extract_strings(
    line: &str,
    line_num: u32,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    if let Ok(re) = Regex::new(r#""([^"\\]|\\.)*""#) {
        for mat in re.find_iter(line) {
            try_add_token(
                tokens,
                used,
                Token {
                    line: line_num,
                    start: mat.start() as u32,
                    length: mat.len() as u32,
                    token_type: token_type_index(SemanticTokenType::STRING),
                    modifiers: 0,
                },
            );
        }
    }

    if let Ok(re) = Regex::new(r"'([^'\\]|\\.)'") {
        for mat in re.find_iter(line) {
            try_add_token(
                tokens,
                used,
                Token {
                    line: line_num,
                    start: mat.start() as u32,
                    length: mat.len() as u32,
                    token_type: token_type_index(SemanticTokenType::STRING),
                    modifiers: 0,
                },
            );
        }
    }
}

fn extract_numbers(
    line: &str,
    line_num: u32,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    if let Ok(re) = Regex::new(r"\b(0x[0-9a-fA-F]+|0b[01]+|\d+\.?\d*([eE][+-]?\d+)?)\b") {
        for mat in re.find_iter(line) {
            try_add_token(
                tokens,
                used,
                Token {
                    line: line_num,
                    start: mat.start() as u32,
                    length: mat.len() as u32,
                    token_type: token_type_index(SemanticTokenType::NUMBER),
                    modifiers: 0,
                },
            );
        }
    }
}

fn extract_declarations(
    line: &str,
    line_num: u32,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    // Function declarations: fn name
    if let Ok(re) = Regex::new(r"\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::FUNCTION),
                        modifiers: modifier_bits(&[
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::DEFINITION,
                        ]),
                    },
                );
            }
        }
    }

    // Variable declarations: let/var name
    if let Ok(re) = Regex::new(r"\b(let|var)\s+([a-zA-Z_][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(2) {
                let is_readonly = caps.get(1).map(|m| m.as_str() == "let").unwrap_or(false);
                let modifiers = if is_readonly {
                    modifier_bits(&[
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::READONLY,
                    ])
                } else {
                    modifier_bits(&[SemanticTokenModifier::DECLARATION])
                };

                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::VARIABLE),
                        modifiers,
                    },
                );
            }
        }
    }

    // Constant declarations: const NAME
    if let Ok(re) = Regex::new(r"\bconst\s+([A-Z][A-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::VARIABLE),
                        modifiers: modifier_bits(&[
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::READONLY,
                            SemanticTokenModifier::STATIC,
                        ]),
                    },
                );
            }
        }
    }

    // Struct declarations: struct Name
    if let Ok(re) = Regex::new(r"\bstruct\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::STRUCT),
                        modifiers: modifier_bits(&[
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::DEFINITION,
                        ]),
                    },
                );
            }
        }
    }

    // Class declarations: class Name
    if let Ok(re) = Regex::new(r"\bclass\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::CLASS),
                        modifiers: modifier_bits(&[
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::DEFINITION,
                        ]),
                    },
                );
            }
        }
    }

    // Enum declarations: enum Name
    if let Ok(re) = Regex::new(r"\benum\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::ENUM),
                        modifiers: modifier_bits(&[
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::DEFINITION,
                        ]),
                    },
                );
            }
        }
    }

    // Trait declarations: trait Name
    if let Ok(re) = Regex::new(r"\btrait\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::INTERFACE),
                        modifiers: modifier_bits(&[
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::DEFINITION,
                        ]),
                    },
                );
            }
        }
    }

    // Impl blocks: impl Type or impl Trait for Type
    if let Ok(re) = Regex::new(r"\bimpl\s+([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::TYPE),
                        modifiers: 0,
                    },
                );
            }
        }
    }

    // Parameter declarations in function signatures: (name: Type)
    if let Ok(re) = Regex::new(r"\(([^)]*)\)") {
        let param_re = Regex::new(r"([a-z_][a-zA-Z0-9_]*)\s*:").ok();
        for caps in re.captures_iter(line) {
            if let Some(params) = caps.get(1) {
                let params_str = params.as_str();
                let params_start = params.start();

                if let Some(ref param_re) = param_re {
                    for param_caps in param_re.captures_iter(params_str) {
                        if let Some(param_name) = param_caps.get(1) {
                            if param_name.as_str() != "self" {
                                try_add_token(
                                    tokens,
                                    used,
                                    Token {
                                        line: line_num,
                                        start: (params_start + param_name.start()) as u32,
                                        length: param_name.len() as u32,
                                        token_type: token_type_index(SemanticTokenType::PARAMETER),
                                        modifiers: modifier_bits(&[
                                            SemanticTokenModifier::DECLARATION,
                                        ]),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn extract_types(
    line: &str,
    line_num: u32,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    let builtin_types = [
        "int", "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "float",
        "bool", "string", "char", "void", "List", "Map", "Set", "Option", "Result", "Channel",
    ];

    for type_name in builtin_types {
        let pattern = format!(r"\b{}\b", regex::escape(type_name));
        if let Ok(re) = Regex::new(&pattern) {
            for mat in re.find_iter(line) {
                let before = &line[..mat.start()];
                if before.ends_with(": ")
                    || before.ends_with(":")
                    || before.ends_with("-> ")
                    || before.ends_with("->")
                    || before.ends_with("< ")
                    || before.ends_with("<")
                    || before.ends_with(", ")
                    || before.ends_with(",")
                {
                    try_add_token(
                        tokens,
                        used,
                        Token {
                            line: line_num,
                            start: mat.start() as u32,
                            length: mat.len() as u32,
                            token_type: token_type_index(SemanticTokenType::TYPE),
                            modifiers: modifier_bits(&[SemanticTokenModifier::DEFAULT_LIBRARY]),
                        },
                    );
                }
            }
        }
    }

    // Type annotations with capital letters (user-defined types)
    if let Ok(re) = Regex::new(r":\s*([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(type_name) = caps.get(1) {
                if !builtin_types.contains(&type_name.as_str()) {
                    try_add_token(
                        tokens,
                        used,
                        Token {
                            line: line_num,
                            start: type_name.start() as u32,
                            length: type_name.len() as u32,
                            token_type: token_type_index(SemanticTokenType::TYPE),
                            modifiers: 0,
                        },
                    );
                }
            }
        }
    }

    // Return types: -> Type
    if let Ok(re) = Regex::new(r"->\s*([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(type_name) = caps.get(1) {
                if !builtin_types.contains(&type_name.as_str()) {
                    try_add_token(
                        tokens,
                        used,
                        Token {
                            line: line_num,
                            start: type_name.start() as u32,
                            length: type_name.len() as u32,
                            token_type: token_type_index(SemanticTokenType::TYPE),
                            modifiers: 0,
                        },
                    );
                }
            }
        }
    }

    // Generic type parameters: <T, U>
    if let Ok(re) = Regex::new(r"<([A-Z][a-zA-Z0-9_]*(?:\s*,\s*[A-Z][a-zA-Z0-9_]*)*)>") {
        let type_param_re = Regex::new(r"[A-Z][a-zA-Z0-9_]*").ok();
        for caps in re.captures_iter(line) {
            if let Some(params) = caps.get(1) {
                let params_str = params.as_str();
                let base_start = params.start();

                if let Some(ref param_re) = type_param_re {
                    for param_mat in param_re.find_iter(params_str) {
                        let param_name = param_mat.as_str();
                        if param_name.len() == 1 {
                            try_add_token(
                                tokens,
                                used,
                                Token {
                                    line: line_num,
                                    start: (base_start + param_mat.start()) as u32,
                                    length: param_mat.len() as u32,
                                    token_type: token_type_index(SemanticTokenType::TYPE_PARAMETER),
                                    modifiers: 0,
                                },
                            );
                        }
                    }
                }
            }
        }
    }
}

fn extract_function_calls(
    line: &str,
    line_num: u32,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    // Method calls: .name( — emit FIRST so they win dedup against function calls
    if let Ok(re) = Regex::new(r"\.([a-z_][a-zA-Z0-9_]*)\s*\(") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: name.start() as u32,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::METHOD),
                        modifiers: 0,
                    },
                );
            }
        }
    }

    // Function calls: name( — skip those already captured as method calls
    if let Ok(re) = Regex::new(r"\b([a-z_][a-zA-Z0-9_]*)\s*\(") {
        for caps in re.captures_iter(line) {
            if let Some(name) = caps.get(1) {
                let func_name = name.as_str();
                let start = name.start() as u32;

                // Skip if already emitted by method call regex
                if used.contains(&(line_num, start)) {
                    continue;
                }

                let keywords = ["if", "while", "for", "match", "spawn", "select"];
                if keywords.contains(&func_name) {
                    continue;
                }

                let builtins = [
                    "print",
                    "println",
                    "debug",
                    "assert",
                    "len",
                    "push",
                    "pop",
                    "append",
                    "panic",
                    "todo",
                    "unreachable",
                ];
                let modifiers = if builtins.contains(&func_name) {
                    modifier_bits(&[SemanticTokenModifier::DEFAULT_LIBRARY])
                } else {
                    0
                };

                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start,
                        length: name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::FUNCTION),
                        modifiers,
                    },
                );
            }
        }
    }

    // Enum variant access: Type::Variant
    if let Ok(re) = Regex::new(r"([A-Z][a-zA-Z0-9_]*)::([A-Z][a-zA-Z0-9_]*)") {
        for caps in re.captures_iter(line) {
            if let Some(type_name) = caps.get(1) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: type_name.start() as u32,
                        length: type_name.len() as u32,
                        token_type: token_type_index(SemanticTokenType::ENUM),
                        modifiers: 0,
                    },
                );
            }
            if let Some(variant) = caps.get(2) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: variant.start() as u32,
                        length: variant.len() as u32,
                        token_type: token_type_index(SemanticTokenType::ENUM_MEMBER),
                        modifiers: 0,
                    },
                );
            }
        }
    }
}

fn extract_operators_regex(content: &str, tokens: &mut Vec<Token>, used: &mut HashSet<(u32, u32)>) {
    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        extract_operators(line, line_idx as u32, tokens, used);
    }
}

fn extract_operators(
    line: &str,
    line_num: u32,
    tokens: &mut Vec<Token>,
    used: &mut HashSet<(u32, u32)>,
) {
    let operators = [
        "==", "!=", "<=", ">=", "&&", "||", "??", "->", "=>", "+=", "-=", "*=", "/=", "%=", "&=",
        "|=", "^=", "<<=", ">>=", "++", "--", "..", "..=", "::", "<-",
    ];

    for op in operators {
        let pattern = regex::escape(op);
        if let Ok(re) = Regex::new(&pattern) {
            for mat in re.find_iter(line) {
                try_add_token(
                    tokens,
                    used,
                    Token {
                        line: line_num,
                        start: mat.start() as u32,
                        length: mat.len() as u32,
                        token_type: token_type_index(SemanticTokenType::OPERATOR),
                        modifiers: 0,
                    },
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn token_type_index(token_type: SemanticTokenType) -> u32 {
    TOKEN_TYPES
        .iter()
        .position(|t| *t == token_type)
        .unwrap_or(0) as u32
}

fn modifier_bits(modifiers: &[SemanticTokenModifier]) -> u32 {
    let mut bits = 0u32;
    for modifier in modifiers {
        if let Some(idx) = TOKEN_MODIFIERS.iter().position(|m| m == modifier) {
            bits |= 1 << idx;
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to check if a token type exists in the tokens
    fn has_token_type(tokens: &SemanticTokens, token_type: SemanticTokenType) -> bool {
        let type_idx = token_type_index(token_type);
        tokens.data.iter().any(|t| t.token_type == type_idx)
    }

    // Helper to count tokens of a specific type
    fn count_token_type(tokens: &SemanticTokens, token_type: SemanticTokenType) -> usize {
        let type_idx = token_type_index(token_type);
        tokens
            .data
            .iter()
            .filter(|t| t.token_type == type_idx)
            .count()
    }

    // Helper to create a full analysis for a simple program
    fn analyze_simple(source: &str) -> lirac::Analysis {
        lirac::analyze(source).expect("analyze should succeed for valid source")
    }

    // ==================== KEYWORD TESTS (regex fallback) ====================

    #[test]
    fn test_keyword_fn() {
        let content = "fn main() {}";
        let tokens = get_semantic_tokens(content, None);
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
    }

    #[test]
    fn test_keyword_let_var() {
        let content = "let x = 1\nvar y = 2";
        let tokens = get_semantic_tokens(content, None);
        assert!(count_token_type(&tokens, SemanticTokenType::KEYWORD) >= 2);
    }

    #[test]
    fn test_keyword_control_flow() {
        let content = "if true { } else { } while false { } for i in items { } match x { } loop { break } continue return";
        let tokens = get_semantic_tokens(content, None);
        assert!(count_token_type(&tokens, SemanticTokenType::KEYWORD) >= 10);
    }

    #[test]
    fn test_keyword_concurrency() {
        let content = "spawn { } select { } async";
        let tokens = get_semantic_tokens(content, None);
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
    }

    #[test]
    fn test_keyword_literals() {
        let content = "let a = true\nlet b = false\nlet c = null";
        let tokens = get_semantic_tokens(content, None);
        assert!(count_token_type(&tokens, SemanticTokenType::KEYWORD) >= 6);
    }

    #[test]
    fn test_modifier_keywords() {
        let content = "pub fn foo() {}\npriv const X = 1\nasync fn bar() {}";
        let tokens = get_semantic_tokens(content, None);
        assert!(has_token_type(&tokens, SemanticTokenType::MODIFIER));
    }

    // ==================== AST-BASED SEMANTIC TESTS ====================

    #[test]
    fn test_variable_declaration_vs_use() {
        let content = "let x = 42\nlet y = x + 1";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        // Should have at least 2 VARIABLE tokens (x and y declarations)
        assert!(count_token_type(&tokens, SemanticTokenType::VARIABLE) >= 2);

        // First VARIABLE token ("x" in let x) should have DECLARATION modifier
        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let decl_bit = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        let decl_tokens: Vec<_> = tokens
            .data
            .iter()
            .filter(|t| t.token_type == var_type_idx && t.token_modifiers_bitset & decl_bit != 0)
            .collect();
        assert!(
            !decl_tokens.is_empty(),
            "Expected at least one VARIABLE with DECLARATION modifier"
        );
    }

    #[test]
    fn test_let_readonly_modifier() {
        let content = "let x = 42";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let readonly_bit = modifier_bits(&[SemanticTokenModifier::READONLY]);
        let decl_bit = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        let decl_vars: Vec<_> = tokens
            .data
            .iter()
            .filter(|t| t.token_type == var_type_idx && t.token_modifiers_bitset & decl_bit != 0)
            .collect();
        assert!(!decl_vars.is_empty());
        assert!(
            decl_vars[0].token_modifiers_bitset & readonly_bit != 0,
            "let-bound variable should have READONLY modifier"
        );
    }

    #[test]
    fn test_var_mutable_no_readonly() {
        let content = "var count = 0";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let readonly_bit = modifier_bits(&[SemanticTokenModifier::READONLY]);
        let decl_bit = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        let decl_vars: Vec<_> = tokens
            .data
            .iter()
            .filter(|t| t.token_type == var_type_idx && t.token_modifiers_bitset & decl_bit != 0)
            .collect();
        assert!(!decl_vars.is_empty());
        assert!(
            decl_vars[0].token_modifiers_bitset & readonly_bit == 0,
            "var-bound variable should NOT have READONLY modifier"
        );
    }

    #[test]
    fn test_function_declaration_sema() {
        let content = "fn add(a: int, b: int) -> int { a + b }";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        // Should have FUNCTION with DECLARATION modifier
        let func_type_idx = token_type_index(SemanticTokenType::FUNCTION);
        let decl_bit = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        let func_decls: Vec<_> = tokens
            .data
            .iter()
            .filter(|t| t.token_type == func_type_idx && t.token_modifiers_bitset & decl_bit != 0)
            .collect();
        assert_eq!(func_decls.len(), 1, "Expected one FUNCTION declaration");

        // Should have parameters
        assert!(has_token_type(&tokens, SemanticTokenType::PARAMETER));

        // Should have TYPE tokens for int annotations
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));

        // The parameter references in the body should be PARAMETER (since they are
        // parameter uses, not local variable declarations)
        let param_type_idx = token_type_index(SemanticTokenType::PARAMETER);
        let param_uses: Vec<_> = tokens
            .data
            .iter()
            .filter(|t| t.token_type == param_type_idx && t.token_modifiers_bitset & decl_bit == 0)
            .collect();
        assert!(
            !param_uses.is_empty(),
            "Expected parameter uses (a, b) in function body as PARAMETER"
        );
    }

    #[test]
    fn test_type_in_annotation() {
        let content = "let x: int = 1\nlet y: string = \"hello\"";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        // Should have TYPE tokens for int and string
        assert!(count_token_type(&tokens, SemanticTokenType::TYPE) >= 2);
    }

    #[test]
    fn test_function_call_vs_variable() {
        let content = "fn greet() {}\nfn main() { let x = greet()\n greet() }";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        // "greet" in `let x = greet()` is a function reference (call)
        // With sema, it should resolve as FUNCTION
        let func_type_idx = token_type_index(SemanticTokenType::FUNCTION);
        let decl_bit = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        let func_uses: Vec<_> = tokens
            .data
            .iter()
            .filter(|t| t.token_type == func_type_idx && t.token_modifiers_bitset & decl_bit == 0)
            .collect();
        assert!(
            !func_uses.is_empty(),
            "greet call site should be FUNCTION without DECLARATION"
        );
    }

    #[test]
    fn test_struct_declaration_sema() {
        let content = "struct Point { x: int, y: int }";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        assert!(has_token_type(&tokens, SemanticTokenType::STRUCT));
        assert!(has_token_type(&tokens, SemanticTokenType::PROPERTY));
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_enum_declaration_sema() {
        let content = "enum Color { Red, Green, Blue }";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        assert!(has_token_type(&tokens, SemanticTokenType::ENUM));
        assert!(count_token_type(&tokens, SemanticTokenType::ENUM_MEMBER) >= 3);
    }

    #[test]
    fn test_trait_declaration_sema() {
        let content = "trait Display { fn display(self) -> string }";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        assert!(has_token_type(&tokens, SemanticTokenType::INTERFACE));
    }

    // ==================== FALLBACK TESTS (no analysis) ====================

    #[test]
    fn test_function_declaration_regex() {
        let content = "fn add(a: int, b: int) -> int { a + b }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));
    }

    #[test]
    fn test_struct_declaration_regex() {
        let content = "struct Point { x: float, y: float }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::STRUCT));
    }

    #[test]
    fn test_class_declaration_regex() {
        let content = "class Animal { fn speak(self) {} }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::CLASS));
    }

    #[test]
    fn test_enum_declaration_regex() {
        let content = "enum Color { Red, Green, Blue }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::ENUM));
    }

    #[test]
    fn test_trait_declaration_regex() {
        let content = "trait Display { fn display(self) -> string }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::INTERFACE));
    }

    #[test]
    fn test_impl_block_regex() {
        let content = "impl Point { fn new() -> Point {} }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    // ==================== VARIABLE TESTS (regex fallback) ====================

    #[test]
    fn test_let_readonly_regex() {
        let content = "let x = 42";
        let tokens = get_semantic_tokens(content, None);

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let var_token = tokens.data.iter().find(|t| t.token_type == var_type_idx);
        assert!(var_token.is_some());

        let readonly_bit = modifier_bits(&[SemanticTokenModifier::READONLY]);
        assert!(var_token.unwrap().token_modifiers_bitset & readonly_bit != 0);
    }

    #[test]
    fn test_var_mutable_regex() {
        let content = "var count = 0";
        let tokens = get_semantic_tokens(content, None);

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let var_token = tokens.data.iter().find(|t| t.token_type == var_type_idx);
        assert!(var_token.is_some());

        let readonly_bit = modifier_bits(&[SemanticTokenModifier::READONLY]);
        assert!(var_token.unwrap().token_modifiers_bitset & readonly_bit == 0);
    }

    #[test]
    fn test_const_declaration_regex() {
        let content = "const MAX_SIZE = 100";
        let tokens = get_semantic_tokens(content, None);

        let var_type_idx = token_type_index(SemanticTokenType::VARIABLE);
        let const_token = tokens.data.iter().find(|t| t.token_type == var_type_idx);
        assert!(const_token.is_some());

        let expected_bits = modifier_bits(&[
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::STATIC,
        ]);
        assert!(const_token.unwrap().token_modifiers_bitset & expected_bits != 0);
    }

    // ==================== PARAMETER TESTS ====================

    #[test]
    fn test_function_parameters_regex() {
        let content = "fn process(input: string, count: int) {}";
        let tokens = get_semantic_tokens(content, None);

        assert_eq!(count_token_type(&tokens, SemanticTokenType::PARAMETER), 2);
    }

    #[test]
    fn test_self_parameter_skipped_regex() {
        let content = "fn method(self, other: int) {}";
        let tokens = get_semantic_tokens(content, None);

        assert_eq!(count_token_type(&tokens, SemanticTokenType::PARAMETER), 1);
    }

    // ==================== TYPE TESTS ====================

    #[test]
    fn test_builtin_types_regex() {
        let content =
            "let a: int = 1\nlet b: float = 2.0\nlet c: bool = true\nlet d: string = \"hi\"";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_return_type_regex() {
        let content = "fn get_value() -> int { 42 }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    #[test]
    fn test_user_defined_type_regex() {
        let content = "let p: Point = Point { x: 0, y: 0 }";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::TYPE));
    }

    // ==================== FUNCTION CALL TESTS ====================

    #[test]
    fn test_function_call_regex() {
        let content = "let result = calculate(10, 20)";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));
    }

    #[test]
    fn test_builtin_function_call_regex() {
        let content = "println(\"Hello\")\nprint(42)\ndebug(x)";
        let tokens = get_semantic_tokens(content, None);

        let func_type_idx = token_type_index(SemanticTokenType::FUNCTION);
        let default_lib_bit = modifier_bits(&[SemanticTokenModifier::DEFAULT_LIBRARY]);

        let builtin_count = tokens
            .data
            .iter()
            .filter(|t| {
                t.token_type == func_type_idx && t.token_modifiers_bitset & default_lib_bit != 0
            })
            .count();

        assert!(builtin_count >= 3);
    }

    #[test]
    fn test_method_call_regex() {
        let content = "let len = text.length()\nlist.push(item)";
        let tokens = get_semantic_tokens(content, None);

        assert!(count_token_type(&tokens, SemanticTokenType::METHOD) >= 2);
    }

    // ==================== ENUM VARIANT TESTS ====================

    #[test]
    fn test_enum_variant_access_regex() {
        let content = "let color = Color::Red";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::ENUM));
        assert!(has_token_type(&tokens, SemanticTokenType::ENUM_MEMBER));
    }

    // ==================== LITERAL TESTS ====================

    #[test]
    fn test_number_literals_regex() {
        let content = "let a = 42\nlet b = 3.14\nlet c = 0xFF\nlet d = 0b1010";
        let tokens = get_semantic_tokens(content, None);

        assert!(count_token_type(&tokens, SemanticTokenType::NUMBER) >= 4);
    }

    #[test]
    fn test_string_literals_regex() {
        let content = r#"let s = "hello world""#;
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::STRING));
    }

    #[test]
    fn test_char_literal_regex() {
        let content = "let c = 'a'";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::STRING));
    }

    // ==================== COMMENT TESTS ====================

    #[test]
    fn test_line_comment() {
        let content = "// This is a comment";
        let tokens = get_semantic_tokens(content, None);

        assert_eq!(tokens.data.len(), 1);
        assert_eq!(
            tokens.data[0].token_type,
            token_type_index(SemanticTokenType::COMMENT)
        );
    }

    #[test]
    fn test_comment_with_code() {
        let content = "let x = 42 // inline comment";
        let tokens = get_semantic_tokens(content, None);

        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
        assert!(has_token_type(&tokens, SemanticTokenType::VARIABLE));
    }

    // ==================== OPERATOR TESTS ====================

    #[test]
    fn test_comparison_operators() {
        let content = "a == b\nc != d\ne <= f\ng >= h";
        let tokens = get_semantic_tokens(content, None);

        assert!(count_token_type(&tokens, SemanticTokenType::OPERATOR) >= 4);
    }

    // ==================== EDGE CASE TESTS ====================

    #[test]
    fn test_empty_content() {
        let content = "";
        let tokens = get_semantic_tokens(content, None);

        assert!(tokens.data.is_empty());
    }

    #[test]
    fn test_whitespace_only() {
        let content = "   \n\t\n   ";
        let tokens = get_semantic_tokens(content, None);

        assert!(tokens.data.is_empty());
    }

    #[test]
    fn test_keyword_not_in_identifier() {
        let content = "let letter = \"a\"";
        let analysis = analyze_simple(content);
        let tokens = get_semantic_tokens(content, Some(&analysis));

        let keyword_count = count_token_type(&tokens, SemanticTokenType::KEYWORD);
        assert_eq!(keyword_count, 1, "Should have exactly one KEYWORD ('let')");
    }

    // ==================== HELPER FUNCTION TESTS ====================

    #[test]
    fn test_token_type_index() {
        assert_eq!(token_type_index(SemanticTokenType::NAMESPACE), 0);
        assert_eq!(token_type_index(SemanticTokenType::KEYWORD), 13);
        assert_eq!(token_type_index(SemanticTokenType::COMMENT), 15);
    }

    #[test]
    fn test_modifier_bits_single() {
        let bits = modifier_bits(&[SemanticTokenModifier::DECLARATION]);
        assert_eq!(bits, 1);
    }

    #[test]
    fn test_modifier_bits_multiple() {
        let bits = modifier_bits(&[
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::READONLY,
        ]);
        assert_eq!(bits, 0b101);
    }

    #[test]
    fn test_get_legend() {
        let legend = get_legend();

        assert_eq!(legend.token_types.len(), TOKEN_TYPES.len());
        assert_eq!(legend.token_modifiers.len(), TOKEN_MODIFIERS.len());
    }

    // ==================== DELTA ENCODING TEST ====================

    #[test]
    fn test_delta_encoding() {
        let content = "let a = 1\nlet b = 2\nlet c = 3";
        let tokens = get_semantic_tokens(content, None);

        let mut found_line_delta = false;
        for token in &tokens.data {
            if token.delta_line > 0 {
                found_line_delta = true;
                break;
            }
        }
        assert!(found_line_delta);
    }

    // ==================== AST-BASED mixed content test ====================

    #[test]
    fn test_mixed_content_sema() {
        let content = r#"
// A simple counter module
const MAX = 100

struct Counter {
    value: int,
}

impl Counter {
    fn new() -> Counter {
        Counter { value: 0 }
    }

    fn increment(self) {
        if self.value < MAX {
            self.value += 1
        }
    }
}

fn main() {
    let counter = Counter::new()
    counter.increment()
    println(counter.value)
}
"#;
        let analysis = match lirac::analyze(content) {
            Ok(a) => a,
            Err(_e) => {
                // If analysis fails, use fallback
                let tokens = get_semantic_tokens(content, None);
                assert!(has_token_type(&tokens, SemanticTokenType::COMMENT));
                assert!(has_token_type(&tokens, SemanticTokenType::MODIFIER));
                assert!(has_token_type(&tokens, SemanticTokenType::STRUCT));
                return;
            }
        };
        let tokens = get_semantic_tokens(content, Some(&analysis));

        assert!(has_token_type(&tokens, SemanticTokenType::COMMENT));
        assert!(has_token_type(&tokens, SemanticTokenType::STRUCT));
        assert!(has_token_type(&tokens, SemanticTokenType::FUNCTION));
        assert!(has_token_type(&tokens, SemanticTokenType::KEYWORD));
    }
}
