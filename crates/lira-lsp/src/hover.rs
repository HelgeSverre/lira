//! Hover support for Lira
//!
//! Provides type information and documentation on hover, powered by the
//! semantic tables produced by the checker. When the cursor resolves to an
//! expression node, the hover shows its inferred type (and, where available,
//! the resolved field/method/call signature). When no expression resolves
//! (e.g. the cursor is on a keyword, builtin, or type name), it falls back to
//! the static keyword/type/builtin documentation.

use crate::sema_index::{self, Cursor};
use crate::utils::{self, WordInfo};
use lirac::ast::{Expression, ExpressionKind};
use lirac::sema::{CallResolution, SemanticTables, SymbolKind};
use tower_lsp::lsp_types::*;

/// Get hover information using a precomputed (cached) analysis when available.
///
/// Falls back to the static keyword/type/builtin documentation when no
/// expression node resolves under the cursor.
pub fn get_hover_with(
    content: &str,
    position: Position,
    analysis: Option<&lirac::Analysis>,
) -> Option<Hover> {
    // Try sema-powered hover first: resolve the expression node under the cursor.
    if let Some(analysis) = analysis {
        let cursor = Cursor::from_lsp(position.line, position.character);
        if let Some(expr) = sema_index::expr_at(&analysis.program, content, cursor) {
            if let Some(markdown) = hover_for_expr(expr, &analysis.sema) {
                let range = sema_index::expr_range(expr, content);
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: Some(range),
                });
            }
        }

        // No use-site expression resolved: the cursor may be on a `let`/`var`
        // binding declaration, whose type is the initializer's inferred type.
        if let Some(binding) = sema_index::binding_at(&analysis.program, content, cursor) {
            let ty = binding
                .init
                .and_then(|init| analysis.sema.expr_types.get(&init.id));
            if let Some(ty) = ty {
                let markdown = code_block(
                    &format!("{}: {}", binding.name, ty.display_name()),
                    Some("variable"),
                );
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: None,
                });
            }
        }
    }

    // Fallback: static documentation for keywords / builtin types / builtins.
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at the cursor position
    let word_info = utils::get_word_at_position(line, col)?;

    // Look up documentation for the word
    let (markdown, range) = get_word_documentation(&word_info, position.line)?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(range),
    })
}

/// Build hover markdown for a resolved expression from the semantic tables.
fn hover_for_expr(expr: &Expression, sema: &SemanticTables) -> Option<String> {
    match &expr.kind {
        ExpressionKind::Identifier(name) => {
            let ty = sema.expr_types.get(&expr.id)?;
            let label = sema
                .symbol_refs
                .get(&expr.id)
                .and_then(|sym_id| sema.symbols.get(sym_id))
                .map(|sym| symbol_kind_label(&sym.kind));
            let signature = format!("{}: {}", name, ty.display_name());
            Some(code_block(&signature, label))
        }
        ExpressionKind::FieldAccess { field, .. }
        | ExpressionKind::OptionalAccess { field, .. } => {
            if let Some(res) = sema.field_resolution.get(&expr.id) {
                let kind = if res.is_method { "method" } else { "field" };
                let signature = format!(
                    "{}.{}: {}",
                    res.owner_type.display_name(),
                    field,
                    res.resolved_type.display_name()
                );
                Some(code_block(&signature, Some(kind)))
            } else {
                let ty = sema.expr_types.get(&expr.id)?;
                Some(code_block(
                    &format!("{}: {}", field, ty.display_name()),
                    None,
                ))
            }
        }
        ExpressionKind::Call { .. } | ExpressionKind::MethodCall { .. } => {
            let ret = sema.expr_types.get(&expr.id);
            match sema.call_resolution.get(&expr.id) {
                Some(CallResolution::Function { name }) => {
                    Some(call_signature(name, ret, sema, None))
                }
                Some(CallResolution::Method {
                    type_name,
                    method_name,
                })
                | Some(CallResolution::StaticMethod {
                    type_name,
                    method_name,
                }) => Some(call_signature(method_name, ret, sema, Some(type_name))),
                None => ret.map(|t| code_block(&t.display_name(), None)),
            }
        }
        _ => {
            // Any other expression: show its inferred type.
            let ty = sema.expr_types.get(&expr.id)?;
            Some(code_block(&ty.display_name(), None))
        }
    }
}

