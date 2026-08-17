//! Real-source regressions for VM casts whose bytecode carries only an outer tag.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    let program = liravm::bytecode::load(&bytecode)
        .unwrap_or_else(|error| panic!("load {name} bytecode: {error}"));
    let mut vm = liravm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);
    let status = vm
        .run()
        .unwrap_or_else(|error| panic!("execute {name}: {error}"));
    assert_eq!(status, 0, "{name} exited with status {status}");
    vm.get_output().to_vec()
}

fn run_error(name: &str, source: &str) -> (Vec<String>, liravm::RuntimeError) {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    liravm::run_with_capture_structured(&bytecode).expect_err("source should fail in the VM")
}

#[test]
fn array_cast_accepts_an_array_and_rejects_a_scalar() {
    assert_eq!(
        run_source(
            "array_cast_outer_tag_valid",
            r#"
let value: any = [1, 2]
let values = value as [int]
println(values[1])
"#,
        ),
        ["2"]
    );

    let (output, error) = run_error(
        "array_cast_outer_tag_invalid",
        r#"
let value: any = 1
println("before")
let values = value as [int]
println("after")
"#,
    );
    assert_eq!(output, ["before"]);
    assert_eq!(error.message, "Cannot cast int to array");
}

#[test]
fn function_cast_accepts_a_function_and_rejects_a_scalar() {
    assert_eq!(
        run_source(
            "function_cast_outer_tag_valid",
            r#"
fn identity(value: int) -> int { return value }
let value: any = identity
let typed = value as fn(int) -> int
println(typed(7))
"#,
        ),
        ["7"]
    );

    let (output, error) = run_error(
        "function_cast_outer_tag_invalid",
        r#"
let value: any = 1
println("before")
let typed = value as fn(int) -> int
println("after")
"#,
    );
    assert_eq!(output, ["before"]);
    assert_eq!(error.message, "Cannot cast int to function");
}

#[test]
fn tuple_cast_accepts_a_tuple_and_rejects_a_scalar() {
    assert_eq!(
        run_source(
            "tuple_cast_outer_tag_valid",
            r#"
let value: any = (1, "one")
let typed = value as (int, string)
println(typed[0])
println(typed[1])
"#,
        ),
        ["1", "one"]
    );

    let (output, error) = run_error(
        "tuple_cast_outer_tag_invalid",
        r#"
let value: any = 1
println("before")
let typed = value as (int, string)
println("after")
"#,
    );
    assert_eq!(output, ["before"]);
    assert_eq!(error.message, "Cannot cast int to tuple");
}

#[test]
fn channel_cast_accepts_a_channel_and_rejects_a_scalar() {
    assert_eq!(
        run_source(
            "channel_cast_outer_tag_valid",
            r#"
let value: any = chan(1)
let typed = value as Channel<int>
send(typed, 7)
println(recv(typed))
"#,
        ),
        ["7"]
    );

    let (output, error) = run_error(
        "channel_cast_outer_tag_invalid",
        r#"
let value: any = 1
println("before")
let typed = value as Channel<int>
println("after")
"#,
    );
    assert_eq!(output, ["before"]);
    assert_eq!(error.message, "Cannot cast int to channel");
}
