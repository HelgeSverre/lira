//! Cursor-to-AST-node resolution for sema-powered LSP features.
//!
//! AST spans in Lira are **start-only points** (a `line`/`column`), and every
//! infix/postfix expression inherits its leftmost child's span. A naive
//! "largest span <= cursor" scan therefore cannot distinguish a `FieldAccess`
//! node from the `object` it shares a span with. Instead we descend the tree
//! structurally: children are visited first (deepest wins), and a node's extent
//! is computed from its own start span plus its children's extents and the
//! source text for leaf tokens.

use lirac::ast::{
    Argument, Block, Expression, ExpressionKind, MatchArm, Pattern, PatternKind, Program,
    SelectArmKind, Statement, StatementKind,
};

/// A 1-indexed source position, matching the convention of AST spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub column: usize,
}

impl Cursor {
    /// Convert a 0-indexed LSP position into a 1-indexed cursor.
    pub fn from_lsp(line: u32, character: u32) -> Self {
        Self {
            line: line as usize + 1,
            column: character as usize + 1,
        }
    }
}

/// A point in the source, used as an extent boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Point {
    line: usize,
    column: usize,
}

impl Point {
    fn from_cursor(c: Cursor) -> Self {
        Self {
            line: c.line,
            column: c.column,
        }
    }
}

/// The half-open `[start, end)` extent of an expression in the source.
#[derive(Debug, Clone, Copy)]
struct Extent {
    start: Point,
    end: Point,
}

impl Extent {
    fn contains(&self, p: Point) -> bool {
        p >= self.start && p < self.end
    }
}

/// A variable binding introduced by a `let`/`var` declaration, resolved at the
/// cursor. `name` is the bound identifier; `init` is the initializer expression
/// whose inferred type is the binding's type (for simple, non-destructuring
/// bindings).
pub struct BindingAtCursor<'a> {
    pub name: String,
    pub init: Option<&'a Expression>,
}

/// If the cursor sits on a simple variable binding in a `let`/`var`
/// declaration, return it. This complements [`expr_at`], which only resolves
/// *uses* of identifiers, not their declarations.
pub fn binding_at<'a>(
    program: &'a Program,
    source: &str,
    cursor: Cursor,
) -> Option<BindingAtCursor<'a>> {
    let lines: Vec<&str> = source.lines().collect();
    let word = word_at(&lines, cursor)?;
    for stmt in &program.statements {
        if let Some(b) = binding_in_statement(stmt, &lines, cursor, &word) {
            return Some(b);
        }
    }
    None
}

fn binding_in_statement<'a>(
    stmt: &'a Statement,
    lines: &[&str],
    cursor: Cursor,
    word: &str,
) -> Option<BindingAtCursor<'a>> {
    match &stmt.kind {
        StatementKind::VarDecl {
            pattern,
            initializer,
            ..
        } => {
            if let PatternKind::Variable(name) = &pattern.kind {
                if name == word && cursor.line == pattern.span.line {
                    return Some(BindingAtCursor {
                        name: name.clone(),
                        init: initializer.as_ref(),
                    });
                }
            }
            None
        }
        StatementKind::FnDecl { body, .. } | StatementKind::Loop { body } => {
            binding_in_block(body, lines, cursor, word)
        }
        StatementKind::If {
            then_branch,
            else_branch,
            ..
        } => binding_in_block(then_branch, lines, cursor, word).or_else(|| {
            else_branch
                .as_ref()
                .and_then(|b| binding_in_block(b, lines, cursor, word))
        }),
        StatementKind::While { body, .. } | StatementKind::For { body, .. } => {
            binding_in_block(body, lines, cursor, word)
        }
        StatementKind::Block(b) => binding_in_block(b, lines, cursor, word),
        StatementKind::ClassDecl { methods, .. }
        | StatementKind::StructDecl { methods, .. }
        | StatementKind::ImplDecl { methods, .. } => methods
            .iter()
            .find_map(|m| binding_in_statement(m, lines, cursor, word)),
        _ => None,
    }
}

fn binding_in_block<'a>(
    block: &'a Block,
    lines: &[&str],
    cursor: Cursor,
    word: &str,
) -> Option<BindingAtCursor<'a>> {
    block
        .statements
        .iter()
        .find_map(|s| binding_in_statement(s, lines, cursor, word))
}