/// Render a `fn name(params) -> ret` signature for a resolved call.
fn call_signature(
    name: &str,
    ret: Option<&lirac::checker::Type>,
    sema: &SemanticTables,
    owner: Option<&str>,
) -> String {
    // Look up parameter types from the member table when the owner is known.
    let params = owner.and_then(|owner| {
        sema.type_members
            .get(owner)
            .and_then(|members| members.methods.iter().find(|m| m.name == name))
            .map(|member| match &member.ty {
                lirac::checker::Type::Function { params, .. } => params
                    .iter()
                    .map(|t| t.display_name())
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => String::new(),
            })
    });
    let ret_str = ret
        .map(|t| t.display_name())
        .unwrap_or_else(|| "?".to_string());
    let signature = match params {
        Some(p) => format!("fn {}({}) -> {}", name, p, ret_str),
        None => format!("fn {}(...) -> {}", name, ret_str),
    };
    let label = owner.map(|_| "method").unwrap_or("function");
    code_block(&signature, Some(label))
}

/// Wrap a signature in a ```lira code block, optionally prefixed by a kind tag.
fn code_block(signature: &str, kind: Option<&str>) -> String {
    match kind {
        Some(kind) => format!("```lira\n{}\n```\n\n*{}*", signature, kind),
        None => format!("```lira\n{}\n```", signature),
    }
}

fn symbol_kind_label(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Variable => "variable",
        SymbolKind::Function => "function",
        SymbolKind::Parameter => "parameter",
        SymbolKind::Type => "type",
        SymbolKind::Field => "field",
        SymbolKind::Method => "method",
        SymbolKind::Constant => "constant",
    }
}

fn get_word_documentation(word_info: &WordInfo, line_num: u32) -> Option<(String, Range)> {
    let range = Range {
        start: Position {
            line: line_num,
            character: word_info.start_col as u32,
        },
        end: Position {
            line: line_num,
            character: word_info.end_col as u32,
        },
    };

    // Look up keyword documentation
    if let Some(doc) = keyword_docs(&word_info.text) {
        return Some((doc, range));
    }

    // Look up type documentation
    if let Some(doc) = type_docs(&word_info.text) {
        return Some((doc, range));
    }

    // Look up built-in function documentation
    if let Some(doc) = builtin_docs(&word_info.text) {
        return Some((doc, range));
    }

    None
}

fn keyword_docs(keyword: &str) -> Option<String> {
    let doc = match keyword {
        "fn" => "**fn** - Function declaration\n\n```lira\nfn name(param: Type) -> ReturnType {\n    // body\n}\n```",
        "let" => "**let** - Immutable variable binding\n\n```lira\nlet x = 42\nlet y: int = 10\n```\n\nBindings declared with `let` cannot be reassigned.",
        "var" => "**var** - Mutable variable binding\n\n```lira\nvar count = 0\ncount = count + 1\n```\n\nBindings declared with `var` can be reassigned.",
        "const" => "**const** - Compile-time constant\n\n```lira\nconst PI = 3.14159\nconst MAX_SIZE = 100\n```",
        "struct" => "**struct** - Product type with named fields\n\n```lira\nstruct Point {\n    x: float,\n    y: float,\n}\n```",
        "class" => "**class** - Class with methods and inheritance\n\n```lira\nclass Animal {\n    fn speak(self) { }\n}\n\nclass Dog extends Animal {\n    fn speak(self) {\n        println(\"Woof!\")\n    }\n}\n```",
        "enum" => "**enum** - Sum type with variants\n\n```lira\nenum Option<T> {\n    Some(T),\n    None,\n}\n```",
        "trait" => "**trait** - Interface definition\n\n```lira\ntrait Display {\n    fn display(self) -> string\n}\n```",
        "impl" => "**impl** - Implementation block\n\n```lira\nimpl Point {\n    fn new(x: float, y: float) -> Point {\n        Point { x, y }\n    }\n}\n\nimpl Display for Point {\n    fn display(self) -> string { ... }\n}\n```",
        "if" => "**if** - Conditional expression\n\n```lira\nif condition {\n    // then branch\n} else {\n    // else branch\n}\n```\n\nIf is an expression and returns a value.",
        "else" => "**else** - Else branch of conditional",
        "match" => "**match** - Pattern matching expression\n\n```lira\nmatch value {\n    0 => \"zero\",\n    n if n > 0 => \"positive\",\n    _ => \"negative\",\n}\n```",
        "while" => "**while** - Loop with condition\n\n```lira\nwhile condition {\n    // body\n}\n```",
        "for" => "**for** - Iterator loop\n\n```lira\nfor item in collection {\n    // body\n}\n\nfor i in 0..10 {\n    println(i)\n}\n```",
        "loop" => "**loop** - Infinite loop\n\n```lira\nloop {\n    if done {\n        break\n    }\n}\n```\n\nUse `break` to exit the loop.",
        "break" => "**break** - Exit a loop\n\n```lira\nloop {\n    if condition {\n        break\n    }\n}\n```\n\nCan optionally return a value: `break value`",
        "continue" => "**continue** - Skip to next iteration\n\n```lira\nfor i in 0..10 {\n    if i % 2 == 0 {\n        continue\n    }\n    println(i)\n}\n```",
        "return" => "**return** - Return from function\n\n```lira\nfn add(a: int, b: int) -> int {\n    return a + b\n}\n```\n\nThe last expression is implicitly returned if no `return` is used.",
        "spawn" => "**spawn** - Create a new fiber\n\n```lira\nspawn {\n    // runs concurrently\n    process()\n}\n```\n\nFibers are lightweight green threads.",
        "select" => "**select** - Wait on multiple channels\n\n```lira\nselect {\n    recv: msg = <-ch1 => handle(msg),\n    recv: msg = <-ch2 => handle(msg),\n    default => timeout(),\n}\n```",
        "try" => "**try** - Error handling block\n\n```lira\ntry {\n    risky_operation()\n} catch Error as e {\n    handle(e)\n} finally {\n    cleanup()\n}\n```",
        "catch" => "**catch** - Catch exceptions\n\n```lira\ntry {\n    risky_operation()\n} catch Error as e {\n    handle(e)\n}\n```",
        "import" => "**import** - Import modules\n\n```lira\nimport std.io\nimport std.collections.{List, Map}\n```",
        "use" => "**use** - Use declaration\n\n```lira\nuse std.io.File\n```",
        "pub" => "**pub** - Public visibility modifier",
        "priv" => "**priv** - Private visibility modifier",
        "async" => "**async** - Asynchronous function modifier",
        "as" => "**as** - Type cast or import alias\n\n```lira\nlet x = value as int\nimport long.path.Module as M\n```",
        "is" => "**is** - Type check\n\n```lira\nif value is int {\n    println(\"It's an int\")\n}\n```",
        "self" => "**self** - Reference to current instance\n\n```lira\nimpl Point {\n    fn x(self) -> float {\n        self.x\n    }\n}\n```",
        "Self" => "**Self** - Reference to implementing type\n\n```lira\nimpl Point {\n    fn origin() -> Self {\n        Self { x: 0.0, y: 0.0 }\n    }\n}\n```",
        "true" => "**true** - Boolean true value",
        "false" => "**false** - Boolean false value",
        "null" => "**null** - Null value for optional types",
        _ => return None,
    };
    Some(doc.to_string())
}

