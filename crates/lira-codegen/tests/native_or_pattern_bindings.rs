//! Bounded VM/AOT/JIT parity for binding and nested OR-patterns.

use std::path::Path;

mod common;

const SOURCE: &str = r#"
    enum Choice { Left(int), Right(int), Pair(int, int) }

    fn value(choice: Choice) -> int {
        return match choice {
            Choice::Left(x) | Choice::Right(x) => x,
            Choice::Pair(x, 0 | 1) => x
        }
    }

    println(value(Choice::Left(11)))
    println(value(Choice::Right(22)))
    println(value(Choice::Pair(33, 0)))
    println(value(Choice::Pair(44, 1)))
"#;

const EXPECTED: &str = "11\n22\n33\n44";

fn run_vm(source: &str) -> Result<String, String> {
    let bytecode = lirac::compile(source)?;
    let (status, lines) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM exited with status {status}"));
    }
    Ok(lines.join("\n"))
}

#[test]
fn matching_or_alternative_binds_its_payload_across_backends() {
    assert_eq!(run_vm(SOURCE).expect("VM OR-pattern run"), EXPECTED);

    let aot = common::run_aot(Path::new("native_or_pattern_bindings.li"), SOURCE)
        .expect("bounded AOT OR-pattern run");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(aot.stdout_text().trim_end(), EXPECTED);

    let (jit_status, jit_stdout) = common::run_jit_capture("native_or_pattern_bindings.li", SOURCE)
        .expect("bounded JIT OR-pattern run");
    assert_eq!(jit_status, 0);
    assert_eq!(String::from_utf8_lossy(&jit_stdout).trim_end(), EXPECTED);
}

#[test]
fn inconsistent_or_bindings_are_rejected_before_native_execution() {
    let source = r#"
        enum Choice { Left(int), Right(int) }
        fn bad(value: Choice) -> int {
            return match value {
                Choice::Left(x) | Choice::Right(y) => x
            }
        }
    "#;
    let compiler_error = lirac::compile(source).expect_err("checker must reject OR bindings");
    assert!(compiler_error.contains("must bind the same variables"));

    let native_error = common::build_aot(Path::new("invalid_or_pattern.li"), source)
        .expect_err("native build must stop at checker diagnostics");
    assert!(native_error.contains("must bind the same variables"));
}
