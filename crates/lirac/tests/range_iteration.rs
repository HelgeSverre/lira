//! Real-source bytecode regressions for built-in range iteration.

fn run(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile(source)?;
    let (status, output) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("bytecode VM exited with status {status}"));
    }
    Ok(output)
}

#[test]
fn bounded_ranges_iterate_with_stable_bounds_and_loop_control() {
    let source = r#"
var exclusive = 0
for value in 1..4 { exclusive += value }
println(exclusive)

var inclusive = 0
for value in 1..=3 { inclusive += value }
println(inclusive)

var descending = 0
for value in 4..1 { descending += value }
println(descending)

var empty = 0
for value in 3..3 { empty += 1 }
println(empty)

let stored = 2..=4
println(stored.start)
println(stored.end)
println(stored.inclusive)
var stored_total = 0
for value in stored { stored_total += value }
println(stored_total)

var controlled = 0
for value in 0..=6 {
    if value == 1 { continue }
    if value == 5 { break }
    controlled += value
}
println(controlled)
"#;

    assert_eq!(
        run(source).expect("bounded ranges should compile and execute"),
        ["6", "6", "0", "0", "2", "4", "true", "9", "9"]
    );
}

#[test]
fn user_range_and_builtin_range_have_distinct_semantics() {
    let source = r#"
struct Range {
    value: int
    fn doubled(self) -> int { return self.value * 2 }
}

let user = Range { value: 7 }
println(user.value)
println(user.doubled())

let builtin = 1..=2
var total = 0
for value in builtin { total += value }
println(total)
"#;

    assert_eq!(
        run(source).expect("user and built-in ranges should coexist"),
        ["7", "14", "3"]
    );
}

#[test]
fn concrete_noniterables_and_open_ranges_are_rejected() {
    let user_struct = r#"
struct Range { value: int }
let user = Range { value: 7 }
for value in user { println(value) }
"#;
    let error = lirac::compile(user_struct).expect_err("a user Range is not iterable");
    assert!(
        error.contains(
            "Cannot iterate value of type 'Range'; expected an array, string, tuple, or range"
        ),
        "unexpected user-Range diagnostic: {error}"
    );

    let stored_open = r#"
let open = (1..)
for value in open { println(value) }
"#;
    let diagnostics = lirac::analyze(stored_open)
        .expect("open-ended range source should parse")
        .diagnostics;
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Open-ended range expressions")),
        "checker did not diagnose the parsed open range: {diagnostics:?}"
    );
    let error = lirac::compile(stored_open).expect_err("an open range is not executable");
    assert!(
        error.contains("Open-ended range expressions are not supported"),
        "unexpected open-range diagnostic: {error}"
    );
}
