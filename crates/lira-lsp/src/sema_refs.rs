//! Scope-aware reference resolution backed by the type checker's
//! [`SemanticTables`](lirac::sema::SemanticTables).
//!
//! Unlike a textual `\b<name>\b` search, this resolves the *binding* under the
//! cursor to its stable [`SymbolId`] and then collects every AST node the
//! checker linked to that same binding. Two same-named identifiers in different
//! scopes therefore never collide, and all uses of one binding — across scopes —
//! are found together.
//!
//! Two resolution mechanisms live here:
//!
//! * **Symbol bindings** (`resolve_symbol_at` / `collect_symbol_ranges`): local
//!   variables, `let`/`var` bindings (including destructuring), function and
//!   lambda parameters, `for`-loop variables, `const`s, function names, and
//!   `match`-pattern binders. These resolve to a stable [`SymbolId`] via the
//!   checker's `symbol_refs` table.
//!
//! * **Struct members** (`resolve_member_at` / `collect_member_ranges`): struct
//!   and class fields and methods. These are *not* symbols; they are identified
//!   by their `(owner_type, member_name)` pair via the checker's
//!   `field_resolution` table, so a field `x` on `Point` is never conflated with
//!   a field `x` on a different type. References include every `obj.member`
//!   access (and `self.member` inside method bodies) plus the declaration site
//!   (the struct field decl or the `impl`/`struct` method name).
//!
//! Types and enum variants are still recorded in separate tables without
//! reference entries and are not covered here; callers degrade to empty results.

use std::collections::HashMap;

use lirac::ast::{
    Block, Expression, ExpressionKind, MatchArm, Pattern, PatternKind, SelectArmKind, Span,
    Statement, StatementKind,
};
use lirac::checker::Type;
use lirac::ids::{NodeId, SymbolId};
use lirac::Analysis;
use tower_lsp::lsp_types::{Position, Range};

use crate::sema_index::Cursor;

/// Resolve the [`SymbolId`] of the binding under the cursor, if any.
///
/// Handles both *uses* (the cursor sits on an identifier reference) and
/// *declarations* (the cursor sits on the bound name): the checker records a
/// `symbol_refs` entry for both, keyed by the relevant AST node id.
pub fn resolve_symbol_at(
    analysis: &Analysis,
    content: &str,
    position: Position,
) -> Option<SymbolId> {
    let cursor = Cursor::from_lsp(position.line, position.character);

    // First try a use site: the innermost expression containing the cursor.
    if let Some(expr) = crate::sema_index::expr_at(&analysis.program, content, cursor) {
        if let Some(id) = analysis.sema.symbol_refs.get(&expr.id) {
            return Some(*id);
        }
    }

    // Otherwise the cursor may be on a declaration node (a pattern, parameter,
    // `fn`/`const`/`for` name) which has no `Expression` wrapper. Find the decl
    // node at the cursor and look it up directly.
    let decl_node = decl_node_at(analysis, content, cursor)?;
    analysis.sema.symbol_refs.get(&decl_node).copied()
}

/// Collect the source [`Range`]s of every node bound to `sym_id`. The
/// declaration site is included iff `include_decl`.
pub fn collect_symbol_ranges(
    analysis: &Analysis,
    content: &str,
    sym_id: SymbolId,
    include_decl: bool,
) -> Vec<Range> {
    let index = build_node_ranges(analysis, content);
    let decl_node = analysis
        .sema
        .symbols
        .get(&sym_id)
        .map(|entry| entry.decl_node);

    let mut ranges = Vec::new();
    for (node, id) in &analysis.sema.symbol_refs {
        if *id != sym_id {
            continue;
        }
        let is_decl = Some(*node) == decl_node;
        if is_decl && !include_decl {
            continue;
        }
        if let Some(range) = index.get(node) {
            ranges.push(*range);
        }
    }

    // Deterministic order (top-to-bottom, left-to-right) so edits/results are
    // stable for tests and editors.
    ranges.sort_by_key(|r| (r.start.line, r.start.character));
    ranges.dedup();
    ranges
}

