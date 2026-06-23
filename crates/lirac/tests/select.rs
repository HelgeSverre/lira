//! Integration tests for correct `select` semantics (blocking + polling).
//!
//! These compile real Lira source via `compile_with_imports` and execute it
//! through `run_with_capture`. They deliberately do NOT call `main()` (codegen
//! auto-invokes a zero-arg `main`, and existing `tests/samples/*.li` select
//! programs double-run because of that). All programs here are top-level code
//! with deterministic single-producer/single-consumer orderings.

/// Compile and run a Lira source string, returning captured output lines.
fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode =
        lirac::compile_with_imports(name, source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let (_code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|e| panic!("run {name}: {e}"));
    output
}

/// A ready buffered value beats the default arm: the recv arm fires.
#[test]
fn select_ready_recv_beats_default() {
    let source = r#"
let ch = chan(1)
send(ch, 7)
select {
    <-ch => println("recv 7")
    _ => println("default")
}
println("done")
"#;
    let output = run_source("select_ready_recv_beats_default", source);
    assert_eq!(
        output,
        vec!["recv 7".to_string(), "done".to_string()],
        "ready buffered recv must win over default",
    );
}

/// Default-only poll: no arm is ready, so the default arm runs (not the recv).
/// This is the fall-through bug: today this wrongly prints "recv".
#[test]
fn select_default_runs_when_no_arm_ready() {
    let source = r#"
let ch = chan(1)
select {
    <-ch => println("recv")
    _ => println("default only")
}
println("after")
"#;
    let output = run_source("select_default_runs_when_no_arm_ready", source);
    assert_eq!(
        output,
        vec!["default only".to_string(), "after".to_string()],
        "empty channel with default must run the default arm",
    );
}

/// Blocking select (no default) parks the fiber until a spawned worker sends
/// on an unbuffered channel, then the recv arm fires with the sent value.
#[test]
fn select_blocks_until_unbuffered_send() {
    let source = r#"
let ch = chan()
fn worker(c: Channel<int>) {
    send(c, 99)
}
spawn worker(ch)
println("waiting")
select {
    <-ch => println("got 99")
}
println("done")
"#;
    let output = run_source("select_blocks_until_unbuffered_send", source);
    assert_eq!(
        output,
        vec![
            "waiting".to_string(),
            "got 99".to_string(),
            "done".to_string()
        ],
        "select with no default must park until the worker sends",
    );
}

/// Real send arm: a spawned receiver is waiting on an unbuffered channel; the
/// select send arm hands the value off (proving the fake-send is gone) and the
/// receiver observes it.
#[test]
fn select_send_arm_delivers_to_waiting_receiver() {
    let source = r#"
let ch = chan()
fn consumer(c: Channel<int>) {
    select {
        v = <-c => println("got " + v)
    }
}
spawn consumer(ch)
fiber_yield()
select {
    5 -> ch => println("sent")
}
fiber_yield()
println("done")
"#;
    let output = run_source("select_send_arm_delivers_to_waiting_receiver", source);
    assert_eq!(
        output,
        vec!["sent".to_string(), "got 5".to_string(), "done".to_string()],
        "select send arm must really transfer the value to the waiting receiver",
    );
}

/// Recv arm binding a variable: `v = <-ch` must be in scope in the arm body.
#[test]
fn select_recv_binds_variable() {
    let source = r#"
let ch = chan(1)
send(ch, 21)
select {
    v = <-ch => println("v=" + v)
    _ => println("default")
}
println("end")
"#;
    let output = run_source("select_recv_binds_variable", source);
    assert_eq!(
        output,
        vec!["v=21".to_string(), "end".to_string()],
        "recv arm variable binding must be usable in the arm body",
    );
}
