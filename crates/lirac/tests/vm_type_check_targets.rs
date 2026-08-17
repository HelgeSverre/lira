//! Real-source regressions for resolved `is` targets.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    let (exit_code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|error| panic!("run {name}: {error}"));
    assert_eq!(exit_code, 0, "{name} exited with status {exit_code}");
    output
}

#[test]
fn is_uses_resolved_aliases_and_runtime_outer_tags() {
    let output = run_source(
        "resolved_is_targets",
        r#"
type Integer = int
struct Point { x: int }

fn identity(value: int) -> int {
    return value
}

let integer: Integer = 42
let narrow: int8 = 7
let unsigned: uint16 = 9
let letter: char = 'a'
let array = [1, 2]
let tuple = (1, "one")
let point = Point { x: 1 }
let channel = chan(1)

println(integer is Integer)
println(narrow is int8)
println(unsigned is uint16)
println(letter is char)
println(array is [int])
println(tuple is (int, string))
println(identity is fn(int) -> int)
println(channel is Channel<int>)
println(point is Point)

println(integer is string)
println(array is (int, string))
println(point is [int])
println(channel is fn(int) -> int)
"#,
    );

    assert_eq!(
        output,
        [
            "true", "true", "true", "true", "true", "true", "true", "true", "true", "false",
            "false", "false", "false",
        ]
    );
}

#[test]
fn is_any_evaluates_its_operand_once_and_returns_true() {
    let output = run_source(
        "is_any_once",
        r#"
fn produce() -> int {
    println("called")
    return 7
}

println(produce() is any)
"#,
    );

    assert_eq!(output, ["called", "true"]);
}

#[test]
fn unknown_is_target_is_rejected_by_the_checker() {
    let error = lirac::check(
        r#"
let value = 1
let result = value is MissingType
"#,
    )
    .expect_err("unknown type-check targets must remain checker errors");

    assert!(
        error.contains("Unknown type"),
        "unexpected diagnostic: {error}"
    );
}