/// A 0-indexed LSP range covering `name` starting at a 1-indexed AST point.
fn range_from_point(point: Span, name: &str) -> Range {
    let line = point.line.saturating_sub(1) as u32;
    let start = point.column.saturating_sub(1) as u32;
    let len = name.chars().count() as u32;
    Range {
        start: Position {
            line,
            character: start,
        },
        end: Position {
            line,
            character: start + len,
        },
    }
}

/// Build a `NodeId -> Range` index spanning every node that may appear in
/// `symbol_refs`: identifier uses and the declaration sites of supported binding
/// kinds. Each range covers exactly the bound/used name.
fn build_node_ranges(analysis: &Analysis, content: &str) -> HashMap<NodeId, Range> {
    let lines: Vec<&str> = content.lines().collect();
    let mut index = HashMap::new();
    for stmt in &analysis.program.statements {
        walk_statement(stmt, &lines, &mut index);
    }
    index
}

fn insert_name(index: &mut HashMap<NodeId, Range>, node: NodeId, span: Span, name: &str) {
    index.insert(node, range_from_point(span, name));
}

/// For declarations whose span points at a keyword (`fn`/`const`/`for`), locate
/// the bound name on the source line at or after the keyword column.
fn keyword_decl_range(lines: &[&str], span: Span, name: &str) -> Option<Range> {
    if span.line == 0 || span.line > lines.len() {
        return None;
    }
    let chars: Vec<char> = lines[span.line - 1].chars().collect();
    let target: Vec<char> = name.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start_col = span.column.saturating_sub(1);
    let mut i = start_col;
    while i + target.len() <= chars.len() {
        let matches = chars[i..i + target.len()] == target[..];
        let left_ok = i == 0 || !is_word(chars[i - 1]);
        let right_ok = i + target.len() == chars.len() || !is_word(chars[i + target.len()]);
        if matches && left_ok && right_ok {
            return Some(range_from_point(
                Span {
                    line: span.line,
                    column: i + 1,
                },
                name,
            ));
        }
        i += 1;
    }
    None
}

fn walk_statement(stmt: &Statement, lines: &[&str], index: &mut HashMap<NodeId, Range>) {
    match &stmt.kind {
        StatementKind::VarDecl {
            pattern,
            initializer,
            ..
        } => {
            walk_pattern_decl(pattern, index);
            if let Some(init) = initializer {
                walk_expr(init, lines, index);
            }
        }
        StatementKind::ConstDecl {
            name, initializer, ..
        } => {
            if let Some(range) = keyword_decl_range(lines, stmt.span.clone(), name) {
                index.insert(stmt.id, range);
            }
            walk_expr(initializer, lines, index);
        }
        StatementKind::Expression(e) => walk_expr(e, lines, index),
        StatementKind::Return(Some(e)) | StatementKind::Break(Some(e)) => {
            walk_expr(e, lines, index)
        }
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_expr(condition, lines, index);
            walk_block(then_branch, lines, index);
            if let Some(b) = else_branch {
                walk_block(b, lines, index);
            }
        }
        StatementKind::While { condition, body } => {
            walk_expr(condition, lines, index);
            walk_block(body, lines, index);
        }
        StatementKind::For {
            variable,
            iterable,
            body,
        } => {
            if let Some(range) = keyword_decl_range(lines, stmt.span.clone(), variable) {
                index.insert(stmt.id, range);
            }
            walk_expr(iterable, lines, index);
            walk_block(body, lines, index);
        }
        StatementKind::Loop { body } => walk_block(body, lines, index),
        StatementKind::Block(b) => walk_block(b, lines, index),
        StatementKind::FnDecl {
            name, params, body, ..
        } => {
            if let Some(range) = keyword_decl_range(lines, stmt.span.clone(), name) {
                index.insert(stmt.id, range);
            }
            for param in params {
                insert_name(index, param.id, param.span.clone(), &param.name);
            }
            walk_block(body, lines, index);
        }
        StatementKind::ClassDecl { methods, .. }
        | StatementKind::StructDecl { methods, .. }
        | StatementKind::ImplDecl { methods, .. } => {
            for m in methods {
                walk_statement(m, lines, index);
            }
        }
        _ => {}
    }
}

