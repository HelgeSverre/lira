//! Source-level parity tests for native floating-point operators.
//!
//! The same source is compiled for the bytecode VM and for a linked native
//! executable. JIT execution is also checked for every successful program;
//! its public API exposes status rather than captured output.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-float-{}-{}",
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

fn run_aot_status(source: &str) -> Result<(bool, String), String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let output = common::run_aot(&path, source).map_err(|error| format!("run AOT: {error}"))?;
        Ok((
            output.status.success(),
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
fn float_remainder_and_power_match_vm_aot_and_jit() {
    let source = r#"
        var remainder = 7.5 % 2.0
        remainder %= -2.0
        println(remainder)
        println(2.0 ** 3.0)
        println(2 ** 3.0)
        println(2.0 ** 3)
    "#;
    let expected = "1.5\n8\n8\n8";
    assert_eq!(run_vm(source).expect("VM run"), expected);
    assert_eq!(run_aot(source).expect("AOT run"), expected);
    assert_eq!(run_jit(source).expect("JIT run"), 0);
}

#[test]
fn float_special_values_and_any_zero_division_keep_ieee_semantics() {
    let source = r#"
        let nan = 0.0 / 0.0
        let inf = 1.0 / 0.0
        let any_div: any = 1.0
        let any_rem: any = 1.0
        let any_inf = any_div / 0.0
        let any_nan = any_rem % -0.0
        println(is_nan(nan))
        println(is_infinite(inf))
        println(nan == nan)
        println(nan != nan)
        println(any_inf)
        println(any_nan)
    "#;
    let expected = "true\ntrue\nfalse\ntrue\ninf\nNaN";
    assert_eq!(run_vm(source).expect("VM run"), expected);
    assert_eq!(run_aot(source).expect("AOT run"), expected);
    assert_eq!(run_jit(source).expect("JIT run"), 0);
}

#[test]
fn negative_integer_power_is_a_runtime_failure_on_all_backends() {
    let source = "println(2 ** -1)";
    let vm_error = run_vm(source).expect_err("VM must reject a negative integer exponent");
    assert!(
        vm_error.to_ascii_lowercase().contains("negative exponent"),
        "unexpected VM diagnostic: {vm_error}"
    );
    let (aot_ok, aot_error) = run_aot_status(source).expect("AOT execution");
    assert!(!aot_ok, "negative integer exponent must fail at runtime");
    assert!(
        aot_error.to_ascii_lowercase().contains("negative exponent"),
        "unexpected AOT diagnostic: {aot_error}"
    );
    assert_eq!(
        run_jit(source).expect("JIT runtime failure returns a status"),
        1
    );
}
