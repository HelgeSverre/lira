//! Real-source regex parity across the bytecode VM, AOT native backend, and
//! Cranelift JIT. Resource-limit edge cases that need multi-megabyte inputs
//! are covered by the runtime unit tests; this test keeps the end-to-end case
//! small enough for the ordinary integration-test budget.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

const SOURCE: &str = r#"
let unicode_ok = regex_match("^\\p{L}+$", "café")
let find_ok = regex_find("[0-9]+", "abc123") == "123"
let all = regex_find_all("^|$", "é")
let split = regex_split("^|$", "é")
let replaced = regex_replace_all("(?P<word>[a-z]+)-([0-9]+)", "abc-42", "\${word}:\$2")
let valid_ok = regex_is_valid("[a-z]+") && !regex_is_valid("[")

if unicode_ok && find_ok && len(all) == 2 && len(split) == 3 &&
    replaced == "abc:42" && valid_ok {
    println("regex parity ok")
} else {
    println(1 / 0)
}
"#;

fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lira-regex-parity-{}-{id}", std::process::id()))
}

fn run_vm(source: &str, dir: &Path) -> Result<String, String> {
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).map_err(|error| error.to_string())?;
    let bytecode = lirac::compile_with_imports(source_path.to_str().unwrap_or_default(), source)?;
    let (status, output) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM exited with status {status}"));
    }
    Ok(output.join("\n"))
}

fn run_aot(source: &str, dir: &Path) -> Result<String, String> {
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).map_err(|error| error.to_string())?;
    let output = common::run_aot(&source_path, source)?;
    if !output.status.success() {
        return Err(format!("AOT exited with status {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn regex_real_source_has_vm_aot_jit_parity() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let vm = run_vm(SOURCE, &dir).expect("VM regex source should run");
    let aot = run_aot(SOURCE, &dir).expect("AOT regex source should run");
    let source_path = dir.join("jit.li");
    std::fs::write(&source_path, SOURCE).expect("write JIT source");
    let jit = common::run_jit(source_path.to_str().unwrap_or_default(), SOURCE)
        .expect("JIT regex source should run");
    assert_eq!(jit, 0);
    assert_eq!(vm.trim(), "regex parity ok");
    assert_eq!(aot.trim(), vm.trim());
    let _ = std::fs::remove_dir_all(dir);
}
