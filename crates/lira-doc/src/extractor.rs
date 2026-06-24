//! Documentation extractor
//!
//! Extracts documentation comments from Lira source files and correlates
//! them with AST declarations to build structured documentation.

use crate::types::*;
use lirac::ast::{
    EnumVariant, Expression, ExpressionKind, Field, Parameter, Program, Statement, StatementKind,
    TraitMethod, TypeExpr, TypeExprKind, TypeParam, UnaryOp,
};
use std::collections::HashMap;

/// Extract documentation from a Lira source file
pub fn extract_docs(source_path: &str, source: &str) -> Result<ModuleDoc, String> {
    // Extract comment blocks and their ending line numbers
    let comments = extract_comments(source);

    // Parse AST to get declarations with locations
    let tokens = lirac::lexer::tokenize(source)?;
    let ast = lirac::parser::parse(&tokens)?;

    // Build module documentation
    let module_doc = build_module_doc(&ast, &comments, source_path, source);

    Ok(module_doc)
}

/// A comment block with its ending line number
#[derive(Debug)]
struct CommentBlock {
    lines: Vec<String>,
}

/// Extract all comment blocks from source, indexed by their ending line
fn extract_comments(source: &str) -> HashMap<usize, CommentBlock> {
    let mut comments: HashMap<usize, CommentBlock> = HashMap::new();
    let mut current_block: Vec<String> = Vec::new();
    let mut block_end_line = 0;
    let mut in_block_comment = false;
    let mut block_comment_content = String::new();

    for (line_idx, line) in source.lines().enumerate() {
        let line_num = line_idx + 1; // 1-based line numbers
        let trimmed = line.trim();

        // Handle block comments
        if in_block_comment {
            if let Some(end_pos) = trimmed.find("*/") {
                // End of block comment
                let before_end = &trimmed[..end_pos];
                if !before_end.is_empty() {
                    block_comment_content.push_str(before_end);
                }
                in_block_comment = false;

                // Store block comment
                let clean_content = block_comment_content
                    .lines()
                    .map(|l| l.trim().trim_start_matches('*').trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>();

                if !clean_content.is_empty() {
                    comments.insert(
                        line_num,
                        CommentBlock {
                            lines: clean_content,
                        },
                    );
                }
                block_comment_content.clear();
            } else {
                block_comment_content.push_str(trimmed);
                block_comment_content.push('\n');
            }
            continue;
        }

        // Check for block comment start
        if let Some(start_pos) = trimmed.find("/*") {
            let after_start = &trimmed[start_pos + 2..];
            if let Some(end_pos) = after_start.find("*/") {
                // Single-line block comment
                let content = after_start[..end_pos].trim();
                if !content.is_empty() {
                    comments.insert(
                        line_num,
                        CommentBlock {
                            lines: vec![content.to_string()],
                        },
                    );
                }
            } else {
                // Multi-line block comment starts
                in_block_comment = true;
                block_comment_content = after_start.to_string();
                block_comment_content.push('\n');
            }
            continue;
        }

        // Handle line comments
        if let Some(comment_text) = trimmed.strip_prefix("//").map(|s| s.trim()) {
            // Skip section headers (lines with === or ---)
            if comment_text.contains("====") || comment_text.contains("----") {
                // Flush current block if any
                if !current_block.is_empty() {
                    comments.insert(
                        block_end_line,
                        CommentBlock {
                            lines: current_block.clone(),
                        },
                    );
                    current_block.clear();
                }
                continue;
            }

            current_block.push(comment_text.to_string());
            block_end_line = line_num;
        } else if trimmed.is_empty() {
            // Empty line - flush current block
            if !current_block.is_empty() {
                comments.insert(
                    block_end_line,
                    CommentBlock {
                        lines: current_block.clone(),
                    },
                );
                current_block.clear();
            }
        } else {
            // Non-comment, non-empty line - flush current block
            if !current_block.is_empty() {
                comments.insert(
                    block_end_line,
                    CommentBlock {
                        lines: current_block.clone(),
                    },
                );
                current_block.clear();
            }
        }
    }

    // Flush any remaining block
    if !current_block.is_empty() {
        comments.insert(
            block_end_line,
            CommentBlock {
                lines: current_block,
            },
        );
    }

    comments
}

/// Find the doc comment for a declaration at the given line
fn find_doc_comment(comments: &HashMap<usize, CommentBlock>, decl_line: usize) -> Option<String> {
    // Look for a comment block that ends on the line immediately before the declaration
    // or within a few lines before (to handle blank lines)
    for offset in 1..=3 {
        if decl_line >= offset {
            if let Some(block) = comments.get(&(decl_line - offset)) {
                return Some(block.lines.join("\n"));
            }
        }
    }
    None
}

/// Build module documentation from AST and comments
fn build_module_doc(
    ast: &Program,
    comments: &HashMap<usize, CommentBlock>,
    source_path: &str,
    source: &str,
) -> ModuleDoc {
    let name = std::path::Path::new(source_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Get module-level description from top-of-file comments
    let description = find_module_description(source);

    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut traits = Vec::new();
    let mut impls = Vec::new();
    let mut classes = Vec::new();
    let mut constants = Vec::new();
    let mut type_aliases = Vec::new();

    for stmt in &ast.statements {
        match &stmt.kind {
            StatementKind::FnDecl {
                name,
                type_params,
                params,
                return_type,
                is_public,
                ..
            } => {
                let doc = find_doc_comment(comments, stmt.span.line);
                functions.push(build_function_doc(
                    name,
                    type_params,
                    params,
                    return_type.as_ref(),
                    *is_public,
                    stmt.span.line,
                    doc,
                ));
            }
            StatementKind::StructDecl {
                name,
                type_params,
                fields,
                methods,
            } => {
                let doc = find_doc_comment(comments, stmt.span.line);
                structs.push(build_struct_doc(
                    name,
                    type_params,
                    fields,
                    methods,
                    stmt.span.line,
                    doc,
                    comments,
                ));
            }
            StatementKind::EnumDecl { name, variants, .. } => {
                let doc = find_doc_comment(comments, stmt.span.line);
                enums.push(build_enum_doc(name, variants, stmt.span.line, doc));
            }
            StatementKind::TraitDecl {
                name,
                type_params,
                supertraits: _,
                methods,
                is_public,
            } => {
                let doc = find_doc_comment(comments, stmt.span.line);
                traits.push(build_trait_doc(
                    name,
                    type_params,
                    methods,
                    *is_public,
                    stmt.span.line,
                    doc,
                ));
            }
            StatementKind::ImplDecl {
                trait_name,
                type_name,
                type_params,
                methods,
            } => {
                impls.push(build_impl_doc(
                    type_name,
                    trait_name.as_ref(),
                    type_params,
                    methods,
                    stmt.span.line,
                    comments,
                ));
            }
            StatementKind::ClassDecl {
                name,
                parent,
                interfaces,
                fields,
                methods,
            } => {
                let doc = find_doc_comment(comments, stmt.span.line);
                classes.push(build_class_doc(
                    name,
                    parent.as_ref(),
                    interfaces,
                    fields,
                    methods,
                    stmt.span.line,
                    doc,
                    comments,
                ));
            }
            StatementKind::ConstDecl { name, type_ann, .. } => {
                let doc = find_doc_comment(comments, stmt.span.line);
                constants.push(ConstDoc {
                    name: name.clone(),
                    doc_comment: doc,
                    type_name: type_ann.as_ref().map(format_type),
                    line: stmt.span.line,
                });
            }
            StatementKind::TypeAlias { name, type_expr } => {
                let doc = find_doc_comment(comments, stmt.span.line);
                type_aliases.push(TypeAliasDoc {
                    name: name.clone(),
                    doc_comment: doc,
                    target_type: format_type(type_expr),
                    line: stmt.span.line,
                });
            }
            _ => {}
        }
    }

    ModuleDoc {
        name,
        path: source_path.to_string(),
        description,
        functions,
        structs,
        enums,
        traits,
        impls,
        classes,
        constants,
        type_aliases,
    }
}

/// Find module-level description from top-of-file comments
fn find_module_description(source: &str) -> Option<String> {
    let mut description_lines = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if !description_lines.is_empty() {
                // Empty line after comments - stop collecting
                break;
            }
            continue;
        }

        if let Some(comment) = trimmed.strip_prefix("//").map(|s| s.trim()) {
            // Skip header lines
            if comment.contains("====") || comment.contains("----") {
                continue;
            }
            description_lines.push(comment.to_string());
        } else if trimmed.starts_with("/*") {
            // Skip block comments at top for now
            continue;
        } else {
            // Found code - stop
            break;
        }
    }

    if description_lines.is_empty() {
        None
    } else {
        Some(description_lines.join("\n"))
    }
}