fn type_docs(type_name: &str) -> Option<String> {
    let doc = match type_name {
        "int" => "**int** - 64-bit signed integer\n\nRange: -2^63 to 2^63-1",
        "int8" => "**int8** - 8-bit signed integer\n\nRange: -128 to 127",
        "int16" => "**int16** - 16-bit signed integer\n\nRange: -32768 to 32767",
        "int32" => "**int32** - 32-bit signed integer\n\nRange: -2^31 to 2^31-1",
        "int64" => "**int64** - 64-bit signed integer\n\nRange: -2^63 to 2^63-1",
        "uint8" => "**uint8** - 8-bit unsigned integer\n\nRange: 0 to 255",
        "uint16" => "**uint16** - 16-bit unsigned integer\n\nRange: 0 to 65535",
        "uint32" => "**uint32** - 32-bit unsigned integer\n\nRange: 0 to 2^32-1",
        "uint64" => "**uint64** - 64-bit unsigned integer\n\nRange: 0 to 2^64-1",
        "float" => "**float** - 64-bit IEEE 754 floating point",
        "bool" => "**bool** - Boolean type\n\nValues: `true` or `false`",
        "string" => "**string** - UTF-8 encoded string\n\n```lira\nlet s = \"Hello, world!\"\nlet multiline = \"\"\"\n    Multi-line\n    string\n\"\"\"\n```",
        "char" => "**char** - Single Unicode character\n\n```lira\nlet c = 'a'\n```",
        "void" => "**void** - Unit type, no value",
        "List" => "**List<T>** - Dynamic array\n\n```lira\nlet nums: List<int> = [1, 2, 3]\npush(nums, 4)\n```",
        "Map" => "**Map<K, V>** - Hash map\n\n```lira\nlet scores: Map<string, int> = {}\nscores[\"alice\"] = 100\n```",
        "Set" => "**Set<T>** - Hash set\n\n```lira\nlet seen: Set<int> = {}\n```",
        "Option" => "**Option<T>** - Optional value\n\n```lira\nenum Option<T> {\n    Some(T),\n    None,\n}\n```\n\nUse `??` for default values: `value ?? default`",
        "Result" => "**Result<T, E>** - Success or error\n\n```lira\nenum Result<T, E> {\n    Ok(T),\n    Err(E),\n}\n```\n\nUse `?` for error propagation.",
        "Channel" => "**Channel<T>** - Communication channel for fibers\n\n```lira\nlet (tx, rx) = Channel::new()\nspawn { tx.send(42) }\nlet value = rx.recv()\n```",
        _ => return None,
    };
    Some(doc.to_string())
}

