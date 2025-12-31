//! Hover support for Lira
//!
//! Provides type information and documentation on hover.

use tower_lsp::lsp_types::*;

/// Get hover information at a position
pub fn get_hover(content: &str, position: Position) -> Option<Hover> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = position.character as usize;

    // Get the word at the cursor position
    let word = get_word_at_position(line, col)?;

    // Look up documentation for the word
    let (markdown, range) = get_word_documentation(word, line, col, position.line)?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(range),
    })
}

fn get_word_at_position(line: &str, col: usize) -> Option<&str> {
    if col > line.len() {
        return None;
    }

    // Find word boundaries
    let chars: Vec<char> = line.chars().collect();

    if col >= chars.len() {
        return None;
    }

    // Check if we're on a word character
    if !chars[col].is_alphanumeric() && chars[col] != '_' {
        return None;
    }

    // Find start of word
    let mut start = col;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }

    // Find end of word
    let mut end = col;
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
        end += 1;
    }

    if start == end {
        return None;
    }

    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
    let byte_end: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();

    Some(&line[byte_start..byte_end])
}

fn get_word_documentation(
    word: &str,
    line: &str,
    col: usize,
    line_num: u32,
) -> Option<(String, Range)> {
    // Calculate the range for the word
    let chars: Vec<char> = line.chars().collect();
    let mut start = col;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
        end += 1;
    }

    let range = Range {
        start: Position {
            line: line_num,
            character: start as u32,
        },
        end: Position {
            line: line_num,
            character: end as u32,
        },
    };

    // Look up keyword documentation
    if let Some(doc) = keyword_docs(word) {
        return Some((doc, range));
    }

    // Look up type documentation
    if let Some(doc) = type_docs(word) {
        return Some((doc, range));
    }

    // Look up built-in function documentation
    if let Some(doc) = builtin_docs(word) {
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

    #[test]
    fn test_get_word_at_position() {
        assert_eq!(get_word_at_position("let x = 42", 0), Some("let"));
        assert_eq!(get_word_at_position("let x = 42", 4), Some("x"));
        assert_eq!(get_word_at_position("let x = 42", 6), None); // on '='
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
}