/// Build function documentation
fn build_function_doc(
    name: &str,
    type_params: &[TypeParam],
    params: &[Parameter],
    return_type: Option<&TypeExpr>,
    is_public: bool,
    line: usize,
    doc: Option<String>,
) -> FunctionDoc {
    let signature = format_function_signature(name, type_params, params, return_type);
    let param_docs: Vec<ParamDoc> = params
        .iter()
        .map(|p| ParamDoc {
            name: p.name.clone(),
            type_name: format_type(&p.type_ann),
            default_value: p.default.as_ref().map(format_default_expr),
        })
        .collect();

    FunctionDoc {
        name: name.to_string(),
        signature,
        doc_comment: doc,
        params: param_docs,
        return_type: return_type.map(format_type),
        type_params: type_params.iter().map(format_type_param).collect(),
        is_public,
        line,
    }
}

/// Build struct documentation
fn build_struct_doc(
    name: &str,
    type_params: &[TypeParam],
    fields: &[Field],
    methods: &[Statement],
    line: usize,
    doc: Option<String>,
    comments: &HashMap<usize, CommentBlock>,
) -> StructDoc {
    let field_docs: Vec<FieldDoc> = fields
        .iter()
        .map(|f| FieldDoc {
            name: f.name.clone(),
            type_name: format_type(&f.type_ann),
            is_public: f.is_public,
            is_mutable: f.is_mutable,
        })
        .collect();

    let method_docs: Vec<FunctionDoc> = methods
        .iter()
        .filter_map(|m| {
            if let StatementKind::FnDecl {
                name,
                type_params,
                params,
                return_type,
                is_public,
                ..
            } = &m.kind
            {
                let doc = find_doc_comment(comments, m.span.line);
                Some(build_function_doc(
                    name,
                    type_params,
                    params,
                    return_type.as_ref(),
                    *is_public,
                    m.span.line,
                    doc,
                ))
            } else {
                None
            }
        })
        .collect();

    StructDoc {
        name: name.to_string(),
        doc_comment: doc,
        type_params: type_params.iter().map(format_type_param).collect(),
        fields: field_docs,
        methods: method_docs,
        line,
    }
}

