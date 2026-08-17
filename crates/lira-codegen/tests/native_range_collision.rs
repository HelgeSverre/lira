//! Native range layout isolation coverage.
//!
//! The source-level name `Range` is legal for user structs. These tests drive
//! one program through the bytecode VM, AOT backend, and JIT so the compiler's
//! private range layout cannot be confused with that user declaration.

use std::path::Path;

mod common;

const RANGE_SOURCE: &str = r#"
struct Range {
    value: int

    fn doubled(self) -> int { return self.value * 2 }
}

fn erase(value) { return value }

let user = Range { value: 7 }
println(user.value)
println(user.doubled())

var range = 1..=3
println(range.start)
println(range.end)
println(range.inclusive)

var erased = erase(range)
println(erased.start)

var total = 0
for value in range { total = total + value }
println(total)
"#;

const EXPECTED: &[&str] = &["7", "14", "1", "3", "true", "1", "6"];

fn assert_expected(lines: &[String]) {
    assert_eq!(
        lines.iter().map(String::as_str).collect::<Vec<_>>(),
        EXPECTED
    );
}

fn run_aot(source: &str) -> Result<Vec<String>, String> {
    let output = common::run_aot(Path::new("native_range_collision.li"), source)?;
    output.assert_complete_output()?;
    if !output.status.success() {
        return Err(format!(
            "native executable failed with {}: {}",
            output.status,
            output.stderr_text()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

fn run_vm(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile(source)?;
    let (status, output) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("bytecode VM exited with status {status}"));
    }
    Ok(output)
}

#[test]
fn user_range_and_builtin_range_coexist_across_vm_aot_and_jit() {
    let vm = run_vm(RANGE_SOURCE).expect("bytecode range collision program should run");
    assert_expected(&vm);

    let aot = run_aot(RANGE_SOURCE).expect("AOT range collision program should run");
    assert_expected(&aot);

    let (status, stdout) = common::run_jit_capture("native_range_collision.li", RANGE_SOURCE)
        .expect("JIT range collision program should run");
    assert_eq!(status, 0);
    let jit_output = String::from_utf8_lossy(&stdout);
    let jit: Vec<&str> = jit_output.lines().collect();
    assert_eq!(jit, EXPECTED);
}

#[test]
fn user_range_is_not_native_iterable() {
    let source = r#"
struct Range { value: int }
let user = Range { value: 7 }
var total = 0
for value in user { total = total + value }
println(total)
"#;
    let error = run_aot(source).expect_err("a user Range must not be treated as iterable");
    assert!(
        error.contains(
            "Cannot iterate value of type 'Range'; expected an array, string, tuple, or range"
        ),
        "checker rejection should identify the user Range type: {error}"
    );
}
