//! Real-source parity tests for native tuple value semantics.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

mod common;

static AOT_LOCK: Mutex<()> = Mutex::new(());

const SOURCE: &str = r#"
struct Point { value: int }
class Box { value: int }

fn transform(value: (Point, (Point, int), [int], Box)) -> (Point, (Point, int), [int], Box) {
    value[0].value = 30
    value[1][0].value = 40
    value[2][0] = 50
    value[3].value = 60
    return value
}

let point = Point { value: 1 }
let values = [7]
let object = Box { value: 9 }
let source = (point, (Point { value: 2 }, 3), values, object)

// Assignment copies the tuple and its value-semantic struct elements, while
// arrays and classes inside it retain their reference identity.
let assigned = source
assigned[0].value = 10
assigned[1][0].value = 20
assigned[2][0] = 70
assigned[3].value = 90
println(source[0].value)
println(source[1][0].value)
println(source[2][0])
println(source[3].value)
println(assigned[0].value)
println(assigned[1][0].value)

// The argument and return boundaries perform the same recursive tuple copy.
let returned = transform(source)
println(source[0].value)
println(source[1][0].value)
println(source[2][0])
println(source[3].value)
println(returned[0].value)
println(returned[1][0].value)

// A returned tuple remains independently mutable for its nested value structs.
returned[0].value = 300
returned[1][0].value = 400
println(source[0].value)
println(source[1][0].value)
println(returned[0].value)
println(returned[1][0].value)
println(point.value)
"#;

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-tuple-value-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn run_vm(source: &str) -> Result<Vec<String>, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let source_path = dir.join("program.li");
    let result = match common::run_vm_capture(&source_path, source) {
        Err(error) => Err(format!("VM containment failure: {error}")),
        Ok(common::VmRunOutcome::Success { status: 0, output }) => {
            Ok(String::from_utf8_lossy(&output)
                .trim_end()
                .lines()
                .map(str::to_owned)
                .collect())
        }
        Ok(common::VmRunOutcome::Success { status, .. }) => {
            Err(format!("VM exited with status {status}"))
        }
        Ok(common::VmRunOutcome::CompileError(error)) => Err(format!("VM compile error: {error}")),
        Ok(common::VmRunOutcome::RuntimeError { message, output }) => Err(format!(
            "VM runtime error: {message}; partial output: {}",
            String::from_utf8_lossy(&output).trim_end()
        )),
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_aot(source: &str) -> Result<(i32, Vec<String>, String), String> {
    let _guard = AOT_LOCK
        .lock()
        .map_err(|error| format!("AOT lock poisoned: {error}"))?;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let output = common::run_aot(&path, source).map_err(|error| error.to_string())?;
        output.assert_complete_output()?;
        let status = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Ok((status, stdout, stderr))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_jit(source: &str) -> Result<(i32, Vec<String>), String> {
    let (status, output) = common::run_jit_capture("native_tuple_value_semantics.li", source)?;
    Ok((
        status,
        String::from_utf8_lossy(&output)
            .trim_end()
            .lines()
            .map(str::to_owned)
            .collect(),
    ))
}

#[test]
fn tuples_copy_nested_structs_but_preserve_reference_slots_across_native_backends() {
    let expected = vec![
        "1", "2", "70", "90", "10", "20", "1", "2", "50", "60", "30", "40", "1", "2", "300", "400",
        "1",
    ];
    assert_eq!(run_vm(SOURCE).expect("VM run"), expected);

    let (aot_status, aot_output, aot_stderr) = run_aot(SOURCE).expect("AOT run");
    assert_eq!(aot_status, 0, "AOT stderr: {aot_stderr}");
    assert!(aot_stderr.is_empty(), "unexpected AOT stderr: {aot_stderr}");
    assert_eq!(aot_output, expected);

    let (jit_status, jit_output) = run_jit(SOURCE).expect("JIT run");
    assert_eq!(jit_status, 0);
    assert_eq!(jit_output, expected);
}

#[test]
fn tuple_index_mutation_remains_a_checker_error() {
    let source = "let value = (1, 2)\nvalue[0] = 3\nvalue[1] += 4\nprintln(value[0])";
    let error = lirac::check(source).expect_err("tuple indexes are immutable");
    assert!(error.contains("Cannot assign to tuple index; tuples are immutable"));
}
