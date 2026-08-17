//! Real-source coverage for spawn operands lowered through a closure wrapper.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    let (_exit_code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|error| panic!("run {name}: {error}"));
    output
}

#[test]
fn direct_named_spawn_call_still_runs() {
    let output = run_source(
        "direct_named_spawn_call",
        r#"
fn worker(value: int) {
    println("worker " + value)
}

fn main() {
    spawn worker(7)
    fiber_yield()
}
"#,
    );
    assert_eq!(output, vec!["worker 7"]);
}

#[test]
fn spawn_named_and_default_arguments_use_normal_call_binding() {
    let output = run_source(
        "spawn_named_default_arguments",
        r#"
fn worker(first: int, second: int = 9) {
    println(first + second)
}

fn main() {
    spawn worker(second: 4, first: 6)
    spawn worker(first: 3)
    fiber_yield()
    fiber_yield()
}
"#,
    );
    assert_eq!(output, vec!["10", "12"]);
}

#[test]
fn spawn_function_value_and_capture_execute_in_child() {
    let output = run_source(
        "spawn_function_value_capture",
        r#"
fn worker(value: int) {
    println("value " + value)
}

fn main() {
    let f = worker
    let captured = 8
    spawn f(captured)
    spawn { println("captured " + captured) }
    fiber_yield()
    fiber_yield()
}
"#,
    );
    assert_eq!(output, vec!["value 8", "captured 8"]);
}

#[test]
fn spawn_block_can_perform_channel_effects() {
    let output = run_source(
        "spawn_block_channel_effect",
        r#"
fn main() {
    let channel = chan(1)
    spawn { send(channel, 42) }
    fiber_yield()
    println(recv(channel))
}
"#,
    );
    assert_eq!(output, vec!["42"]);
}

#[test]
fn spawn_expression_produces_a_fiber_handle() {
    let output = run_source(
        "spawn_fiber_handle",
        r#"
fn worker() { }

fn main() {
    let handle = spawn worker()
    println(handle)
}
"#,
    );
    assert_eq!(output, vec!["<fiber>"]);
}

#[test]
fn spawn_capture_traverses_nested_select_body() {
    let output = run_source(
        "spawn_nested_select_capture",
        r#"
fn main() {
    let channel = chan(1)
    let value = 17
    spawn {
        select {
            _ => send(channel, value)
        }
    }
    fiber_yield()
    println(recv(channel))
}
"#,
    );
    assert_eq!(output, vec!["17"]);
}

#[test]
fn spawn_staged_struct_argument_executes_with_value_semantics() {
    let output = run_source(
        "spawn_struct_argument",
        r#"
struct Payload { value: int }

fn worker(payload: Payload) {
    println(payload.value)
}

fn main() {
    let payload = Payload { value: 23 }
    spawn worker(payload)
    fiber_yield()
}
"#,
    );
    assert_eq!(output, vec!["23"]);
}

#[test]
fn spawn_method_callees_reuse_static_and_instance_dispatch() {
    let output = run_source(
        "spawn_method_dispatch",
        r#"
struct Worker { value: int }
impl Worker {
    fn emit(self, channel: Channel<int>, extra: int = 2) {
        send(channel, self.value + extra)
    }
    fn static_emit(channel: Channel<int>, value: int) {
        send(channel, value)
    }
}

fn main() {
    let channel = chan(2)
    let worker = Worker { value: 3 }
    spawn worker.emit(channel, extra: 4)
    spawn Worker.static_emit(channel, 8)
    println(recv(channel))
    println(recv(channel))
}
"#,
    );
    assert_eq!(output, vec!["7", "8"]);
}

#[test]
fn spawn_operand_once_preserves_root_mutable_cell_access() {
    let output = run_source(
        "spawn_root_mutable_operand",
        r#"
var calls: int = 0

fn make(channel: Channel<int>) -> Channel<int> {
    calls = calls + 1
    return channel
}

fn worker(channel: Channel<int>) {
    send(channel, calls)
}

fn main() {
    let channel = chan(1)
    spawn worker(make(channel))
    let value = recv(channel)
    println(value)
    println(calls)
}
"#,
    );
    assert_eq!(output, vec!["1", "1"]);
}

#[test]
fn spawn_function_capture_propagates_through_helper_calls() {
    let output = run_source(
        "spawn_transitive_root_capture",
        r#"
var value: int = 12

fn helper(channel: Channel<int>) {
    send(channel, value)
}

fn worker(channel: Channel<int>) {
    helper(channel)
}

fn main() {
    let channel = chan(1)
    spawn worker(channel)
    println(recv(channel))
}
"#,
    );
    assert_eq!(output, vec!["12"]);
}

#[test]
fn spawn_block_capture_respects_nested_shadowing() {
    let output = run_source(
        "spawn_nested_shadowing",
        r#"
fn main() {
    let value = 1
    {
        let value = 2
        spawn { println(value) }
    }
    fiber_yield()
}
"#,
    );
    assert_eq!(output, vec!["2"]);
}

#[test]
fn spawn_call_operands_are_evaluated_once_in_parent() {
    let output = run_source(
        "spawn_operand_evaluation_once",
        r#"
fn evaluate() -> int {
    println("evaluated")
    return 4
}

fn worker(value: int) {
    println("worker " + value)
}

fn main() {
    spawn worker(evaluate())
    fiber_yield()
}
"#,
    );
    assert_eq!(output, vec!["evaluated", "worker 4"]);
}

#[test]
fn spawned_child_runtime_error_keeps_function_context() {
    let source = r#"
fn fail() {
    let zero = 0
    let value = 1 / zero
    println(value)
}

fn main() {
    spawn fail()
    fiber_yield()
}
"#;
    let bytecode =
        lirac::compile_with_imports("spawn_child_error", source).expect("source should compile");
    let (_output, error) =
        liravm::run_with_capture_structured(&bytecode).expect_err("the spawned child should fail");
    assert_eq!(error.message, "Division by zero");
    assert_eq!(error.line, Some(4));
    assert_eq!(error.column, Some(5));
    assert!(
        error.stack.iter().any(|name| name == "fail"),
        "stack: {:?}",
        error.stack
    );
    assert_eq!(error.stack, vec!["fail".to_string()]);
}

#[test]
fn spawn_undefined_capture_is_a_compiler_error() {
    let source = r#"
fn main() {
    spawn { println(undefined_value) }
}
"#;
    let error = lirac::compile_with_imports("spawn_undefined_capture", source)
        .expect_err("undefined captures must be rejected");
    assert!(
        error.contains("Undefined variable: undefined_value"),
        "{error}"
    );
}

#[test]
fn spawn_non_callable_invocation_is_a_checker_error() {
    let source = r#"
fn main() {
    spawn 1()
}
"#;
    let error = lirac::compile_with_imports("spawn_non_callable", source)
        .expect_err("a non-callable spawn callee must be rejected");
    assert!(
        error.contains("Cannot call non-function type: 'int'"),
        "{error}"
    );
}