fn walk_block(block: &Block, lines: &[&str], index: &mut HashMap<NodeId, Range>) {
    for stmt in &block.statements {
        walk_statement(stmt, lines, index);
    }
}

/// Record the declaration ranges for the bound names in a `let`/`var` pattern.
fn walk_pattern_decl(pattern: &Pattern, index: &mut HashMap<NodeId, Range>) {
    match &pattern.kind {
        PatternKind::Variable(name) => {
            insert_name(index, pattern.id, pattern.span.clone(), name);
        }
        PatternKind::Binding {
            name,
            pattern: inner,
        } => {
            insert_name(index, pattern.id, pattern.span.clone(), name);
            walk_pattern_decl(inner, index);
        }
        PatternKind::Tuple(patterns)
        | PatternKind::Constructor {
            fields: patterns, ..
        }
        | PatternKind::Or(patterns) => {
            for p in patterns {
                walk_pattern_decl(p, index);
            }
        }
        PatternKind::Struct { fields, .. } => {
            for (_, p) in fields {
                walk_pattern_decl(p, index);
            }
        }
        _ => {}
    }
}

fn walk_expr(expr: &Expression, lines: &[&str], index: &mut HashMap<NodeId, Range>) {
    if let ExpressionKind::Identifier(name) = &expr.kind {
        insert_name(index, expr.id, expr.span.clone(), name);
    }
    match &expr.kind {
        ExpressionKind::Binary { left, right, .. } => {
            walk_expr(left, lines, index);
            walk_expr(right, lines, index);
        }
        ExpressionKind::Unary { operand, .. } => walk_expr(operand, lines, index),
        ExpressionKind::Call { callee, args, .. } => {
            walk_expr(callee, lines, index);
            for a in args {
                walk_expr(&a.value, lines, index);
            }
        }
        ExpressionKind::MethodCall { receiver, args, .. } => {
            walk_expr(receiver, lines, index);
            for a in args {
                walk_expr(&a.value, lines, index);
            }
        }
        ExpressionKind::FieldAccess { object, .. }
        | ExpressionKind::OptionalAccess { object, .. } => walk_expr(object, lines, index),
        ExpressionKind::Index { object, index: idx } => {
            walk_expr(object, lines, index);
            walk_expr(idx, lines, index);
        }
        ExpressionKind::Array(items) | ExpressionKind::Tuple(items) => {
            for e in items {
                walk_expr(e, lines, index);
            }
        }
        ExpressionKind::Map(pairs) => {
            for (k, v) in pairs {
                walk_expr(k, lines, index);
                walk_expr(v, lines, index);
            }
        }
        ExpressionKind::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                walk_expr(e, lines, index);
            }
        }
        ExpressionKind::Lambda { params, body } => {
            for p in params {
                insert_name(index, p.id, p.span.clone(), &p.name);
            }
            walk_expr(body, lines, index);
        }
        ExpressionKind::IfExpr {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_expr(condition, lines, index);
            walk_expr(then_expr, lines, index);
            walk_expr(else_expr, lines, index);
        }
        ExpressionKind::Match { subject, arms } => {
            walk_expr(subject, lines, index);
            for arm in arms {
                walk_match_arm(arm, lines, index);
            }
        }
        ExpressionKind::Range { start, end, .. } => {
            if let Some(s) = start {
                walk_expr(s, lines, index);
            }
            if let Some(e) = end {
                walk_expr(e, lines, index);
            }
        }
        ExpressionKind::Cast { expr, .. } | ExpressionKind::TypeCheck { expr, .. } => {
            walk_expr(expr, lines, index)
        }
        ExpressionKind::Assign { target, value } => {
            walk_expr(target, lines, index);
            walk_expr(value, lines, index);
        }
        ExpressionKind::CompoundAssign { target, value, .. } => {
            walk_expr(target, lines, index);
            walk_expr(value, lines, index);
        }
        ExpressionKind::Block(b) => walk_block(b, lines, index),
        ExpressionKind::Spawn(e) | ExpressionKind::Try(e) => walk_expr(e, lines, index),
        ExpressionKind::Select(arms) => {
            for arm in arms {
                match &arm.kind {
                    SelectArmKind::Recv { channel, .. } => walk_expr(channel, lines, index),
                    SelectArmKind::Send { value, channel } => {
                        walk_expr(value, lines, index);
                        walk_expr(channel, lines, index);
                    }
                    SelectArmKind::Default => {}
                }
                walk_expr(&arm.body, lines, index);
            }
        }
        _ => {}
    }
}