/// Build enum documentation
fn build_enum_doc(
    name: &str,
    variants: &[EnumVariant],
    line: usize,
    doc: Option<String>,
) -> EnumDoc {
    let variant_docs: Vec<VariantDoc> = variants
        .iter()
        .map(|v| VariantDoc {
            name: v.name.clone(),
            fields: v.fields.iter().map(format_type).collect(),
        })
        .collect();

    EnumDoc {
        name: name.to_string(),
        doc_comment: doc,
        variants: variant_docs,
        line,
    }
}

/// Build trait documentation
fn build_trait_doc(
    name: &str,
    type_params: &[TypeParam],
    methods: &[TraitMethod],
    is_public: bool,
    line: usize,
    doc: Option<String>,
) -> TraitDoc {
    let method_docs: Vec<TraitMethodDoc> = methods
        .iter()
        .map(|m| {
            let sig = format_trait_method_signature(m);
            TraitMethodDoc {
                name: m.name.clone(),
                signature: sig,
                has_default: m.default_impl.is_some(),
            }
        })
        .collect();

    TraitDoc {
        name: name.to_string(),
        doc_comment: doc,
        type_params: type_params.iter().map(format_type_param).collect(),
        methods: method_docs,
        is_public,
        line,
    }
}

/// Build impl documentation
fn build_impl_doc(
    type_name: &str,
    trait_name: Option<&String>,
    type_params: &[TypeParam],
    methods: &[Statement],
    line: usize,
    comments: &HashMap<usize, CommentBlock>,
) -> ImplDoc {
    let method_docs: Vec<FunctionDoc> = methods
        .iter()
        .filter_map(|m| {
            if let StatementKind::FnDecl {
                name,
                type_params,
                params,
                return_type,
                is_public,
                ..
            } = &m.kind
            {
                let doc = find_doc_comment(comments, m.span.line);
                Some(build_function_doc(
                    name,
                    type_params,
                    params,
                    return_type.as_ref(),
                    *is_public,
                    m.span.line,
                    doc,
                ))
            } else {
                None
            }
        })
        .collect();

    ImplDoc {
        type_name: type_name.to_string(),
        trait_name: trait_name.cloned(),
        type_params: type_params.iter().map(format_type_param).collect(),
        methods: method_docs,
        line,
    }
}

