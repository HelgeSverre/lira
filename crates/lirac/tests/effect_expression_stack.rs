//! End-to-end stack-balance regressions for effect-only expressions.
//!
//! These intentionally use effects where an expression value is required.
//! Bytecode lowering must materialize the language `null` value after an
//! effect opcode consumes its operands, so enclosing blocks, control flow, and
//! local bindings retain the one-value expression invariant.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    let program = liravm::bytecode::load(&bytecode)
        .unwrap_or_else(|error| panic!("load compiled {name} bytecode: {error}"));
    let mut vm = liravm::vm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);

    let status = vm
        .run()
        .unwrap_or_else(|error| panic!("execute {name}: {error}"));
    assert_eq!(status, 0, "{name} exited with status {status}");
    vm.get_output().to_vec()
}

#[test]
fn effect_only_block_can_be_bound_to_a_local() {
    let output = run_source(
        "effect_only_block_bound_to_local",
        r#"
let ignored = {
    println("block effect")
}
println("after block")
"#,
    );

    assert_eq!(output, ["block effect", "after block"]);
}

#[test]
fn if_expression_preserves_effects_before_value_arms() {
    let output = run_source(
        "if_effect_and_value_arms",
        r#"
fn choose(flag) {
    return if flag as bool {
        println("if effect")
        7
    } else {
        8
    }
}

println(choose(true))
println(choose(false))
"#,
    );

    assert_eq!(output, ["if effect", "7", "8"]);
}

#[test]
fn match_expression_preserves_effects_before_value_arms() {
    let output = run_source(
        "match_effect_and_value_arms",
        r#"
fn choose(value) {
    return match value {
        0 => {
            println("match effect")
            42
        },
        _ => 42
    }
}

println(choose(0))
println(choose(1))
"#,
    );

    assert_eq!(output, ["match effect", "42", "42"]);
}

#[test]
fn array_push_effects_are_valid_final_block_expressions() {
    let output = run_source(
        "array_push_effects_as_block_values",
        r#"
let values = [1]
let builtin_push_result = {
    push(values, 2)
}
let method_push_result = {
    values.push(3)
}

println(values[0])
println(values[1])
println(values[2])
"#,
    );

    assert_eq!(output, ["1", "2", "3",]);
}

#[test]
fn yield_and_assert_effects_compose_inside_blocks() {
    let output = run_source(
        "yield_and_assert_effect_blocks",
        r#"
let yielded = {
    fiber_yield()
}
let asserted = {
    assert(2 + 2 == 4)
}

println("effects completed")
"#,
    );

    assert_eq!(output, ["effects completed"]);
}

#[test]
fn select_effect_body_produces_a_value_for_its_enclosing_expression() {
    let output = run_source(
        "select_effect_body_as_expression",
        r#"
let channel = chan(1)
send(channel, 9)
let selected = select {
    value = <-channel => println("selected " + value)
}

println("select completed")
"#,
    );

    assert_eq!(output, ["selected 9", "select completed"]);
}
