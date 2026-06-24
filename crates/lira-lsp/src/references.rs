//! Find references support for Lira.
//!
//! Scope-aware: resolves the binding under the cursor to a stable `SymbolId`
//! (via the type checker's [`SemanticTables`](lirac::sema)) and returns every
//! node bound to that same symbol. Same-named identifiers in unrelated scopes
//! are never conflated.

use crate::sema_refs;
use tower_lsp::lsp_types::*;

/// Find all references to the symbol at the given position.
///
/// Returns an empty vector when the buffer fails to parse or the cursor is not
/// on a resolvable binding (e.g. a keyword, a struct field, or whitespace), so
/// the editor stays responsive.
pub fn find_references(
    uri: &Url,
    content: &str,
    position: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let analysis = match lirac::analyze(content) {
        Ok(a) => a,
        Err(_) => return vec![],
    };

    // First try a variable/param/fn-name binding (resolved to a `SymbolId`).
    if let Some(sym_id) = sema_refs::resolve_symbol_at(&analysis, content, position) {
        return sema_refs::collect_symbol_ranges(&analysis, content, sym_id, include_declaration)
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range,
            })
            .collect();
    }

    // Otherwise the cursor may be on a struct field or method (scoped by owner
    // type), which is not a `SymbolId` but a `(owner_type, member_name)` pair.
    if let Some(key) = sema_refs::resolve_member_at(&analysis, content, position) {
        return sema_refs::collect_member_ranges(&analysis, content, &key, include_declaration)
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range,
            })
            .collect();
    }

    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri() -> Url {
        Url::parse("file:///test.li").unwrap()
    }

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn test_find_function_references() {
        let content = "fn add(a: int, b: int) -> int {\n    a + b\n}\n\nfn main() {\n    let x = add(1, 2)\n    let y = add(3, 4)\n    println(add(x, y))\n}\n";
        // Cursor on the `add` call site at line 5 (0-indexed), col 12.
        let refs = find_references(&uri(), content, pos(5, 12), true);
        // Definition + 3 call sites = 4.
        assert_eq!(refs.len(), 4);
    }

    #[test]
    fn test_find_references_exclude_declaration() {
        let content = "fn add(a: int, b: int) -> int {\n    a + b\n}\n\nfn main() {\n    let x = add(1, 2)\n    let y = add(3, 4)\n}\n";
        let refs = find_references(&uri(), content, pos(5, 12), false);
        // Only the two call sites, not the definition.
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_shadowed_inner_binding_excludes_outer() {
        // Two distinct `x` bindings: an outer top-level one and an inner one
        // inside `f`. Find-references on the inner `x` must return only the
        // inner declaration + use, never the outer.
        let content = "let x = 1\nfn f() -> int {\n    let x = 2\n    return x\n}\nlet y = x\n";
        // Cursor on `return x` (line 3, col 11).
        let inner = find_references(&uri(), content, pos(3, 11), true);
        // Inner decl (`let x = 2`) + inner use (`return x`) = 2.
        assert_eq!(inner.len(), 2);
        // None of them is the outer `let x = 1` (line 0).
        assert!(inner.iter().all(|loc| loc.range.start.line != 0));
        assert!(inner.iter().all(|loc| loc.range.start.line != 5));
    }

    #[test]
    fn test_outer_binding_excludes_inner() {
        let content = "let x = 1\nfn f() -> int {\n    let x = 2\n    return x\n}\nlet y = x\n";
        // Cursor on the outer use `let y = x` (line 5, col 8).
        let outer = find_references(&uri(), content, pos(5, 8), true);
        // Outer decl (line 0) + outer use (line 5) = 2.
        assert_eq!(outer.len(), 2);
        // Inner lines (2 = `let x = 2`, 3 = `return x`) are excluded.
        assert!(outer.iter().all(|loc| loc.range.start.line != 2));
        assert!(outer.iter().all(|loc| loc.range.start.line != 3));
    }

    #[test]
    fn test_parameter_references() {
        let content = "fn add(a: int, b: int) -> int {\n    return a + a + b\n}\n";
        // Cursor on parameter `a` declaration (line 0, col 7).
        let refs = find_references(&uri(), content, pos(0, 7), true);
        // Declaration + two uses of `a` = 3.
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn test_unicode_references() {
        let content = "fn main() {\n    let 变量 = 5\n    println(变量)\n    let x = 变量 + 1\n}\n";
        // Cursor on the declaration `let 变量` (line 1, col 8).
        let refs = find_references(&uri(), content, pos(1, 8), true);
        // Declaration + 2 uses = 3.
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn test_keyword_yields_no_references() {
        let content = "let x = 42\n";
        // Cursor on `let`.
        let refs = find_references(&uri(), content, pos(0, 0), true);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_string_content_not_matched() {
        // The word `test` appears inside a string but must not be treated as a
        // reference to the function `test`.
        let content = "fn test() {\n    println(\"test is a word\")\n    test()\n}\n";
        // Cursor on the call `test()` (line 2, col 4).
        let refs = find_references(&uri(), content, pos(2, 4), true);
        // Definition + call = 2 (string occurrence excluded).
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_for_loop_variable_uses() {
        let content = "fn main() {\n    for i in 0..3 {\n        println(i)\n    }\n}\n";
        // Cursor on the use `println(i)` (line 2, col 16).
        let refs = find_references(&uri(), content, pos(2, 16), false);
        // At least the one use is found (decl range support for `for` is
        // best-effort; uses must always resolve).
        assert!(refs.iter().any(|loc| loc.range.start.line == 2));
    }

    // ---- Struct fields & methods (scope-aware by owner type) ----

    #[test]
    fn test_find_field_references_includes_decl_and_accesses() {
        // (a) find-references on `p.x` (Point.x) returns all Point.x accesses
        // plus the field declaration.
        let content = "struct Point { x: int, y: int }\nfn main() {\n    let p = Point { x: 1, y: 2 }\n    let a = p.x\n    let b = p.x\n}\n";
        // Cursor on `p.x` field name (line 3: `    let a = p.x`), `x` at col 14.
        let refs = find_references(&uri(), content, pos(3, 14), true);
        // Field decl (line 0) + struct-literal field (line 2) + two accesses
        // (lines 3, 4) = 4. The struct-literal `x` must be renamed too, or
        // `Point { x: ... }` would reference the old name after a rename.
        assert_eq!(refs.len(), 4, "got {:?}", refs);
        assert!(
            refs.iter().any(|l| l.range.start.line == 0),
            "decl included"
        );
        assert!(
            refs.iter().any(|l| l.range.start.line == 2),
            "struct-literal field included"
        );
        assert!(refs.iter().any(|l| l.range.start.line == 3));
        assert!(refs.iter().any(|l| l.range.start.line == 4));
    }

    #[test]
    fn test_find_field_references_isolated_from_same_named_field_on_other_type() {
        // (d) `x` on Point must NOT be conflated with `x` on Box.
        let content = "struct Point { x: int }\nstruct Box { x: int }\nfn main() {\n    let p = Point { x: 1 }\n    let q = Box { x: 9 }\n    let a = p.x\n    let b = q.x\n}\n";
        // Cursor on `p.x` (line 5: `    let a = p.x`), `x` at col 14.
        let refs = find_references(&uri(), content, pos(5, 14), true);
        // Point.x decl (line 0) + Point literal field (line 3) + the single
        // Point access (line 5) = 3.
        assert_eq!(refs.len(), 3, "got {:?}", refs);
        assert!(
            refs.iter().any(|l| l.range.start.line == 3),
            "Point literal field included"
        );
        // Never Box.x: its decl (line 1), its literal (line 4), or access (line 6).
        assert!(refs.iter().all(|l| l.range.start.line != 1));
        assert!(refs.iter().all(|l| l.range.start.line != 4));
        assert!(refs.iter().all(|l| l.range.start.line != 6));
    }

    #[test]
    fn test_find_method_references_includes_impl_decl_and_call_sites() {
        // (c-as-references) find-references on a method call returns the impl
        // decl plus every call site on that type.
        let content = "struct Point { x: int }\nimpl Point {\n    fn get(self) -> int { self.x }\n}\nfn main() {\n    let p = Point { x: 1 }\n    let a = p.get()\n    let b = p.get()\n}\n";
        // Cursor on `p.get()` (line 6: `    let a = p.get()`), `get` at col 14.
        let refs = find_references(&uri(), content, pos(6, 14), true);
        // impl method decl (line 2) + two call sites (lines 6, 7) = 3.
        assert_eq!(refs.len(), 3, "got {:?}", refs);
        assert!(refs.iter().any(|l| l.range.start.line == 2), "impl decl");
        assert!(refs.iter().any(|l| l.range.start.line == 6));
        assert!(refs.iter().any(|l| l.range.start.line == 7));
    }

    #[test]
    fn test_field_references_from_inside_method_body() {
        // A `self.x` access inside a method body counts as a reference to the
        // field, scoped to the owner type.
        let content = "struct Point { x: int }\nimpl Point {\n    fn get(self) -> int { self.x }\n}\nfn main() {\n    let p = Point { x: 1 }\n    let a = p.x\n}\n";
        // Cursor on the field decl `x` (line 0, col 15).
        let refs = find_references(&uri(), content, pos(0, 15), true);
        // decl (line 0) + self.x (line 2) + literal (line 5) + p.x (line 6) = 4.
        assert_eq!(refs.len(), 4, "got {:?}", refs);
        assert!(
            refs.iter().any(|l| l.range.start.line == 2),
            "self.x access"
        );
        assert!(
            refs.iter().any(|l| l.range.start.line == 5),
            "struct-literal field"
        );
    }

    #[test]
    fn test_lambda_parameter_scoped() {
        // Two lambdas each bind `n`; a reference on one must not pull in the
        // other.
        let content = "fn main() {\n    let f = |n: int| n + 1\n    let g = |n: int| n + 2\n}\n";
        // Cursor on `n` inside the first lambda body. Line 1:
        // `    let f = |n: int| n + 1` — the body `n` is at col 21.
        let refs = find_references(&uri(), content, pos(1, 21), true);
        // Only occurrences on line 1, never line 2.
        assert!(!refs.is_empty());
        assert!(refs.iter().all(|loc| loc.range.start.line == 1));
    }
}
