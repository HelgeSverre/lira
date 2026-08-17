//! Explicit-cast regressions for pointer-backed storage types.

use lirac::{analyze, check};

#[test]
fn pointer_backed_storage_cannot_be_reinterpreted_by_as() {
    let source = r#"let ints: [int] = [1]
let same_ints = ints as [int]
let strings = ints as [string]
let int_map: Map<string, int> = { "answer": 1 }
let string_map = int_map as Map<string, string>
let int_channel: Channel<int> = chan(1)
let string_channel = int_channel as Channel<string>
let tuple: (int, string) = (1, "one")
let swapped = tuple as (string, int)
struct First { value: int }
struct Second { value: string }
let first = First { value: 1 }
let second = first as Second
let maybe_int: int? = 1
let maybe_string = maybe_int as string?
fn identity(value: int) -> int { return value }
let wrong_function = identity as fn(string) -> string
"#;

    let diagnostics = analyze(source).expect("source parses").diagnostics;
    let expected = [
        (3, "Cannot cast '[int]' to '[string]'"),
        (5, "Cannot cast 'Map<string, int>' to 'Map<string, string>'"),
        (7, "Cannot cast 'Channel<int>' to 'Channel<string>'"),
        (9, "Cannot cast '(int, string)' to '(string, int)'"),
        (13, "Cannot cast 'First' to 'Second'"),
        (15, "Cannot cast 'int?' to 'string?'"),
        (17, "Cannot cast 'fn(int) -> int' to 'fn(string) -> string'"),
    ];

    for (line, message) in expected {
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.line == line && diagnostic.message == message),
            "missing line {line} diagnostic {message:?}: {diagnostics:?}"
        );
    }
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.line != 2),
        "the exact array cast must remain valid: {diagnostics:?}"
    );
}

#[test]
fn scalar_dynamic_and_nominal_upcasts_remain_valid() {
    let source = r#"
class Animal {}
class Dog extends Animal {}

let integer = 4
let widened = integer as float
let parsed = "42" as int
let rendered = integer as string
let exact_array = [1] as [int]
let dog = Dog {}
let animal = dog as Animal
let maybe_animal = dog as Animal?
fn dynamic_cast(value) -> int { return value as int }
let boxed = integer as any
"#;

    check(source).expect("defined scalar, dynamic, exact, and class casts must check");
}
