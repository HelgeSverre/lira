//! Real-source coverage for tuple and erased-Any `for` iteration.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-for-tuple-any-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn run_vm(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let bytecode = lirac::compile_with_imports(path.to_str().ok_or("non-utf8 path")?, source)?;
        let (status, lines) = liravm::run_with_capture(&bytecode)?;
        if status != 0 {
            return Err(format!("VM status {status}"));
        }
        Ok(lines.join("\n"))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_aot(source: &str) -> Result<(bool, String, String), String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let output = common::run_aot(&path, source).map_err(|error| error.to_string())?;
        output.assert_complete_output()?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_jit(source: &str) -> Result<(i32, String), String> {
    let (status, output) = common::run_jit_capture("native_for_tuple_any.li", source)?;
    Ok((
        status,
        String::from_utf8_lossy(&output).trim_end().to_string(),
    ))
}

#[test]
fn tuples_and_erased_iterables_match_vm_aot_jit() {
    let source = r#"
        fn identity(value: any) -> any { return value }
        struct Cell { value: int }

        var total = 0
        for value in (1, 2, 3) { total += value }
        println(total)

        let cells = (Cell { value: 1 }, Cell { value: 2 })
        for cell in cells {
            cell.value += 10
            println(cell.value)
        }
        println(cells[0].value)

        for value in (1, "é") { println(value) }
        let json_values = json_parse("[1,\"é\"]")
        for value in json_values { println(value) }

        let erased_tuple = identity((1, "é"))
        for value in erased_tuple { println(value) }

        let erased_cells = identity(cells)
        for cell in erased_cells {
            cell.value += 20
            println(cell.value)
        }
        println(cells[0].value)

        let erased_string = identity("é🙂")
        for codepoint in erased_string { println(codepoint) }

        for value in (1, 2, 3) {
            if value == 2 { continue }
            println(value)
            if value == 3 { break }
        }
    "#;
    let expected = "6\n11\n12\n1\n1\né\n1\né\n1\né\n21\n22\n1\n233\n128578\n1\n3";
    assert_eq!(run_vm(source).expect("VM run"), expected);
    let (aot_status, aot_output, aot_error) = run_aot(source).expect("AOT run");
    assert!(aot_status, "AOT failed: {aot_error}");
    assert_eq!(aot_output, expected);
    let (jit_status, jit_output) = run_jit(source).expect("JIT run");
    assert_eq!(jit_status, 0);
    assert_eq!(jit_output, expected);
}

#[test]
fn concrete_non_iterables_are_rejected_by_the_checker() {
    for source in [
        "struct Point { x: int } let p = Point { x: 1 } for x in p { println(x) }",
        "let values = {\"x\": 1} for x in values { println(x) }",
    ] {
        let error = lirac::check(source).expect_err("non-iterable should be rejected");
        assert!(
            error.contains("expected an array, string, tuple, or range"),
            "unexpected checker diagnostic: {error}"
        );
    }
}

#[test]
fn erased_scalar_iteration_fails_deterministically() {
    let source = r#"
        fn identity(value: any) -> any { return value }
        let scalar = identity(1)
        for value in scalar { println(value) }
    "#;
    let vm_error = run_vm(source).expect_err("VM should reject an erased scalar iterable");
    assert!(
        vm_error.contains("Cannot get length of non-sized value"),
        "{vm_error}"
    );
    let (aot_status, _, aot_error) = run_aot(source).expect("AOT run");
    assert!(!aot_status);
    assert!(
        aot_error.contains("cannot iterate dynamic value"),
        "{aot_error}"
    );
    let (jit_status, _) = run_jit(source).expect("JIT run");
    assert_ne!(jit_status, 0);
}
