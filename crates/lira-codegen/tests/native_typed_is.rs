//! Descriptor-precise native `is` checks.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-typed-is-{}-{}",
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
        output.assert_complete_output()?;
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

fn run_jit(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = common::run_jit_capture(path.to_str().ok_or("non-utf8 path")?, source).and_then(
        |(status, output)| {
            if status != 0 {
                return Err(format!("JIT status {status}"));
            }
            String::from_utf8(output).map_err(|error| error.to_string())
        },
    );
    let _ = std::fs::remove_dir_all(&dir);
    result.map(|output| output.trim_end().to_string())
}

#[test]
fn scalar_and_nested_descriptors_have_vm_aot_jit_parity() {
    let source = r#"
        fn probe() -> any {
            println("probed")
            return [1, 2]
        }

        let array: any = probe()
        let tuple: any = (1, 2)
        println(array is Array<int>)
        println(array is (int, int))
        println(tuple is (int, int))
        println(tuple is Array<int>)
        println(1 is int)
        println("text" is string)
    "#;
    let expected = "probed\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue";
    assert_eq!(run_vm(source).expect("VM run"), expected);
    assert_eq!(run_aot(source).expect("AOT run"), expected);
    assert_eq!(run_jit(source).expect("JIT run"), expected);
}

#[test]
fn native_descriptors_distinguish_nominals_and_nested_payloads() {
    let source = r#"
        class Animal {}
        class Dog extends Animal {}
        class Cat {}

        let erased_dog: any = Dog {}
        let erased_array: any = [1]
        let declared_animal: Animal = Dog {}

        println(erased_dog is Animal)
        println(erased_dog is Dog)
        println(erased_dog is Cat)
        println(declared_animal is Animal)
        println(declared_animal is Dog)
        println(erased_array is Array<string>)
    "#;

    // The bytecode VM only carries the coarse object/array TypeIs tag, so it
    // cannot express these negative descriptor cases. Native AOT and JIT
    // retain the exact aggregate/nominal descriptors and must agree here.
    let native_expected = "true\ntrue\nfalse\ntrue\ntrue\nfalse";
    assert_eq!(run_aot(source).expect("AOT run"), native_expected);
    assert_eq!(run_jit(source).expect("JIT run"), native_expected);

    let vm_output = run_vm(source).expect("VM run");
    assert_eq!(vm_output, "true\ntrue\ntrue\ntrue\ntrue\ntrue");
}

#[test]
fn native_descriptors_cover_functions_channels_maps_results_and_structs() {
    let source = r#"
        struct Left { value: int }
        struct Right { value: int }

        fn increment(value: int) -> int { return value + 1 }
        let erased_function: any = increment
        let typed_channel: Channel<int> = chan(1)
        let erased_channel: any = typed_channel
        let erased_map: any = { "answer": 1 }
        let typed_result: Result<int, string> = Result::Ok(1)
        let erased_result: any = typed_result
        let erased_left: any = Left { value: 1 }

        println(erased_function is fn(int) -> int)
        println(erased_function is fn(string) -> string)
        println(erased_channel is Channel<int>)
        println(erased_channel is Channel<string>)
        println(erased_map is Map<string, int>)
        println(erased_map is Map<int, int>)
        println(erased_result is Result<int, string>)
        println(erased_result is Result<string, string>)
        println(erased_left is Left)
        println(erased_left is Right)
    "#;
    let native_expected = "true\nfalse\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse";
    assert_eq!(run_aot(source).expect("AOT run"), native_expected);
    assert_eq!(run_jit(source).expect("JIT run"), native_expected);

    // VM TypeIs intentionally has no payload/nominal descriptor metadata yet.
    let vm_output = run_vm(source).expect("VM run");
    assert_eq!(
        vm_output,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue"
    );
}

#[test]
fn undeclared_type_check_targets_are_rejected_by_checker() {
    for target in ["MissingType", "array", "object", "function"] {
        let source = format!("println(1 is {target})");
        let error = lirac::check(&source).expect_err("unknown target");
        assert!(
            error.contains(&format!("Unknown type: {target}")),
            "unexpected error for `{target}`: {error}"
        );
    }
}