fn walk_match_arm(arm: &MatchArm, lines: &[&str], index: &mut HashMap<NodeId, Range>) {
    walk_pattern_decl(&arm.pattern, index);
    if let Some(g) = &arm.guard {
        walk_expr(g, lines, index);
    }
    walk_expr(&arm.body, lines, index);
}

/// Find the declaration node id at the cursor for a supported binding kind.
///
/// Mirrors the decl-recording walk: it returns the [`NodeId`] of the pattern /
/// parameter / `fn`/`const`/`for` name whose name range covers the cursor.
fn decl_node_at(analysis: &Analysis, content: &str, cursor: Cursor) -> Option<NodeId> {
    let index = build_node_ranges(analysis, content);
    let target = Position {
        line: cursor.line.saturating_sub(1) as u32,
        character: cursor.column.saturating_sub(1) as u32,
    };
    // A node qualifies only if it is a declaration recorded in `symbols`
    // (i.e. its id is some symbol's decl_node) and its range covers the cursor.
    let decl_nodes: std::collections::HashSet<NodeId> = analysis
        .sema
        .symbols
        .values()
        .map(|e| e.decl_node)
        .collect();
    for (node, range) in &index {
        if decl_nodes.contains(node) && covers(range, target) {
            return Some(*node);
        }
    }
    None
}

fn covers(range: &Range, pos: Position) -> bool {
    range.start.line == pos.line
        && pos.character >= range.start.character
        && pos.character < range.end.character
}

// ===========================================================================
// Struct fields & methods (scope-aware by owner type)
// ===========================================================================
//
// Variables/params/fn-names resolve to a [`SymbolId`]; struct *members* (fields
// and methods) are not symbols and instead live in `field_resolution`, keyed by
// `(owner_type, member_name)`. A member reference is therefore identified by
// that pair: two fields both named `x` on different types never collide because
// their owner type differs.
//
// Reference sites are every `FieldAccess` node whose recorded
// [`FieldResolution`](lirac::sema::FieldResolution) matches the pair (this
// includes `self.field` accesses inside method bodies, which the checker records
// in its dedicated pass). The declaration site is the struct field declaration
// or the `impl`/`struct` method name.

/// A struct member (field or method) identified by its owner type and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberKey {
    pub owner: String,
    pub name: String,
}

/// The simple type name backing a `Struct`/`Class` type, if any. Members are
/// only meaningful on these nominal types.
fn owner_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Struct(n) | Type::Class(n) => Some(n.clone()),
        _ => None,
    }
}

/// Resolve the struct member (field or method) under the cursor.
///
/// Handles both a *use* (the cursor sits on the field/method portion of an
/// `obj.member` access) and a *declaration* (the cursor sits on a struct field
/// name or an `impl`/`struct` method name).
pub fn resolve_member_at(
    analysis: &Analysis,
    content: &str,
    position: Position,
) -> Option<MemberKey> {
    let cursor = Cursor::from_lsp(position.line, position.character);

    // Use site: the innermost expression at the cursor is a field/method access
    // whose resolution names the owner type and member.
    if let Some(expr) = crate::sema_index::expr_at(&analysis.program, content, cursor) {
        if let Some(res) = analysis.sema.field_resolution.get(&expr.id) {
            if let Some(owner) = owner_type_name(&res.owner_type) {
                return Some(MemberKey {
                    owner,
                    name: res.field_name.clone(),
                });
            }
        }
    }

    // Declaration site: a struct field decl or an impl/struct method name.
    member_decl_at(&analysis.program, content, cursor)
}

