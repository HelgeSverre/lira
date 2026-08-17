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

#[test]
fn modulo_promotes_mixed_int_float_operands() {
    // `Mod` must accept mixed int/float operands (as the type checker and the
    // other arithmetic operators already do), promoting to float. It used to
    // reject them with "Cannot modulo int by float" even though the checker
    // accepted the same source — a checker/runtime divergence.
    let out = run("println(10 % 2.5)\n").unwrap();
    assert_eq!(out, vec!["0".to_string()]);
    let out = run("println(10.5 % 2)\n").unwrap();
    assert_eq!(out, vec!["0.5".to_string()]);
}

#[test]
fn modulo_results_match_native_for_mixed_operands() {
    // Guard the full matrix of int/float operand combinations so the VM never
    // regresses to rejecting a pair the native backend computes.
    let out =
        run("println(7 % 2)\nprintln(7.0 % 2.0)\nprintln(7 % 2.0)\nprintln(7.0 % 2)").unwrap();
    assert_eq!(
        out,
        vec![
            "1".to_string(),
            "1".to_string(),
            "1".to_string(),
            "1".to_string()
        ]
    );
}
