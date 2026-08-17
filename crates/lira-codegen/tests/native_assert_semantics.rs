//! Bounded native behavior for the formal core `assert(bool)` builtin.

use std::path::Path;

mod common;

#[test]
fn native_assert_continues_on_true_and_stops_on_false() {
    let passing = "assert(2 > 1)\nprintln(\"ok\")\n";
    let passing_aot = common::run_aot(Path::new("assert-passing.li"), passing)
        .expect("bounded passing AOT assertion runs");
    assert!(
        passing_aot.status.success(),
        "AOT stderr: {}",
        passing_aot.stderr_text()
    );
    assert_eq!(passing_aot.stdout, b"ok\n");
    let (passing_jit_status, passing_jit_output) =
        common::run_jit_capture("assert-passing.li", passing)
            .expect("bounded passing JIT assertion runs");
    assert_eq!(passing_jit_status, 0);
    assert_eq!(passing_jit_output, b"ok\n");

    let failing = "println(\"before\")\nassert(false)\nprintln(\"after\")\n";
    let failing_aot = common::run_aot(Path::new("assert-failing.li"), failing)
        .expect("bounded failing AOT assertion reports its status");
    assert!(!failing_aot.status.success());
    assert_eq!(failing_aot.stdout, b"before\n");
    assert!(failing_aot.stderr_text().contains("assertion failed"));

    let (failing_jit_status, failing_jit_output) =
        common::run_jit_capture("assert-failing.li", failing)
            .expect("bounded failing JIT assertion reports its status");
    assert_eq!(failing_jit_status, 1);
    assert_eq!(failing_jit_output, b"before\n");
}