/// The identifier word at a 1-indexed cursor, if any.
fn word_at(lines: &[&str], cursor: Cursor) -> Option<String> {
    if cursor.line == 0 || cursor.line > lines.len() {
        return None;
    }
    crate::utils::get_word_at_position(lines[cursor.line - 1], cursor.column.saturating_sub(1))
        .map(|w| w.text)
}

/// Find the innermost [`Expression`] whose extent contains the cursor.
pub fn expr_at<'a>(program: &'a Program, source: &str, cursor: Cursor) -> Option<&'a Expression> {
    let lines: Vec<&str> = source.lines().collect();
    let target = Point::from_cursor(cursor);
    for stmt in &program.statements {
        if let Some(found) = expr_in_statement(stmt, &lines, target) {
            return Some(found);
        }
    }
    None
}

fn expr_in_statement<'a>(
    stmt: &'a Statement,
    lines: &[&str],
    target: Point,
) -> Option<&'a Expression> {
    match &stmt.kind {
        StatementKind::VarDecl { initializer, .. } => initializer
            .as_ref()
            .and_then(|e| expr_in_expr(e, lines, target)),
        StatementKind::ConstDecl { initializer, .. } => expr_in_expr(initializer, lines, target),
        StatementKind::Expression(e) => expr_in_expr(e, lines, target),
        StatementKind::Return(Some(e)) => expr_in_expr(e, lines, target),
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => expr_in_expr(condition, lines, target)
            .or_else(|| expr_in_block(then_branch, lines, target))
            .or_else(|| {
                else_branch
                    .as_ref()
                    .and_then(|b| expr_in_block(b, lines, target))
            }),
        StatementKind::While { condition, body } => {
            expr_in_expr(condition, lines, target).or_else(|| expr_in_block(body, lines, target))
        }
        StatementKind::For { iterable, body, .. } => {
            expr_in_expr(iterable, lines, target).or_else(|| expr_in_block(body, lines, target))
        }
        StatementKind::Loop { body } => expr_in_block(body, lines, target),
        StatementKind::Break(Some(e)) => expr_in_expr(e, lines, target),
        StatementKind::Block(b) => expr_in_block(b, lines, target),
        StatementKind::FnDecl { body, .. } => expr_in_block(body, lines, target),
        StatementKind::ClassDecl { methods, .. }
        | StatementKind::StructDecl { methods, .. }
        | StatementKind::ImplDecl { methods, .. } => methods
            .iter()
            .find_map(|m| expr_in_statement(m, lines, target)),
        _ => None,
    }
}

fn expr_in_block<'a>(block: &'a Block, lines: &[&str], target: Point) -> Option<&'a Expression> {
    block
        .statements
        .iter()
        .find_map(|s| expr_in_statement(s, lines, target))
}

/// Recurse into an expression. Children are visited first so the deepest
/// containing node wins; if no child contains the cursor but the expression's
/// own extent does, the expression itself is returned.
fn expr_in_expr<'a>(expr: &'a Expression, lines: &[&str], target: Point) -> Option<&'a Expression> {
    // Visit children first (deepest match wins).
    if let Some(found) = expr_in_children(expr, lines, target) {
        return Some(found);
    }
    if expr_extent(expr, lines).contains(target) {
        return Some(expr);
    }
    None
}

fn expr_in_args<'a>(args: &'a [Argument], lines: &[&str], target: Point) -> Option<&'a Expression> {
    args.iter()
        .find_map(|a| expr_in_expr(&a.value, lines, target))
}