/// Collect the source ranges of every reference to `key`'s member: each matching
/// `obj.member` access, plus the declaration site iff `include_decl`.
pub fn collect_member_ranges(
    analysis: &Analysis,
    content: &str,
    key: &MemberKey,
    include_decl: bool,
) -> Vec<Range> {
    let lines: Vec<&str> = content.lines().collect();
    let access_ranges = build_member_access_ranges(analysis, content);

    let mut ranges = Vec::new();

    // Every field/method access resolved to this exact (owner, name) pair.
    for (node, res) in &analysis.sema.field_resolution {
        let same_owner = owner_type_name(&res.owner_type).as_deref() == Some(key.owner.as_str());
        if same_owner && res.field_name == key.name {
            if let Some(range) = access_ranges.get(node) {
                ranges.push(*range);
            }
        }
    }

    // The declaration site(s): the struct field decl or the method name.
    if include_decl {
        for decl in member_decl_ranges(&analysis.program, &lines, key) {
            ranges.push(decl);
        }
    }

    ranges.sort_by_key(|r| (r.start.line, r.start.character));
    ranges.dedup();
    ranges
}

/// Build a `NodeId -> Range` index over the *member-name* portion of every
/// `FieldAccess`/`OptionalAccess` node. The range covers exactly the field or
/// method name (after the final `.`), not the receiver.
fn build_member_access_ranges(analysis: &Analysis, content: &str) -> HashMap<NodeId, Range> {
    let lines: Vec<&str> = content.lines().collect();
    let mut index = HashMap::new();
    for stmt in &analysis.program.statements {
        collect_access_ranges_stmt(stmt, &lines, &mut index);
    }
    index
}

fn collect_access_ranges_stmt(
    stmt: &Statement,
    lines: &[&str],
    index: &mut HashMap<NodeId, Range>,
) {
    match &stmt.kind {
        StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } => collect_access_ranges_expr(init, lines, index),
        StatementKind::ConstDecl { initializer, .. } => {
            collect_access_ranges_expr(initializer, lines, index)
        }
        StatementKind::Expression(e) => collect_access_ranges_expr(e, lines, index),
        StatementKind::Return(Some(e)) | StatementKind::Break(Some(e)) => {
            collect_access_ranges_expr(e, lines, index)
        }
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_access_ranges_expr(condition, lines, index);
            collect_access_ranges_block(then_branch, lines, index);
            if let Some(b) = else_branch {
                collect_access_ranges_block(b, lines, index);
            }
        }
        StatementKind::While { condition, body } => {
            collect_access_ranges_expr(condition, lines, index);
            collect_access_ranges_block(body, lines, index);
        }
        StatementKind::For { iterable, body, .. } => {
            collect_access_ranges_expr(iterable, lines, index);
            collect_access_ranges_block(body, lines, index);
        }
        StatementKind::Loop { body } => collect_access_ranges_block(body, lines, index),
        StatementKind::Block(b) => collect_access_ranges_block(b, lines, index),
        StatementKind::FnDecl { body, .. } => collect_access_ranges_block(body, lines, index),
        StatementKind::ClassDecl { methods, .. }
        | StatementKind::StructDecl { methods, .. }
        | StatementKind::ImplDecl { methods, .. } => {
            for m in methods {
                collect_access_ranges_stmt(m, lines, index);
            }
        }
        _ => {}
    }
}

fn collect_access_ranges_block(block: &Block, lines: &[&str], index: &mut HashMap<NodeId, Range>) {
    for stmt in &block.statements {
        collect_access_ranges_stmt(stmt, lines, index);
    }
}