fn builtin_docs(func_name: &str) -> Option<String> {
    let doc = match func_name {
        "print" => "**print**(value: any) -> void\n\nPrint a value to stdout without newline.",
        "println" => "**println**(value: any) -> void\n\nPrint a value to stdout with newline.",
        "debug" => "**debug**(value: any) -> void\n\nPrint debug representation of a value.",
        "assert" => "**assert**(condition: bool) -> void\n\nPanic if condition is false.",
        "len" => "**len**(collection: [T]) -> int\n\nReturn the length of a collection.",
        "push" => "**push**(array: [T], value: T) -> void\n\nAppend a value to an array.",
        "pop" => "**pop**(array: [T]) -> T?\n\nRemove and return the last element.",
        "append" => "**append**(a: [T], b: [T]) -> [T]\n\nConcatenate two arrays.",
        "panic" => {
            "**panic**(message: string) -> never\n\nTerminate execution with an error message."
        }
        "todo" => {
            "**todo**(message: string) -> never\n\nMark unimplemented code. Panics at runtime."
        }
        "unreachable" => "**unreachable**() -> never\n\nMark code that should never execute.",
        _ => return None,
    };
    Some(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test convenience: analyze on demand, mirroring the server's cache-aware
    /// call but without the document store.
    fn get_hover(content: &str, position: Position) -> Option<Hover> {
        let analysis = lirac::analyze(content).ok();
        get_hover_with(content, position, analysis.as_ref())
    }

    #[test]
    fn test_get_word_at_position() {
        let word = utils::get_word_at_position("let x = 42", 0).unwrap();
        assert_eq!(word.text, "let");
        let word = utils::get_word_at_position("let x = 42", 4).unwrap();
        assert_eq!(word.text, "x");
        assert!(utils::get_word_at_position("let x = 42", 6).is_none()); // on '='
    }

    #[test]
    fn test_keyword_docs() {
        assert!(keyword_docs("fn").is_some());
        assert!(keyword_docs("let").is_some());
        assert!(keyword_docs("unknown").is_none());
    }

    #[test]
    fn test_type_docs() {
        assert!(type_docs("int").is_some());
        assert!(type_docs("string").is_some());
        assert!(type_docs("unknown").is_none());
    }

    #[test]
    fn test_hover_unicode() {
        // Test hover works with unicode
        let content = "let 变量 = 5";
        let hover = get_hover(
            content,
            Position {
                line: 0,
                character: 0,
            },
        );
        assert!(hover.is_some());
    }

    fn hover_text(content: &str, line: u32, character: u32) -> String {
        let hover = get_hover(content, Position { line, character }).expect("hover present");
        match hover.contents {
            HoverContents::Markup(m) => m.value,
            other => panic!("expected markup hover, got {:?}", other),
        }
    }

    #[test]
    fn test_sema_hover_let_int() {
        // `let x = 42`; hover on `x` shows int.
        let content = "let x = 42";
        let text = hover_text(content, 0, 4);
        assert!(text.contains("int"), "expected int in hover, got: {}", text);
        assert!(text.contains('x'));
    }

    #[test]
    fn test_sema_hover_string() {
        let content = "let s = \"hi\"";
        let text = hover_text(content, 0, 4);
        assert!(
            text.contains("string"),
            "expected string in hover, got: {}",
            text
        );
    }

    #[test]
    fn test_sema_hover_field_access() {
        // hover on `x` in `p.x` -> int via field_resolution; hover on `p` -> P.
        let content = "struct P { x: int }\nlet p = P { x: 1 }\nlet z = p.x";
        // `p` on line 3, column index 8 (0-indexed).
        let on_p = hover_text(content, 2, 8);
        assert!(on_p.contains('P'), "expected P, got: {}", on_p);
        // `x` on line 3, column index 10.
        let on_x = hover_text(content, 2, 10);
        assert!(on_x.contains("int"), "expected int, got: {}", on_x);
    }

    #[test]
    fn test_sema_hover_error_tolerant() {
        // Error on line 2; hover on `a` (line 3) still resolves to int. This is
        // the core foundation: `analyze` returns sema even on error.
        let content = "let a = 1\nlet b = missing\nlet c = a";
        let text = hover_text(content, 2, 8); // `a` in `let c = a`
        assert!(
            text.contains("int"),
            "error-tolerant hover should still show int, got: {}",
            text
        );
    }
}
