//! Native/VM/JIT coverage for homogeneous arrays and explicit `[any]` arrays.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-any-array-{}-{}",
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

fn run_aot(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let output = common::run_aot(&path, source).map_err(|error| format!("run AOT: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "AOT status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_aot_status(source: &str) -> Result<(bool, String, String), String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let output = common::run_aot(&path, source).map_err(|error| format!("run AOT: {error}"))?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_jit(source: &str) -> Result<i32, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = common::run_jit(path.to_str().ok_or("non-utf8 path")?, source);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[test]
fn explicit_any_array_is_tagged_and_matches_vm_aot_jit() {
    let source = r#"
        let a: [any] = [1]
        push(a, "s")
        println(a)
        println(a[0])
        println(a[1])
        if a[0] != 1 || a[1] != "s" { println(1 / 0) }
    "#;
    let expected = "[1, s]\n1\ns";
    assert_eq!(run_vm(source).expect("VM run"), expected);
    assert_eq!(run_aot(source).expect("AOT run"), expected);
    assert_eq!(run_jit(source).expect("JIT run"), 0);
}

#[test]
fn erased_typed_array_widens_without_reinterpreting_slots() {
    let source = r#"
        fn id(x) { x }
        let a = id([1])
        push(a, "s")
        println(a)
    "#;
    let expected = "[1, s]";
    assert_eq!(run_vm(source).expect("VM run"), expected);
    assert_eq!(run_aot(source).expect("AOT run"), expected);
    assert_eq!(run_jit(source).expect("JIT run"), 0);
}

#[test]
fn homogeneous_array_storage_remains_typed() {
    let source = r#"
        let values: [int] = [1]
        push(values, 2)
        println(values[1])
    "#;
    assert_eq!(run_vm(source).expect("VM run"), "2");
    assert_eq!(run_aot(source).expect("AOT run"), "2");
    assert_eq!(run_jit(source).expect("JIT run"), 0);
}

#[test]
fn generic_array_channel_and_tuple_type_checks_match_all_backends() {
    let source = r#"
        let array: any = [1, 2]
        let tuple: any = (1, 2)
        let typed_channel: Channel<int> = chan(1)
        let channel: any = typed_channel

        println(array is Array<int>)
        println(array is Channel<int>)
        println(array is (int, int))
        println(tuple is Array<int>)
        println(tuple is (int, int))
        println(channel is Channel<int>)
        println(channel is Array<int>)

        if !(array is Array<int>) || array is Channel<int> || array is (int, int) {
            println(1 / 0)
        }
        if tuple is Array<int> || !(tuple is (int, int)) {
            println(1 / 0)
        }
        if !(channel is Channel<int>) || channel is Array<int> {
            println(1 / 0)
        }
    "#;
    let expected = "true\nfalse\nfalse\nfalse\ntrue\ntrue\nfalse";
    assert_eq!(run_vm(source).expect("VM run"), expected);
    assert_eq!(run_aot(source).expect("AOT run"), expected);
    assert_eq!(run_jit(source).expect("JIT run"), 0);
}

#[test]
fn concrete_array_alias_cannot_be_widened_by_push() {
    let source = r#"
        let xs = [1]
        let ys = xs
        push(xs, "s")
        println(ys[1])
    "#;
    let error = lirac::check(source).expect_err("heterogeneous push must be rejected");
    assert!(
        error.contains("Array element type mismatch: expected 'int', got 'string'"),
        "unexpected checker diagnostic: {error}"
    );
}

#[test]
fn wrong_any_array_descriptor_fails_before_slot_reinterpretation() {
    let source = r#"
        fn id(x) { x }
        fn fail(x) { return x["wrong"] }
        let a: [int] = id(["s"])
        println(a[0])
        fail(a)
    "#;
    // The VM has no erased-array descriptor metadata: it retains the tagged
    // string value. Native must still reject before interpreting that value as
    // an integer slot; the VM result documents this backend limitation.
    let vm_error = run_vm(source).expect_err("VM must reject the invalid dynamic array use");
    assert!(
        vm_error.contains("array") || vm_error.contains("index"),
        "unexpected VM diagnostic: {vm_error}"
    );
    let (aot_ok, aot_output, aot_error) = run_aot_status(source).expect("AOT execution");
    assert!(
        !aot_ok,
        "invalid Any array conversion must not execute successfully"
    );
    assert!(
        aot_output.is_empty(),
        "native must reject before interpreting or printing the first slot: {aot_output:?}"
    );
    assert!(
        aot_error.contains("array") || aot_error.contains("Any"),
        "unexpected AOT diagnostic: {aot_error}"
    );
    assert_eq!(
        run_jit(source).expect("JIT runtime failures return a status"),
        1,
        "JIT must report the invalid dynamic array use"
    );
}

#[test]
fn wrong_any_map_key_or_value_descriptor_fails_before_slot_reinterpretation() {
    for (name, source) in [
        (
            "value",
            r#"
                fn cast(value) -> Map<string, int> {
                    return value as Map<string, int>
                }
                let wrong = { "answer": "oops" }
                println(cast(wrong)["answer"])
            "#,
        ),
        (
            "key",
            r#"
                fn cast(value) -> Map<int, int> {
                    return value as Map<int, int>
                }
                let wrong = { "answer": 7 }
                println(cast(wrong))
            "#,
        ),
    ] {
        let (aot_ok, aot_output, aot_error) =
            run_aot_status(source).unwrap_or_else(|error| panic!("{name} AOT execution: {error}"));
        assert!(!aot_ok, "wrong Any map {name} descriptor must fail");
        assert!(
            aot_output.is_empty(),
            "wrong Any map {name} descriptor produced output before rejection: {aot_output:?}"
        );
        assert!(
            aot_error.contains("Any aggregate type does not match the requested type"),
            "unexpected Any map {name} diagnostic: {aot_error}"
        );
        assert_eq!(
            run_jit(source).unwrap_or_else(|error| panic!("{name} JIT execution: {error}")),
            1,
            "JIT must reject the wrong Any map {name} descriptor"
        );
    }
}

#[test]
fn wrong_any_function_or_channel_descriptor_fails_before_reinterpretation() {
    for (name, source) in [
        (
            "function",
            r#"
                fn as_string_function(value) -> fn(string) -> string {
                    return value as fn(string) -> string
                }
                fn increment(value: int) -> int { return value + 1 }
                let erased: any = increment
                let wrong = as_string_function(erased)
            "#,
        ),
        (
            "channel",
            r#"
                fn as_string_channel(value) -> Channel<string> {
                    return value as Channel<string>
                }
                let integers: Channel<int> = chan(1)
                let erased: any = integers
                let wrong = as_string_channel(erased)
            "#,
        ),
    ] {
        let (aot_ok, aot_output, aot_error) =
            run_aot_status(source).unwrap_or_else(|error| panic!("{name} AOT execution: {error}"));
        assert!(!aot_ok, "wrong Any {name} descriptor must fail");
        assert!(
            aot_output.is_empty(),
            "wrong Any {name} descriptor produced output before rejection: {aot_output:?}"
        );
        assert!(
            aot_error.contains("Any aggregate type does not match the requested type")
                || aot_error.contains("Any function descriptor")
                || aot_error.contains("Any channel descriptor"),
            "unexpected Any {name} descriptor diagnostic: {aot_error}"
        );
        assert_eq!(
            run_jit(source).unwrap_or_else(|error| panic!("{name} JIT execution: {error}")),
            1,
            "JIT must reject the wrong Any {name} descriptor"
        );
    }
}
