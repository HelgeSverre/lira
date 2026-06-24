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

    let sym_id = match sema_refs::resolve_symbol_at(&analysis, content, position) {
        Some(id) => id,
        None => return vec![],
    };

    sema_refs::collect_symbol_ranges(&analysis, content, sym_id, include_declaration)
        .into_iter()
        .map(|range| Location {
            uri: uri.clone(),
            range,
        })
        .collect()
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
