//! Integer overflow must be deterministic and must never panic the VM.
//!
//! Found by the differential fuzzer: the VM used plain `a + b` / `a * b` /
//! `-n` / `base.pow(e)`, which are overflow-checked in debug builds — so an
//! overflowing Lira program aborted the host process with a Rust panic, while
//! release builds silently wrapped. Same source, different behavior by profile,
//! and a hard crash instead of a catchable outcome. Overflow is now defined as
//! two's-complement wraparound everywhere (matching the previous release
//! behavior), identical in debug and release.

fn run(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile_with_imports("test.li", source)?;
    let (_exit_code, output) = liravm::run_with_capture(&bytecode)?;
    Ok(output)
}

#[test]
fn mul_overflow_wraps() {
    // i64::MAX * 2 wraps to -2.
    let out = run("println(9223372036854775807 * 2)\n").unwrap();
    assert_eq!(out, vec!["-2".to_string()]);
}

#[test]
fn add_overflow_wraps() {
    // i64::MAX + 1 wraps to i64::MIN.
    let out = run("println(9223372036854775807 + 1)\n").unwrap();
    assert_eq!(out, vec!["-9223372036854775808".to_string()]);
}

#[test]
fn neg_min_wraps() {
    // i64::MIN (built by wrapping MAX+1, since the literal 2^63 can't be
    // written directly) has no positive representation; -MIN wraps to MIN.
    let out = run("var min = 9223372036854775807 + 1\nprintln(-min)\n").unwrap();
    assert_eq!(out, vec!["-9223372036854775808".to_string()]);
}

#[test]
fn sub_overflow_wraps() {
    // i64::MIN - 1 wraps to i64::MAX.
    let out = run("var min = 9223372036854775807 + 1\nprintln(min - 1)\n").unwrap();
    assert_eq!(out, vec!["9223372036854775807".to_string()]);
}
