//! Real-source regressions for VM tuple value semantics.

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode =
        lirac::compile_with_imports(name, source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let (code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|e| panic!("run {name}: {e}"));
    assert_eq!(code, 0, "{name} exited with status {code}");
    output
}

#[test]
fn tuple_assignment_copies_nested_tuples_but_not_arrays() {
    assert_eq!(
        run_source(
            "tuple_assignment",
            r#"
let values = [1]
let first = (1, (2, 3), values)
let second = first
second[2][0] = 7
println(first[1][0])
println(second[1][0])
println(first[2][0])
"#,
        ),
        vec!["2", "2", "7"]
    );
}

#[test]
fn tuple_containing_struct_and_class_preserves_their_semantics() {
    assert_eq!(
        run_source(
            "tuple_aggregates",
            r#"
struct Point { value: int }
class Box { value: int }
let point = Point { value: 1 }
let object = Box { value: 2 }
let first = (point, object)
let second = first
second[0].value = 8
second[1].value = 9
println(first[0].value)
println(first[1].value)
"#,
        ),
        vec!["1", "9"]
    );
}

#[test]
fn tuple_function_boundaries_and_closure_capture_preserve_values() {
    assert_eq!(
        run_source(
            "tuple_boundaries",
            r#"
fn read(value: (int, int)) -> int {
    return value[0]
}
fn make() -> int {
    let value = (4, 5)
    let get = || value[0]
    return get()
}
let source = (1, 2)
println(source[0])
println(read(source))
println(make())
"#,
        ),
        vec!["1", "1", "4"]
    );
}

#[test]
fn tuple_channel_and_select_payloads_are_copied() {
    assert_eq!(
        run_source(
            "tuple_channel_select",
            r#"
let ch: Channel<(int, int)> = chan(1)
let source = (1, 2)
send(ch, source)
select {
    received = <-ch => {
        println(received[0])
    }
    _ => println(0)
}
println(source[0])
"#,
        ),
        vec!["1", "1"]
    );
}

#[test]
fn tuple_display_json_and_type_checks_remain_distinct_from_arrays() {
    assert_eq!(
        run_source(
            "tuple_runtime_shape",
            r#"
let tuple = (1, 2)
let array = [1, 2]
println(tuple)
println(tuple as string)
println(len(tuple))
println(json_stringify(tuple))
println(tuple is (int, int))
println(array is [int])
println(tuple is [int])
println(array is (int, int))
"#,
        ),
        vec!["(1, 2)", "(1, 2)", "2", "[1,2]", "true", "true", "false", "false",]
    );
}

#[test]
fn tuple_pattern_matching_and_round_trip_execution_work() {
    let source = r#"
let value = (1, (2, 3))
match value {
    (1, (x, y)) => println(x + y)
    _ => println(0)
}
"#;
    let bytecode = lirac::compile(source).expect("compile tuple pattern");
    let first = liravm::run_with_capture(&bytecode).expect("run tuple pattern");
    let program = liravm::bytecode::load(&bytecode).expect("load tuple bytecode");
    let mut loaded_vm = liravm::VM::new(program);
    loaded_vm.set_fiber_mode(true);
    loaded_vm.set_capture_output(true);
    let loaded_status = loaded_vm.run().expect("execute loaded tuple bytecode");
    let second = (loaded_status, loaded_vm.get_output().to_vec());
    assert_eq!(first, (0, vec!["5".to_owned()]));
    assert_eq!(second, first);

    // Decode a deliberately minimal tuple literal instead of searching raw
    // bytes, where an operand can coincidentally equal an opcode value.
    let literal = lirac::compile("println((1, 2))").expect("compile tuple literal");
    let literal = liravm::bytecode::load(&literal).expect("load tuple literal bytecode");
    let mut opcodes = Vec::new();
    let mut offset = 0;
    while offset < literal.code.len() {
        let opcode = lira_core::opcode::Opcode::from_byte(literal.code[offset])
            .unwrap_or_else(|| panic!("unknown opcode at byte {offset}"));
        opcodes.push(opcode);
        offset += 1;
        offset += match opcode {
            lira_core::opcode::Opcode::LoadConst | lira_core::opcode::Opcode::Jump => 2,
            lira_core::opcode::Opcode::NewTuple
            | lira_core::opcode::Opcode::Dup
            | lira_core::opcode::Opcode::CopyValue
            | lira_core::opcode::Opcode::TupleSet
            | lira_core::opcode::Opcode::Print
            | lira_core::opcode::Opcode::Println
            | lira_core::opcode::Opcode::Pop
            | lira_core::opcode::Opcode::Halt => 0,
            other => panic!("unexpected opcode {other:?} in minimal tuple literal"),
        };
    }
    assert!(opcodes.contains(&lira_core::opcode::Opcode::NewTuple));
    assert!(opcodes.contains(&lira_core::opcode::Opcode::TupleSet));
    assert!(!opcodes.contains(&lira_core::opcode::Opcode::ArraySet));
}

#[test]
fn tuple_index_assignment_is_diagnosed_at_the_target_span() {
    let source = "let value = (1, 2)\nvalue[0] = 3\nvalue[1] += 4\nprintln(value[0])";
    assert!(
        lirac::compile(source).is_err(),
        "tuple mutation must be rejected by the normal compile path"
    );
    let diagnostics = lirac::analyze(source)
        .expect("tuple mutation source parses")
        .diagnostics;
    let tuple_errors: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.message == "Cannot assign to tuple index; tuples are immutable"
        })
        .collect();
    assert_eq!(
        tuple_errors.len(),
        2,
        "unexpected diagnostics: {diagnostics:?}"
    );
    assert_eq!(tuple_errors[0].line, 2);
    assert_eq!(tuple_errors[0].column, 1);
    assert_eq!(tuple_errors[1].line, 3);
    assert_eq!(tuple_errors[1].column, 1);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.line != 4),
        "neighboring tuple reads must remain valid: {diagnostics:?}"
    );
}

#[test]
fn old_bytecode_versions_are_rejected_after_tuple_encoding_change() {
    let mut bytecode = lirac::compile("println((1, 2))").expect("compile tuple bytecode");
    bytecode[4..8].copy_from_slice(&1_u32.to_le_bytes());
    let error = liravm::bytecode::load(&bytecode)
        .err()
        .expect("v1 bytecode must be rejected");
    assert!(
        error.contains("Unsupported version: 1"),
        "unexpected error: {error}"
    );
}

#[test]
fn tuple_bounds_errors_are_reported_by_compiled_source() {
    for (name, source) in [
        (
            "tuple_negative_get",
            "let value: any = (1, 2)\nprintln(value[-1])",
        ),
        (
            "tuple_oob_get",
            "let value: any = (1, 2)\nprintln(value[2])",
        ),
    ] {
        let bytecode = lirac::compile(source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
        let (_, error) = liravm::run_with_capture_structured(&bytecode)
            .expect_err("tuple bounds access should fail");
        assert!(
            error.message.contains("out of bounds")
                || error.message.contains("Index out of bounds"),
            "unexpected {name} error: {}",
            error.message
        );
    }
}
