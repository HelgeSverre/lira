//! Boundary integer literals.
//!
//! `i64::MIN` is `-9223372036854775808`, whose *magnitude* 2^63 is one past
//! `i64::MAX`. The lexer tokenizes `-` and the number separately, so the
//! magnitude must be lexable (as u64) and the negation folded by the parser;
//! otherwise `i64::MIN` is simply unwritable. A bare 2^63 (no minus) is still
//! out of range and must stay an error.

fn run(source: &str) -> Result<Vec<String>, String> {
    let bytecode = lirac::compile_with_imports("test.li", source)?;
    let (_exit_code, output) = liravm::run_with_capture(&bytecode)?;
    Ok(output)
}

#[test]
fn i64_min_literal_is_representable() {
    let out = run("println(-9223372036854775808)\n").unwrap();
    assert_eq!(out, vec!["-9223372036854775808".to_string()]);
}

#[test]
fn i64_min_literal_in_expression() {
    // Used as a value, not just printed.
    let out = run("var x = -9223372036854775808\nprintln(x + 1)\n").unwrap();
    assert_eq!(out, vec!["-9223372036854775807".to_string()]);
}

#[test]
fn i64_max_literal_still_works() {
    let out = run("println(9223372036854775807)\n").unwrap();
    assert_eq!(out, vec!["9223372036854775807".to_string()]);
}

#[test]
fn bare_two_pow_63_is_out_of_range() {
    // Positive 2^63 has no `int` representation — must be a clean error.
    let err = run("println(9223372036854775808)\n").unwrap_err();
    assert!(
        err.to_lowercase().contains("out of range") || err.to_lowercase().contains("range"),
        "expected an out-of-range error, got: {err}"
    );
}

#[test]
fn far_out_of_range_literal_errors() {
    // Beyond u64 too — still a clean error, not a panic.
    let err = run("println(99999999999999999999999999)\n").unwrap_err();
    assert!(!err.is_empty(), "expected an error");
}