fn collect_access_ranges_expr(
    expr: &Expression,
    lines: &[&str],
    index: &mut HashMap<NodeId, Range>,
) {
    if let ExpressionKind::FieldAccess { object, field }
    | ExpressionKind::OptionalAccess { object, field } = &expr.kind
    {
        if let Some(range) = member_name_range(object, field, lines) {
            index.insert(expr.id, range);
        }
    }
    match &expr.kind {
        ExpressionKind::Binary { left, right, .. } => {
            collect_access_ranges_expr(left, lines, index);
            collect_access_ranges_expr(right, lines, index);
        }
        ExpressionKind::Unary { operand, .. } => collect_access_ranges_expr(operand, lines, index),
        ExpressionKind::Call { callee, args, .. } => {
            collect_access_ranges_expr(callee, lines, index);
            for a in args {
                collect_access_ranges_expr(&a.value, lines, index);
            }
        }
        ExpressionKind::MethodCall { receiver, args, .. } => {
            collect_access_ranges_expr(receiver, lines, index);
            for a in args {
                collect_access_ranges_expr(&a.value, lines, index);
            }
        }
        ExpressionKind::FieldAccess { object, .. }
        | ExpressionKind::OptionalAccess { object, .. } => {
            collect_access_ranges_expr(object, lines, index)
        }
        ExpressionKind::Index { object, index: idx } => {
            collect_access_ranges_expr(object, lines, index);
            collect_access_ranges_expr(idx, lines, index);
        }
        ExpressionKind::Array(items) | ExpressionKind::Tuple(items) => {
            for e in items {
                collect_access_ranges_expr(e, lines, index);
            }
        }
        ExpressionKind::Map(pairs) => {
            for (k, v) in pairs {
                collect_access_ranges_expr(k, lines, index);
                collect_access_ranges_expr(v, lines, index);
            }
        }
        ExpressionKind::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                collect_access_ranges_expr(e, lines, index);
            }
        }
        ExpressionKind::Lambda { body, .. } => collect_access_ranges_expr(body, lines, index),
        ExpressionKind::IfExpr {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_access_ranges_expr(condition, lines, index);
            collect_access_ranges_expr(then_expr, lines, index);
            collect_access_ranges_expr(else_expr, lines, index);
        }
        ExpressionKind::Match { subject, arms } => {
            collect_access_ranges_expr(subject, lines, index);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_access_ranges_expr(g, lines, index);
                }
                collect_access_ranges_expr(&arm.body, lines, index);
            }
        }
        ExpressionKind::Range { start, end, .. } => {
            if let Some(s) = start {
                collect_access_ranges_expr(s, lines, index);
            }
            if let Some(e) = end {
                collect_access_ranges_expr(e, lines, index);
            }
        }
        ExpressionKind::Cast { expr, .. } | ExpressionKind::TypeCheck { expr, .. } => {
            collect_access_ranges_expr(expr, lines, index)
        }
        ExpressionKind::Assign { target, value } => {
            collect_access_ranges_expr(target, lines, index);
            collect_access_ranges_expr(value, lines, index);
        }
        ExpressionKind::CompoundAssign { target, value, .. } => {
            collect_access_ranges_expr(target, lines, index);
            collect_access_ranges_expr(value, lines, index);
        }
        ExpressionKind::Block(b) => collect_access_ranges_block(b, lines, index),
        ExpressionKind::Spawn(e) | ExpressionKind::Try(e) => {
            collect_access_ranges_expr(e, lines, index)
        }
        ExpressionKind::Select(arms) => {
            for arm in arms {
                match &arm.kind {
                    SelectArmKind::Recv { channel, .. } => {
                        collect_access_ranges_expr(channel, lines, index)
                    }
                    SelectArmKind::Send { value, channel } => {
                        collect_access_ranges_expr(value, lines, index);
                        collect_access_ranges_expr(channel, lines, index);
                    }
                    SelectArmKind::Default => {}
                }
                collect_access_ranges_expr(&arm.body, lines, index);
            }
        }
        _ => {}
    }
}

