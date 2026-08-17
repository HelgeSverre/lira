//! End-to-end regressions for value-producing control-flow expressions.
//!
//! In particular, a call in the final branch of an `if`/`match` must not be
//! rewritten as the tail call of the whole return expression. The branches
//! converge at a shared return boundary in the callee; jumping past that
//! boundary re-enters the caller's bytecode and can loop indefinitely.

use lira_core::opcode::Opcode;

fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode =
        lirac::compile_with_imports(name, source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let (exit_code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|e| panic!("run {name}: {e}"));
    assert_eq!(exit_code, 0, "{name} exited with status {exit_code}");
    output
}

#[test]
fn mixed_any_if_return_returns_from_both_branches() {
    let output = run_source(
        "mixed_any_if_return",
        r#"
fn identity(value) {
    return value
}

fn choose(flag) {
    return if flag as bool { identity(1) } else { identity("one") }
}

println(choose(true))
println(choose(false))
"#,
    );
    assert_eq!(output, vec!["1", "one"]);
}

#[test]
fn mixed_any_match_return_returns_from_both_arms() {
    let output = run_source(
        "mixed_any_match_return",
        r#"
fn identity(value) {
    return value
}

fn choose(flag) {
    return match flag {
        true => identity(1),
        false => identity("one")
    }
}

println(choose(true))
println(choose(false))
"#,
    );
    assert_eq!(output, vec!["1", "one"]);
}

#[test]
fn mixed_any_block_return_preserves_nested_if_value() {
    let output = run_source(
        "mixed_any_block_return",
        r#"
fn identity(value) {
    return value
}

fn choose(flag) {
    return {
        let result = if flag as bool {
            identity(1)
        } else {
            identity("one")
        }
        result
    }
}

println(choose(true))
println(choose(false))
"#,
    );
    assert_eq!(output, vec!["1", "one"]);
}

#[test]
fn control_flow_return_has_no_tail_call_or_jump_into_entry() {
    let source = r#"
fn identity(value) {
    return value
}

fn choose(flag) {
    return if flag as bool { identity(1) } else { identity("one") }
}

println(choose(true))
"#;
    let bytecode = lirac::compile_with_imports("control_flow_return_layout", source)
        .expect("source should compile");
    let program = liravm::bytecode::load(&bytecode).expect("compiled bytecode should load");
    let choose = program
        .debug_info
        .function_symbols
        .iter()
        .find(|function| function.name == "choose")
        .expect("choose function should be present");
    let main_delta = i16::from_le_bytes(
        program.code[1..3]
            .try_into()
            .expect("entry jump must have a two-byte operand"),
    );
    let main_start = (3i32 + i32::from(main_delta)) as u32;
    let function_end = program
        .debug_info
        .function_symbols
        .iter()
        .filter(|function| function.code_offset > choose.code_offset)
        .map(|function| function.code_offset)
        .chain(std::iter::once(main_start))
        .chain(std::iter::once(program.code.len() as u32))
        .min()
        .expect("function end should be discoverable");
    let function_start = choose.code_offset as usize;
    let function_end = function_end as usize;
    let function_code = &program.code[function_start..function_end];
    assert!(
        !function_code.is_empty(),
        "choose function must have bytecode"
    );

    // Every relative branch target in choose must remain inside the function,
    // rather than jumping to the top-level entry after the function body.
    let mut offset = function_start;
    let mut saw_return = false;
    let mut saw_tail_call = false;
    while offset < function_end {
        let opcode = program.code[offset];
        offset += 1;
        match Opcode::from_byte(opcode) {
            Some(Opcode::Jump) | Some(Opcode::JumpIfFalse) | Some(Opcode::JumpIfTrue) => {
                let delta = i16::from_le_bytes(
                    program.code[offset..offset + 2]
                        .try_into()
                        .expect("jump must have a two-byte operand"),
                );
                let target = (offset as isize + 2 + delta as isize) as usize;
                assert!(
                    (function_start..function_end).contains(&target),
                    "branch at {} escapes choose to {}",
                    offset - 1,
                    target
                );
                offset += 2;
            }
            Some(Opcode::LoadConst)
            | Some(Opcode::LoadLocal)
            | Some(Opcode::StoreLocal)
            | Some(Opcode::GetField)
            | Some(Opcode::SetField)
            | Some(Opcode::ArrayGet)
            | Some(Opcode::ArraySet) => offset += 2,
            Some(Opcode::Call)
            | Some(Opcode::TailCall)
            | Some(Opcode::Spawn)
            | Some(Opcode::MakeClosure)
            | Some(Opcode::TypeIs)
            | Some(Opcode::Cast) => {
                if opcode == Opcode::TailCall as u8 {
                    saw_tail_call = true;
                }
                offset += 1;
            }
            Some(Opcode::Select) => panic!("select is not part of this fixture"),
            Some(Opcode::Return) => saw_return = true,
            Some(_) => {}
            None => panic!("unknown opcode {opcode:#x} at {}", offset - 1),
        }
    }
    assert_eq!(
        offset, function_end,
        "function bytecode should disassemble cleanly"
    );
    assert!(saw_return, "choose must have a return boundary");
    assert!(
        !saw_tail_call,
        "nested branch calls must not be tail-call rewritten"
    );
}

#[test]
fn incompatible_typed_if_branches_are_rejected() {
    let source = r#"
fn choose_bad(flag: bool) -> int {
    return if flag { 1 } else { "one" }
}
"#;
    let error = lirac::check(source).expect_err("incompatible branches must be rejected");
    assert!(
        error.contains("If expression branches have incompatible types"),
        "unexpected diagnostic: {error}"
    );
}