/// Build class documentation
#[allow(clippy::too_many_arguments)]
fn build_class_doc(
    name: &str,
    parent: Option<&String>,
    interfaces: &[String],
    fields: &[Field],
    methods: &[Statement],
    line: usize,
    doc: Option<String>,
    comments: &HashMap<usize, CommentBlock>,
) -> ClassDoc {
    let field_docs: Vec<FieldDoc> = fields
        .iter()
        .map(|f| FieldDoc {
            name: f.name.clone(),
            type_name: format_type(&f.type_ann),
            is_public: f.is_public,
            is_mutable: f.is_mutable,
        })
        .collect();

    let method_docs: Vec<FunctionDoc> = methods
        .iter()
        .filter_map(|m| {
            if let StatementKind::FnDecl {
                name,
                type_params,
                params,
                return_type,
                is_public,
                ..
            } = &m.kind
            {
                let doc = find_doc_comment(comments, m.span.line);
                Some(build_function_doc(
                    name,
                    type_params,
                    params,
                    return_type.as_ref(),
                    *is_public,
                    m.span.line,
                    doc,
                ))
            } else {
                None
            }
        })
        .collect();

    ClassDoc {
        name: name.to_string(),
        doc_comment: doc,
        parent: parent.cloned(),
        interfaces: interfaces.to_vec(),
        fields: field_docs,
        methods: method_docs,
        line,
    }
}

