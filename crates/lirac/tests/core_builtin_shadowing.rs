//! End-to-end regressions for user functions shadowing core builtins.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    let program =
        liravm::bytecode::load(&bytecode).unwrap_or_else(|error| panic!("load {name}: {error}"));
    let mut vm = liravm::vm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);

    let status = vm
        .run()
        .unwrap_or_else(|error| panic!("run {name}: {error}"));
    assert_eq!(status, 0, "{name} exited with status {status}");
    vm.get_output().to_vec()
}

#[test]
fn user_print_shadows_core_print_and_returns_a_value() {
    let output = run_source(
        "shadow_print",
        r#"
fn print(value: string) -> string {
    return "user-print:" + value
}

println(print("ok"))
"#,
    );

    assert_eq!(output, ["user-print:ok"]);
}

#[test]
fn user_println_shadows_core_println_and_returns_a_value() {
    let output = run_source(
        "shadow_println",
        r#"
fn println(value: string) -> string {
    return "user-println:" + value
}

print(println("ok") + "\n")
"#,
    );

    assert_eq!(output, ["user-println:ok"]);
}

#[test]
fn user_assert_shadows_core_assert_and_returns_a_value() {
    let output = run_source(
        "shadow_assert",
        r#"
fn assert(condition: bool) -> string {
    return if condition { "user-assert:passed" } else { "user-assert:failed" }
}

println(assert(true))
"#,
    );

    assert_eq!(output, ["user-assert:passed"]);
}

#[test]
fn local_function_value_shadows_core_println() {
    let output = run_source(
        "local_shadow_println",
        r#"
let println = |value: string| "local:" + value
print(println("ok") + "\n")
"#,
    );

    assert_eq!(output, ["local:ok"]);
}

#[test]
fn imported_std_io_assert_accepts_a_message_argument() {
    let output = run_source(
        "imported_std_io_assert",
        r#"
import std.io

assert(false, "imported assertion")
println("after")
"#,
    );

    assert_eq!(output, ["[ASSERT FAILED] imported assertion", "after"]);
}