fn expr_in_children<'a>(
    expr: &'a Expression,
    lines: &[&str],
    target: Point,
) -> Option<&'a Expression> {
    match &expr.kind {
        ExpressionKind::Binary { left, right, .. } => {
            expr_in_expr(left, lines, target).or_else(|| expr_in_expr(right, lines, target))
        }
        ExpressionKind::Unary { operand, .. } => expr_in_expr(operand, lines, target),
        ExpressionKind::Call { callee, args, .. } => {
            expr_in_args(args, lines, target).or_else(|| expr_in_expr(callee, lines, target))
        }
        ExpressionKind::MethodCall { receiver, args, .. } => {
            expr_in_args(args, lines, target).or_else(|| expr_in_expr(receiver, lines, target))
        }
        ExpressionKind::FieldAccess { object, .. }
        | ExpressionKind::OptionalAccess { object, .. } => {
            // The object shares this node's start span; only descend into it when
            // the cursor falls within the object's own (narrower) extent so that
            // the field portion resolves to the FieldAccess node itself.
            let obj_extent = expr_extent(object, lines);
            if obj_extent.contains(target) {
                expr_in_expr(object, lines, target)
            } else {
                None
            }
        }
        ExpressionKind::Index { object, index } => {
            expr_in_expr(index, lines, target).or_else(|| expr_in_expr(object, lines, target))
        }
        ExpressionKind::Array(items) | ExpressionKind::Tuple(items) => {
            items.iter().find_map(|e| expr_in_expr(e, lines, target))
        }
        ExpressionKind::Map(pairs) => pairs.iter().find_map(|(k, v)| {
            expr_in_expr(k, lines, target).or_else(|| expr_in_expr(v, lines, target))
        }),
        ExpressionKind::StructLiteral { fields, .. } => fields
            .iter()
            .find_map(|(_, e)| expr_in_expr(e, lines, target)),
        ExpressionKind::Lambda { body, .. } => expr_in_expr(body, lines, target),
        ExpressionKind::IfExpr {
            condition,
            then_expr,
            else_expr,
        } => expr_in_expr(condition, lines, target)
            .or_else(|| expr_in_expr(then_expr, lines, target))
            .or_else(|| expr_in_expr(else_expr, lines, target)),
        ExpressionKind::Match { subject, arms } => {
            expr_in_expr(subject, lines, target).or_else(|| {
                arms.iter()
                    .find_map(|a| expr_in_match_arm(a, lines, target))
            })
        }
        ExpressionKind::Range { start, end, .. } => start
            .as_ref()
            .and_then(|e| expr_in_expr(e, lines, target))
            .or_else(|| end.as_ref().and_then(|e| expr_in_expr(e, lines, target))),
        ExpressionKind::Cast { expr, .. } | ExpressionKind::TypeCheck { expr, .. } => {
            expr_in_expr(expr, lines, target)
        }
        ExpressionKind::Assign { target: t, value } => {
            expr_in_expr(value, lines, target).or_else(|| expr_in_expr(t, lines, target))
        }
        ExpressionKind::CompoundAssign {
            target: t, value, ..
        } => expr_in_expr(value, lines, target).or_else(|| expr_in_expr(t, lines, target)),
        ExpressionKind::Block(b) => expr_in_block_expr(b, lines, target),
        ExpressionKind::Spawn(e) | ExpressionKind::Try(e) => expr_in_expr(e, lines, target),
        ExpressionKind::Select(arms) => arms.iter().find_map(|arm| {
            let from_kind = match &arm.kind {
                SelectArmKind::Recv { channel, .. } => expr_in_expr(channel, lines, target),
                SelectArmKind::Send { value, channel } => expr_in_expr(value, lines, target)
                    .or_else(|| expr_in_expr(channel, lines, target)),
                SelectArmKind::Default => None,
            };
            from_kind.or_else(|| expr_in_expr(&arm.body, lines, target))
        }),
        _ => None,
    }
}

fn expr_in_block_expr<'a>(
    block: &'a Block,
    lines: &[&str],
    target: Point,
) -> Option<&'a Expression> {
    block
        .statements
        .iter()
        .find_map(|s| expr_in_statement(s, lines, target))
}

fn expr_in_match_arm<'a>(
    arm: &'a MatchArm,
    lines: &[&str],
    target: Point,
) -> Option<&'a Expression> {
    arm.guard
        .as_ref()
        .and_then(|g| expr_in_expr(g, lines, target))
        .or_else(|| expr_in_pattern(&arm.pattern, lines, target))
        .or_else(|| expr_in_expr(&arm.body, lines, target))
}

fn expr_in_pattern<'a>(
    pattern: &'a Pattern,
    lines: &[&str],
    target: Point,
) -> Option<&'a Expression> {
    match &pattern.kind {
        PatternKind::Literal(e) => expr_in_expr(e, lines, target),
        _ => None,
    }
}