/// Range of the member name in `object.field`. The object node carries only a
/// start point, so we locate the name on the source line at or after the
/// object's start column, picking the **last** matching occurrence on the line
/// up to a reasonable bound (handles chained `a.field.field`).
fn member_name_range(object: &Expression, field: &str, lines: &[&str]) -> Option<Range> {
    let line_idx = object.span.line;
    if line_idx == 0 || line_idx > lines.len() {
        return None;
    }
    let chars: Vec<char> = lines[line_idx - 1].chars().collect();
    let target: Vec<char> = field.chars().collect();
    if target.is_empty() {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start_col = object.span.column.saturating_sub(1);

    // Scan forward from the object's start; require the match to be immediately
    // preceded by a `.` so we never pick the receiver identifier itself.
    let mut i = start_col;
    while i + target.len() <= chars.len() {
        let matches = chars[i..i + target.len()] == target[..];
        let dot_before = i > 0 && chars[i - 1] == '.';
        let right_ok = i + target.len() == chars.len() || !is_word(chars[i + target.len()]);
        if matches && dot_before && right_ok {
            return Some(range_from_point(
                Span {
                    line: line_idx,
                    column: i + 1,
                },
                field,
            ));
        }
        i += 1;
    }
    None
}

/// If the cursor sits on a member *declaration* — a struct field name or an
/// `impl`/`struct` method name — resolve its [`MemberKey`].
fn member_decl_at(
    program: &lirac::ast::Program,
    content: &str,
    cursor: Cursor,
) -> Option<MemberKey> {
    let lines: Vec<&str> = content.lines().collect();
    let target = Position {
        line: cursor.line.saturating_sub(1) as u32,
        character: cursor.column.saturating_sub(1) as u32,
    };
    for stmt in &program.statements {
        if let Some(key) = member_decl_in_stmt(stmt, &lines, target) {
            return Some(key);
        }
    }
    None
}

fn member_decl_in_stmt(stmt: &Statement, lines: &[&str], target: Position) -> Option<MemberKey> {
    match &stmt.kind {
        StatementKind::StructDecl {
            name,
            fields,
            methods,
            ..
        }
        | StatementKind::ClassDecl {
            name,
            fields,
            methods,
            ..
        } => {
            for f in fields {
                if let Some(range) = keyword_decl_range(lines, f.span.clone(), &f.name) {
                    if covers(&range, target) {
                        return Some(MemberKey {
                            owner: name.clone(),
                            name: f.name.clone(),
                        });
                    }
                }
            }
            method_name_decl(name, methods, lines, target)
        }
        StatementKind::ImplDecl {
            type_name, methods, ..
        } => method_name_decl(type_name, methods, lines, target),
        _ => None,
    }
}

/// Match the cursor against a method *name* in a list of `FnDecl` methods.
fn method_name_decl(
    owner: &str,
    methods: &[Statement],
    lines: &[&str],
    target: Position,
) -> Option<MemberKey> {
    for m in methods {
        if let StatementKind::FnDecl { name, .. } = &m.kind {
            if let Some(range) = keyword_decl_range(lines, m.span.clone(), name) {
                if covers(&range, target) {
                    return Some(MemberKey {
                        owner: owner.to_string(),
                        name: name.clone(),
                    });
                }
            }
        }
    }
    None
}

/// Declaration-site range(s) for a member: the struct field decl or the method
/// name, restricted to the member's owner type.
fn member_decl_ranges(
    program: &lirac::ast::Program,
    lines: &[&str],
    key: &MemberKey,
) -> Vec<Range> {
    let mut out = Vec::new();
    for stmt in &program.statements {
        match &stmt.kind {
            StatementKind::StructDecl {
                name,
                fields,
                methods,
                ..
            }
            | StatementKind::ClassDecl {
                name,
                fields,
                methods,
                ..
            } if name == &key.owner => {
                for f in fields {
                    if f.name == key.name {
                        if let Some(r) = keyword_decl_range(lines, f.span.clone(), &f.name) {
                            out.push(r);
                        }
                    }
                }
                push_method_name_ranges(methods, &key.name, lines, &mut out);
            }
            StatementKind::ImplDecl {
                type_name, methods, ..
            } if type_name == &key.owner => {
                push_method_name_ranges(methods, &key.name, lines, &mut out);
            }
            _ => {}
        }
    }
    out
}

fn push_method_name_ranges(
    methods: &[Statement],
    member: &str,
    lines: &[&str],
    out: &mut Vec<Range>,
) {
    for m in methods {
        if let StatementKind::FnDecl { name, .. } = &m.kind {
            if name == member {
                if let Some(r) = keyword_decl_range(lines, m.span.clone(), name) {
                    out.push(r);
                }
            }
        }
    }
}
