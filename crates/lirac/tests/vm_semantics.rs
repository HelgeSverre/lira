//! Source-to-bytecode-to-VM regressions for VM/native semantic parity.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode =
        lirac::compile_with_imports(name, source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let (code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|e| panic!("run {name}: {e}"));
    assert_eq!(code, 0, "{name} exited with status {code}");
    output
}

fn run_error(name: &str, source: &str) -> (Vec<String>, liravm::RuntimeError) {
    let bytecode =
        lirac::compile_with_imports(name, source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
    liravm::run_with_capture_structured(&bytecode).expect_err("source should fail in the VM")
}

#[test]
fn recv_returns_buffered_int_payload_only() {
    let output = run_source(
        "recv_buffered_int",
        r#"
let ch = chan(1)
send(ch, 42)
println(recv(ch))
"#,
    );
    assert_eq!(output, vec!["42"]);
}

#[test]
fn recv_returns_buffered_string_payload_only() {
    let output = run_source(
        "recv_buffered_string",
        r#"
let ch = chan(1)
send(ch, "payload")
println(recv(ch))
"#,
    );
    assert_eq!(output, vec!["payload"]);
}

#[test]
fn closed_receive_returns_null_for_string_channel() {
    let output = run_source(
        "recv_closed_string",
        r#"
let ch: Channel<string> = chan(1)
close(ch)
println(recv(ch))
"#,
    );
    assert_eq!(output, vec!["null"]);
}

#[test]
fn recv_expression_statement_does_not_leave_internal_ok_on_stack() {
    let output = run_source(
        "recv_expression_statement",
        r#"
let ch = chan(2)
send(ch, 1)
send(ch, 2)
recv(ch)
println(recv(ch))
"#,
    );
    assert_eq!(output, vec!["2"]);
}

#[test]
fn len_supports_maps_and_keeps_utf8_string_byte_length() {
    let output = run_source(
        "len_map_and_utf8",
        r#"
let values = {"b": 2, "a": 1}
println(len(values))
println(len("å"))
"#,
    );
    assert_eq!(output, vec!["2", "2"]);
}

#[test]
fn aggregate_rendering_bounds_array_and_map_cycles() {
    let output = run_source(
        "render_cycles",
        r#"
var array: [any] = [0]
push(array, array)
println(array)
println(array as string)
var map = json_parse("{\"z\":1,\"link\":null}")
map.link = map
println(map)
println(map as string)
"#,
    );
    assert_eq!(
        output,
        vec![
            "[0, [...]]",
            "[0, [...]]",
            "{link: {...}, z: 1}",
            "{link: {...}, z: 1}",
        ]
    );
}

#[test]
fn mutable_root_capture_reads_live_value_after_assignment() {
    let output = run_source(
        "mutable_root_capture",
        r#"
var n = 1
let f = || n
n = 2
println(f())
"#,
    );
    assert_eq!(output, vec!["2"]);
}

#[test]
fn local_capture_remains_value_snapshot() {
    let output = run_source(
        "local_capture_snapshot",
        r#"
fn make() {
    var n = 1
    let f = || n
    n = 2
    return f()
}
println(make())
"#,
    );
    assert_eq!(output, vec!["1"]);
}

#[test]
fn local_shadow_of_global_mutable_is_not_treated_as_a_cell() {
    let output = run_source(
        "global_shadow_capture",
        r#"
var n = 1
{
    let n = 3
    let f = || n
    println(n)
    println(f())
}
println(n)
"#,
    );
    assert_eq!(output, vec!["3", "3", "1"]);
}

#[test]
fn direct_child_runtime_error_propagates_with_child_context() {
    let source = r#"
fn worker() {
    let impossible = 1 / 0
    println(impossible)
}
fn main() {
    spawn worker()
    fiber_yield()
    println("root must not hide child failure")
}
"#;
    let (output, error) = run_error("child_runtime_error", source);
    assert!(
        output.is_empty(),
        "failed child must not produce output: {output:?}"
    );
    assert_eq!(error.message, "Division by zero");
    assert_eq!(error.line, Some(3));
    assert!(error.stack.iter().any(|name| name == "worker"));

    let bytecode = lirac::compile_with_imports("child_runtime_error_rendered", source)
        .expect("source should compile");
    let rendered = liravm::run_with_capture(&bytecode).expect_err("child failure should escape");
    assert_eq!(rendered, "3:5: Division by zero\n  at worker");
}

#[test]
fn blocked_sender_failure_after_close_propagates() {
    let (output, error) = run_error(
        "blocked_sender_after_close",
        r#"
fn worker(ch: Channel<int>) {
    send(ch, 1)
}
fn main() {
    let ch = chan()
    spawn worker(ch)
    fiber_yield()
    close(ch)
    println("root must not hide closed-send failure")
}
"#,
    );
    assert!(
        output.is_empty(),
        "failed child must not produce output: {output:?}"
    );
    assert_eq!(error.message, "send on closed channel");
    assert!(
        error.line.is_some(),
        "closed send should retain a source line"
    );
    assert!(error.stack.iter().any(|name| name == "worker"));
}

#[test]
fn first_failing_child_is_deterministic() {
    let (output, error) = run_error(
        "first_child_failure",
        r#"
fn first() {
    let impossible = 1 / 0
}
fn second() {
    let impossible = 1 % 0
}
fn main() {
    spawn first()
    spawn second()
    fiber_yield()
}
"#,
    );
    assert!(output.is_empty());
    assert_eq!(error.message, "Division by zero");
    assert!(error.stack.iter().any(|name| name == "first"));
    assert!(!error.stack.iter().any(|name| name == "second"));
}

#[test]
fn successful_child_does_not_change_normal_exit() {
    assert_eq!(
        run_source(
            "successful_child_exit",
            r#"
fn worker() {
    println("child ok")
}
fn main() {
    spawn worker()
    fiber_yield()
    println("root ok")
}
"#,
        ),
        vec!["child ok", "root ok"]
    );
}

#[test]
fn child_error_after_root_returns_is_not_lost() {
    let (output, error) = run_error(
        "late_child_runtime_error",
        r#"
fn worker() {
    fiber_yield()
    let impossible = 1 / 0
}
fn main() {
    spawn worker()
    println("root returned")
    fiber_yield()
    fiber_yield()
}
"#,
    );
    assert_eq!(output, vec!["root returned"]);
    assert_eq!(error.message, "Division by zero");
    assert!(error.stack.iter().any(|name| name == "worker"));
}

#[test]
fn debug_session_reports_child_failure_location() {
    let source = r#"
fn worker() {
    let impossible = 1 / 0
}
fn main() {
    spawn worker()
    fiber_yield()
}
"#;
    let bytecode =
        lirac::compile_with_imports("debug_child_failure", source).expect("source should compile");
    let session = liravm::DebugSession::new();
    session.set_fiber_mode(true);
    session
        .load(source, bytecode)
        .expect("debug session should load source");
    let event = session
        .run_to_completion()
        .expect("debug session should return an error event");
    match event {
        liravm::DebugEvent::Error { message, location } => {
            assert_eq!(message, "3:5: Division by zero\n  at worker");
            assert_eq!(location, Some((3, 5)));
        }
        other => panic!("expected child runtime error, got {other:?}"),
    }
}