/// Compute the half-open extent of an expression.
///
/// Start is the node's own span; end is the maximum of the start advanced past
/// the leaf token text and every child's extent end. This makes parent nodes
/// strictly enclose their children even though only start points are stored.
fn expr_extent(expr: &Expression, lines: &[&str]) -> Extent {
    let start = Point {
        line: expr.span.line,
        column: expr.span.column,
    };

    let mut end = leaf_end(expr, lines, start);

    let mut extend = |e: &Expression| {
        let child = expr_extent(e, lines);
        if child.end > end {
            end = child.end;
        }
    };

    match &expr.kind {
        ExpressionKind::Binary { left, right, .. } => {
            extend(left);
            extend(right);
        }
        ExpressionKind::Unary { operand, .. } => extend(operand),
        ExpressionKind::Call { callee, args, .. } => {
            extend(callee);
            for a in args {
                extend(&a.value);
            }
        }
        ExpressionKind::MethodCall { receiver, args, .. } => {
            extend(receiver);
            for a in args {
                extend(&a.value);
            }
        }
        ExpressionKind::FieldAccess { object, field }
        | ExpressionKind::OptionalAccess { object, field } => {
            extend(object);
            // Extend past `.field`. The object's end + 1 (dot) gives the field
            // start; advance by the field name length.
            let obj_end = expr_extent(object, lines).end;
            let field_end = Point {
                line: obj_end.line,
                column: obj_end.column + 1 + field.chars().count(),
            };
            if field_end > end {
                end = field_end;
            }
        }
        ExpressionKind::Index { object, index } => {
            extend(object);
            extend(index);
        }
        ExpressionKind::Array(items) | ExpressionKind::Tuple(items) => {
            for e in items {
                extend(e);
            }
        }
        ExpressionKind::Map(pairs) => {
            for (k, v) in pairs {
                extend(k);
                extend(v);
            }
        }
        ExpressionKind::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                extend(e);
            }
        }
        ExpressionKind::Lambda { body, .. } => extend(body),
        ExpressionKind::IfExpr {
            condition,
            then_expr,
            else_expr,
        } => {
            extend(condition);
            extend(then_expr);
            extend(else_expr);
        }
        ExpressionKind::Match { subject, arms } => {
            extend(subject);
            for arm in arms {
                extend(&arm.body);
            }
        }
        ExpressionKind::Range { start, end: e, .. } => {
            if let Some(s) = start {
                extend(s);
            }
            if let Some(e) = e {
                extend(e);
            }
        }
        ExpressionKind::Cast { expr, .. } | ExpressionKind::TypeCheck { expr, .. } => extend(expr),
        ExpressionKind::Assign { target, value } => {
            extend(target);
            extend(value);
        }
        ExpressionKind::CompoundAssign { target, value, .. } => {
            extend(target);
            extend(value);
        }
        ExpressionKind::Spawn(e) | ExpressionKind::Try(e) => extend(e),
        _ => {}
    }

    Extent { start, end }
}

/// Compute the end point of a leaf token (identifier / literal / keyword-ish
/// expression) by reading the source text at the node's start span.
fn leaf_end(expr: &Expression, lines: &[&str], start: Point) -> Point {
    let len = match &expr.kind {
        ExpressionKind::Identifier(name) => name.chars().count(),
        ExpressionKind::IntLiteral(v) => v.to_string().chars().count(),
        ExpressionKind::FloatLiteral(v) => v.to_string().chars().count(),
        ExpressionKind::BoolLiteral(v) => {
            if *v {
                4
            } else {
                5
            }
        }
        ExpressionKind::StringLiteral(s) => s.chars().count() + 2, // quotes
        ExpressionKind::CharLiteral(_) => 3,                       // 'x'
        ExpressionKind::Null => 4,
        ExpressionKind::EnumVariant {
            enum_name,
            variant_name,
        } => enum_name.chars().count() + 2 + variant_name.chars().count(),
        ExpressionKind::Path { segments } => {
            let seg: usize = segments.iter().map(|s| s.chars().count()).sum();
            seg + 2 * segments.len().saturating_sub(1)
        }
        // For non-leaf nodes use the word under the start position, if any, as a
        // minimal extent; children will extend `end` further.
        _ => word_len_at(lines, start),
    };
    Point {
        line: start.line,
        column: start.column + len.max(1),
    }
}

