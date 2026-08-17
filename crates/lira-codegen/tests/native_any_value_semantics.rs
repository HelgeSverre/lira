//! Real-source parity tests for dynamic Any value-copy boundaries.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

mod common;

static AOT_LOCK: Mutex<()> = Mutex::new(());

const SOURCE: &str = r#"
struct Point { x: int }
struct Holder { payload: any }
class Box { x: int }

fn pass(value: any) -> any { return value }

// Passing an erased nested tuple must copy both Point values.
let source = (Point { x: 1 }, (Point { x: 2 }, 3))
let erased: any = source
let passed: any = pass(erased)
let extracted: (Point, (Point, int)) = passed
extracted[0].x = 10
extracted[1][0].x = 20
println(source[0].x)
println(source[1][0].x)
println(extracted[0].x)
println(extracted[1][0].x)

// Any-valued struct fields are value-semantic at a struct copy boundary.
let holder = Holder { payload: Point { x: 7 } }
let holder_copy = holder
holder_copy.payload.x = 9
println(holder.payload.x)
println(holder_copy.payload.x)

// Rvalue extraction from erased arrays and maps gets an independent Point.
let points = [Point { x: 20 }]
let points_any: any = points
let array_item: any = points_any[0]
array_item.x = 21
println(points[0].x)
println(array_item.x)

let point_map = { "p": Point { x: 30 } }
let map_any: any = point_map
let map_item: any = map_any["p"]
map_item.x = 31
println(point_map["p"].x)
println(map_item.x)

// A copied tuple clones Point but preserves array, map, and class references.
let shared_array = [40]
let shared_map = { "v": Box { x: 50 } }
let shared_box = Box { x: 60 }
let aggregate: any = (Point { x: 70 }, shared_array, shared_map, shared_box)
let aggregate_copy: any = aggregate
let aggregate_array: [int] = aggregate_copy[1]
let aggregate_map: Map<string, Box> = aggregate_copy[2]
let aggregate_box: Box = aggregate_copy[3]
aggregate_copy[0].x = 71
aggregate_array[0] = 41
aggregate_map["v"].x = 51
aggregate_box.x = 61
println(aggregate[0].x)
println(aggregate_copy[0].x)
println(shared_array[0])
println(shared_map["v"].x)
println(shared_box.x)

// A tuple reached through Any is immutable even though its backing storage is
// an array; this must fail at the runtime boundary when the checker permits it.
"#;

const TUPLE_SET_SOURCE: &str = r#"
let value: any = (1, 2)
value[0] = 3
println(1)
"#;

const TUPLE_PUSH_SOURCE: &str = r#"
let value: any = (1, 2)
push(value, 3)
println(1)
"#;

const TUPLE_POP_SOURCE: &str = r#"
let value: any = (1, 2)
pop(value)
println(1)
"#;

fn scratch_dir(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-any-value-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn output_lines(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .trim_end()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn run_vm(source: &str) -> Result<Vec<String>, String> {
    let dir = scratch_dir("vm");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    let result = match common::run_vm_capture(&path, source)? {
        common::VmRunOutcome::Success { status: 0, output } => Ok(output_lines(&output)),
        common::VmRunOutcome::Success { status, .. } => Err(format!("VM exited with {status}")),
        common::VmRunOutcome::CompileError(error) => Err(format!("VM compile error: {error}")),
        common::VmRunOutcome::RuntimeError { message, output } => Err(format!(
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
    let dir = scratch_dir("aot");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let output = common::run_aot(&path, source).map_err(|error| error.to_string())?;
        output.assert_complete_output()?;
        Ok((
            output.status.code().unwrap_or(-1),
            output_lines(&output.stdout),
            output.stderr_text(),
        ))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_jit(source: &str) -> Result<(i32, Vec<String>), String> {
    let (status, output) = common::run_jit_capture("native_any_value_semantics.li", source)?;
    Ok((status, output_lines(&output)))
}

#[test]
fn dynamic_any_copies_value_semantics_and_preserves_references() {
    let expected = vec![
        "1", "2", "10", "20", "7", "9", "20", "21", "30", "31", "70", "71", "41", "51", "61",
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
fn dynamic_any_tuple_index_assignment_is_bounded_and_rejected() {
    let vm = common::run_vm_capture(Path::new("native_any_tuple_set.li"), TUPLE_SET_SOURCE)
        .expect("bounded VM containment");
    assert!(
        matches!(vm, common::VmRunOutcome::RuntimeError { .. }),
        "VM must compile the dynamic assignment and reject it at runtime: {vm:?}"
    );

    let (aot_status, _, aot_stderr) =
        run_aot(TUPLE_SET_SOURCE).expect("bounded AOT runtime rejection");
    assert_eq!(aot_status, 1, "AOT stderr: {aot_stderr}");
    assert!(
        aot_stderr.contains("tuple indexes are immutable"),
        "unexpected AOT runtime diagnostic: {aot_stderr}"
    );

    let (jit_status, jit_output) =
        run_jit(TUPLE_SET_SOURCE).expect("bounded JIT runtime rejection");
    assert_eq!(jit_status, 1);
    assert!(
        jit_output.is_empty(),
        "unexpected JIT output: {jit_output:?}"
    );
}

#[test]
fn dynamic_any_tuple_push_and_pop_are_bounded_and_rejected() {
    for (label, source) in [("push", TUPLE_PUSH_SOURCE), ("pop", TUPLE_POP_SOURCE)] {
        let vm = common::run_vm_capture(Path::new("native_any_tuple_mutation.li"), source)
            .expect("bounded VM containment");
        assert!(
            matches!(vm, common::VmRunOutcome::RuntimeError { .. }),
            "VM must reject dynamic tuple {label}: {vm:?}"
        );

        let (aot_status, _, aot_stderr) = run_aot(source).expect("bounded AOT runtime rejection");
        assert_eq!(aot_status, 1, "AOT {label} stderr: {aot_stderr}");
        assert!(
            aot_stderr.contains("tuples are immutable"),
            "unexpected AOT {label} diagnostic: {aot_stderr}"
        );

        let (jit_status, jit_output) = run_jit(source).expect("bounded JIT runtime rejection");
        assert_eq!(jit_status, 1, "JIT must reject tuple {label}");
        assert!(
            jit_output.is_empty(),
            "unexpected JIT {label} output: {jit_output:?}"
        );
    }
}