/// Format a function signature
fn format_function_signature(
    name: &str,
    type_params: &[TypeParam],
    params: &[Parameter],
    return_type: Option<&TypeExpr>,
) -> String {
    let mut sig = String::from("fn ");
    sig.push_str(name);

    if !type_params.is_empty() {
        sig.push('<');
        sig.push_str(
            &type_params
                .iter()
                .map(format_type_param)
                .collect::<Vec<_>>()
                .join(", "),
        );
        sig.push('>');
    }

    sig.push('(');
    sig.push_str(
        &params
            .iter()
            .map(|p| {
                let base = format!("{}: {}", p.name, format_type(&p.type_ann));
                match &p.default {
                    Some(default) => format!("{} = {}", base, format_default_expr(default)),
                    None => base,
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    sig.push(')');

    if let Some(ret) = return_type {
        sig.push_str(" -> ");
        sig.push_str(&format_type(ret));
    }

    sig
}

/// Format a trait method signature
fn format_trait_method_signature(method: &TraitMethod) -> String {
    let mut sig = String::from("fn ");
    sig.push_str(&method.name);
    sig.push('(');

    let mut params_str = Vec::new();
    if method.has_self {
        params_str.push("self".to_string());
    }
    for p in &method.params {
        params_str.push(format!("{}: {}", p.name, format_type(&p.type_ann)));
    }
    sig.push_str(&params_str.join(", "));
    sig.push(')');

    if let Some(ret) = &method.return_type {
        sig.push_str(" -> ");
        sig.push_str(&format_type(ret));
    }

    sig
}

/// Render a default-value expression for documentation (e.g. `x: int = 5`).
///
/// Handles the literals and simple compound expressions that realistically
/// appear as parameter defaults; anything more complex falls back to `...` so
/// the docs never display a misleading value.
fn format_default_expr(expr: &Expression) -> String {
    match &expr.kind {
        ExpressionKind::IntLiteral(n) => n.to_string(),
        ExpressionKind::FloatLiteral(f) => f.to_string(),
        ExpressionKind::StringLiteral(s) => format!("\"{}\"", s),
        ExpressionKind::CharLiteral(c) => format!("'{}'", c),
        ExpressionKind::BoolLiteral(b) => b.to_string(),
        ExpressionKind::Null => "null".to_string(),
        ExpressionKind::Identifier(name) => name.clone(),
        ExpressionKind::Unary { op, operand } => {
            let inner = format_default_expr(operand);
            match op {
                UnaryOp::Neg => format!("-{}", inner),
                UnaryOp::Not => format!("!{}", inner),
                UnaryOp::BitNot => format!("~{}", inner),
                _ => "...".to_string(),
            }
        }
        ExpressionKind::Array(elems) => {
            let items = elems
                .iter()
                .map(format_default_expr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", items)
        }
        ExpressionKind::Tuple(elems) => {
            let items = elems
                .iter()
                .map(format_default_expr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", items)
        }
        _ => "...".to_string(),
    }
}

/// Format a type parameter
fn format_type_param(tp: &TypeParam) -> String {
    if tp.bounds.is_empty() {
        tp.name.clone()
    } else {
        format!("{}: {}", tp.name, tp.bounds.join(" + "))
    }
}

/// Format a type expression to a string
fn format_type(type_expr: &TypeExpr) -> String {
    match &type_expr.kind {
        TypeExprKind::Named(name) => name.clone(),
        TypeExprKind::Generic { name, args } => {
            format!(
                "{}<{}>",
                name,
                args.iter().map(format_type).collect::<Vec<_>>().join(", ")
            )
        }
        TypeExprKind::Optional(inner) => {
            format!("{}?", format_type(inner))
        }
        TypeExprKind::Function {
            params,
            return_type,
        } => {
            format!(
                "fn({}) -> {}",
                params
                    .iter()
                    .map(format_type)
                    .collect::<Vec<_>>()
                    .join(", "),
                format_type(return_type)
            )
        }
        TypeExprKind::Tuple(types) => {
            format!(
                "({})",
                types.iter().map(format_type).collect::<Vec<_>>().join(", ")
            )
        }
        TypeExprKind::Array(inner) => {
            format!("[{}]", format_type(inner))
        }
        TypeExprKind::Result { ok_type, err_type } => {
            format!(
                "Result<{}, {}>",
                format_type(ok_type),
                format_type(err_type)
            )
        }
        TypeExprKind::Path(segments) => segments.join("::"),
        TypeExprKind::Infer => "any".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func_doc<'a>(module: &'a ModuleDoc, name: &str) -> &'a FunctionDoc {
        module
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("function '{}' not found", name))
    }

    #[test]
    fn renders_default_parameter_values() {
        let source = "\
pub fn connect(host: string, port: int = 8080, secure: bool = true) {
}
";
        let module = extract_docs("test.li", source).expect("extraction failed");
        let f = func_doc(&module, "connect");

        // The signature should render the defaults inline.
        assert!(
            f.signature.contains("port: int = 8080"),
            "signature missing int default: {}",
            f.signature
        );
        assert!(
            f.signature.contains("secure: bool = true"),
            "signature missing bool default: {}",
            f.signature
        );

        // The structured param docs should carry the default values.
        let port = f.params.iter().find(|p| p.name == "port").unwrap();
        assert_eq!(port.default_value.as_deref(), Some("8080"));
        let secure = f.params.iter().find(|p| p.name == "secure").unwrap();
        assert_eq!(secure.default_value.as_deref(), Some("true"));
        let host = f.params.iter().find(|p| p.name == "host").unwrap();
        assert_eq!(host.default_value, None);
    }

    #[test]
    fn renders_string_and_array_defaults() {
        let source = "\
pub fn make(label: string = \"none\", tags: list = []) {
}
";
        let module = extract_docs("test.li", source).expect("extraction failed");
        let f = func_doc(&module, "make");
        let label = f.params.iter().find(|p| p.name == "label").unwrap();
        assert_eq!(label.default_value.as_deref(), Some("\"none\""));
        let tags = f.params.iter().find(|p| p.name == "tags").unwrap();
        assert_eq!(tags.default_value.as_deref(), Some("[]"));
    }
}