/// Length of the identifier word at a 1-indexed source point, or 1 if none.
fn word_len_at(lines: &[&str], p: Point) -> usize {
    if p.line == 0 || p.line > lines.len() {
        return 1;
    }
    let chars: Vec<char> = lines[p.line - 1].chars().collect();
    let col0 = p.column.saturating_sub(1);
    if col0 >= chars.len() {
        return 1;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if !is_word(chars[col0]) {
        return 1;
    }
    let mut end = col0;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    end - col0
}

/// The 0-indexed LSP range of an expression's extent, for hover highlighting.
pub fn expr_range(expr: &Expression, source: &str) -> tower_lsp::lsp_types::Range {
    use tower_lsp::lsp_types::{Position, Range};
    let lines: Vec<&str> = source.lines().collect();
    let extent = expr_extent(expr, &lines);
    Range {
        start: Position {
            line: extent.start.line.saturating_sub(1) as u32,
            character: extent.start.column.saturating_sub(1) as u32,
        },
        end: Position {
            line: extent.end.line.saturating_sub(1) as u32,
            character: extent.end.column.saturating_sub(1) as u32,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lirac::ast::ExpressionKind;

    fn parse(source: &str) -> Program {
        lirac::parse_source(source).expect("parses")
    }

    fn at<'a>(program: &'a Program, source: &str, line: usize, col: usize) -> &'a Expression {
        expr_at(program, source, Cursor { line, column: col }).expect("an expression at cursor")
    }

    #[test]
    fn field_access_disambiguation() {
        // `let z = p.x` — column 9 is `p`, column 11 is `x`.
        let src = "struct P { x: int }\nlet p = P { x: 1 }\nlet z = p.x";
        let program = parse(src);

        let on_p = at(&program, src, 3, 9);
        assert!(matches!(&on_p.kind, ExpressionKind::Identifier(n) if n == "p"));

        let on_x = at(&program, src, 3, 11);
        match &on_x.kind {
            ExpressionKind::FieldAccess { field, .. } => assert_eq!(field, "x"),
            other => panic!("expected FieldAccess, got {:?}", other),
        }
    }

    #[test]
    fn binary_literal_and_whole() {
        // `let x = 1 + 2`
        let src = "let x = 1 + 2";
        let program = parse(src);

        let on_one = at(&program, src, 1, 9);
        assert!(matches!(&on_one.kind, ExpressionKind::IntLiteral(1)));

        // On the `+` operator (col 11) the only containing extent is the Binary.
        let on_plus = at(&program, src, 1, 11);
        assert!(matches!(&on_plus.kind, ExpressionKind::Binary { .. }));
    }

    #[test]
    fn call_argument_and_callee() {
        let src = "fn foo(bar: int) -> int { bar }\nlet r = foo(1)";
        let program = parse(src);

        // `foo` callee on line 2 col 9.
        let on_callee = at(&program, src, 2, 9);
        assert!(matches!(&on_callee.kind, ExpressionKind::Identifier(n) if n == "foo"));
    }

    #[test]
    fn whitespace_yields_none() {
        let src = "let x = 1 + 2";
        let program = parse(src);
        // Way past end of line.
        assert!(expr_at(
            &program,
            src,
            Cursor {
                line: 1,
                column: 80
            }
        )
        .is_none());
        // Past last line.
        assert!(expr_at(
            &program,
            src,
            Cursor {
                line: 50,
                column: 1
            }
        )
        .is_none());
    }

    #[test]
    fn nested_field_access() {
        // `a.b.c` — cursor on `b` is the inner FieldAccess(a.b), on `c` the outer.
        let src =
            "struct A { b: B }\nstruct B { c: int }\nlet a = A { b: B { c: 1 } }\nlet r = a.b.c";
        let program = parse(src);

        // line 4: `let r = a.b.c`  a@9 .@10 b@11 .@12 c@13
        let on_b = at(&program, src, 4, 11);
        match &on_b.kind {
            ExpressionKind::FieldAccess { object, field } => {
                assert_eq!(field, "b");
                assert!(matches!(&object.kind, ExpressionKind::Identifier(n) if n == "a"));
            }
            other => panic!("expected inner FieldAccess(a.b), got {:?}", other),
        }

        let on_c = at(&program, src, 4, 13);
        match &on_c.kind {
            ExpressionKind::FieldAccess { field, .. } => assert_eq!(field, "c"),
            other => panic!("expected outer FieldAccess(_.c), got {:?}", other),
        }
    }
}
