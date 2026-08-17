//! Native encoding regressions exercised through real Lira source.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

mod common;

static AOT_LOCK: Mutex<()> = Mutex::new(());

fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "lira-native-encoding-{}-{}",
        std::process::id(),
        id
    ))
}

fn run_native(source: &str) -> Result<common::BoundedOutput, String> {
    let _aot_guard = AOT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create scratch dir: {error}"))?;
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source)
        .map_err(|error| format!("could not write source: {error}"))?;

    let result = common::run_aot(&source_path, source)
        .map_err(|error| format!("could not run native binary: {error}"));
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_bytecode(source: &str) -> Result<(i32, Vec<String>), String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create VM scratch dir: {error}"))?;
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source)
        .map_err(|error| format!("could not write VM source: {error}"))?;

    let result = (|| {
        let bytecode = lirac::compile_with_imports(
            source_path
                .to_str()
                .ok_or_else(|| "VM source path is not UTF-8".to_owned())?,
            source,
        )?;
        liravm::run_with_capture(&bytecode)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[test]
fn aot_decoders_preserve_valid_unicode() {
    let output = run_native(
        r#"
        println(base64_decode("4pyT"))
        println(url_decode("%E2%9C%93"))
        "#,
    )
    .expect("valid decoder source should compile and run");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "✓\n✓\n");
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
}

#[test]
fn aot_decoders_replace_invalid_utf8_with_empty_strings_and_report_errors() {
    let source = r#"
        println(base64_decode("////"))
        println(url_decode("%FF"))
        println(url_decode("%GG"))
        println(url_decode("%A"))
        println(url_decode("%"))
        println(url_decode("left%20+right"))
        println(url_decode("%C3%A9+ok"))
        "#;
    let output = run_native(source).expect("malformed decoder source should compile and run");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "\n\n\n\n\nleft  right\né ok\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "base64_decode error: UTF-8 decode error: invalid utf-8 sequence of 1 bytes from index 0\n\
url_decode error: UTF-8 decode error: invalid utf-8 sequence of 1 bytes from index 0\n"
    );

    let (exit_code, lines) = run_bytecode(source).expect("malformed VM source should run");
    assert_eq!(exit_code, 0);
    assert_eq!(
        lines,
        vec![
            "".to_owned(),
            "".to_owned(),
            "".to_owned(),
            "".to_owned(),
            "".to_owned(),
            "left  right".to_owned(),
            "é ok".to_owned(),
        ]
    );
}

#[test]
fn jit_decoders_validate_unicode_before_returning_strings() {
    let source = r#"
        fn main() {
            if base64_decode("4pyT") != "✓" { println(1 / 0) }
            if url_decode("%E2%9C%93") != "✓" { println(1 / 0) }
            if base64_decode("////") != "" { println(1 / 0) }
            if url_decode("%FF") != "" { println(1 / 0) }
            if url_decode("%GG") != "" { println(1 / 0) }
            if url_decode("%A") != "" { println(1 / 0) }
            if url_decode("%") != "" { println(1 / 0) }
            if url_decode("left%20+right") != "left  right" { println(1 / 0) }
            if url_decode("%C3%A9+ok") != "é ok" { println(1 / 0) }
        }
        "#;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("create JIT scratch dir");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("write JIT source");
    let result = common::run_jit(source_path.to_str().expect("UTF-8 path"), source);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(result, Ok(0), "JIT decoder source failed: {result:?}");
}
