//! Native spawn call-shape coverage.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-spawn-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn run_vm(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let bytecode = lirac::compile_with_imports(path.to_str().ok_or("non-utf8 path")?, source)?;
        let (status, lines) = liravm::run_with_capture(&bytecode)?;
        if status != 0 {
            return Err(format!("VM status {status}"));
        }
        Ok(lines.join("\n"))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_aot(source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let result = (|| {
        let output = common::run_aot(&path, source).map_err(|error| format!("run AOT: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "AOT status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_jit(source: &str) -> Result<String, String> {
    let (status, stdout) = common::run_jit_capture("native-spawn.li", source)?;
    if status != 0 {
        return Err(format!("JIT status {status}"));
    }
    Ok(String::from_utf8_lossy(&stdout).trim_end().to_string())
}

fn assert_vm_aot_jit(source: &str, expected: &str) {
    assert_eq!(run_vm(source).expect("VM run"), expected);
    assert_eq!(run_aot(source).expect("AOT run"), expected);
    assert_eq!(run_jit(source).expect("JIT run"), expected);
}

#[test]
fn spawn_handle_renders_as_an_opaque_fiber() {
    assert_vm_aot_jit(
        r#"
        fn worker() { }
        fn main() {
            let handle = spawn worker()
            println(handle)
        }
        "#,
        "<fiber>",
    );
}

#[test]
fn direct_spawn_preserves_channel_effects() {
    assert_vm_aot_jit(
        r#"
        fn worker(ch: Channel<int>, value: int) -> int {
            send(ch, value)
            return 99
        }
        fn main() {
            let ch: Channel<int> = chan(1)
            spawn worker(ch, 7)
            let got = recv(ch)
            if got != 7 { println(1 / 0) }
            println(got)
        }
        "#,
        "7",
    );
}

#[test]
fn direct_spawn_binds_named_and_default_arguments() {
    assert_vm_aot_jit(
        r#"
        fn worker(ch: Channel<int>, first: int, second: int = 4) { send(ch, first * 10 + second) }
        fn main() {
            let ch: Channel<int> = chan(2)
            spawn worker(ch, second: 8, first: 3)
            spawn worker(ch, first: 4)
            let first = recv(ch)
            let second = recv(ch)
            if first != 38 || second != 44 { println(1 / 0) }
            println(first)
            println(second)
        }
        "#,
        "38\n44",
    );
}

#[test]
fn generic_direct_spawn_is_monomorphized_before_thunking() {
    let source = r#"
        fn worker<T>(ch: Channel<T>, value: T) { send(ch, value) }
        fn main() {
            let ch: Channel<int> = chan(1)
            spawn worker(ch, 12)
            let got = recv(ch)
            if got != 12 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "12");
}

#[test]
fn generic_static_method_spawn_is_monomorphized_before_thunking() {
    let source = r#"
        struct Factory {}
        impl Factory {
            fn emit<T>(ch: Channel<T>, value: T) { send(ch, value) }
        }
        fn main() {
            let ch: Channel<int> = chan(1)
            spawn Factory.emit::<int>(ch, 13)
            let got = recv(ch)
            if got != 13 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "13");
}

#[test]
fn static_method_spawn_keeps_the_first_explicit_argument_type() {
    let source = r#"
        interface Job { fn value() -> int }
        struct Work { n: int }
        impl Work { fn value(self) -> int { return self.n } }
        struct Factory {}
        impl Factory {
            fn emit(job: Job, ch: Channel<int>) { send(ch, job.value()) }
        }
        fn main() {
            let ch: Channel<int> = chan(1)
            spawn Factory.emit(Work { n: 17 }, ch)
            println(recv(ch))
        }
        "#;
    assert_vm_aot_jit(source, "17");
}

#[test]
fn function_value_spawn_preserves_captured_callable() {
    let source = r#"
        fn worker(ch: Channel<int>, value: int) { send(ch, value) }
        fn main() {
            let ch: Channel<int> = chan(1)
            let f = worker
            spawn f(ch, 9)
            let got = recv(ch)
            if got != 9 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "9");
}

#[test]
fn capturing_lambda_spawn_preserves_environment() {
    let source = r#"
        fn main() {
            let ch: Channel<int> = chan(1)
            let offset: int = 5
            let worker = |value: int| { send(ch, value + offset) }
            spawn worker(6)
            let got = recv(ch)
            if got != 11 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "11");
}

#[test]
fn method_spawns_cover_instance_and_static_dispatch() {
    let source = r#"
        struct Worker { value: int }
        impl Worker {
            fn emit(self, ch: Channel<int>, extra: int = 2) { send(ch, self.value + extra) }
            fn static_emit(ch: Channel<int>, value: int) { send(ch, value) }
        }
        fn main() {
            let ch: Channel<int> = chan(2)
            let worker = Worker { value: 3 }
            spawn worker.emit(ch, extra: 4)
            spawn Worker.static_emit(ch, 8)
            let first = recv(ch)
            let second = recv(ch)
            if first != 7 || second != 8 { println(1 / 0) }
            println(first)
            println(second)
        }
        "#;
    assert_vm_aot_jit(source, "7\n8");
}

#[test]
fn instance_method_spawn_stages_named_and_default_arguments() {
    let source = r#"
        struct Worker {}
        impl Worker {
            fn emit(self, ch: Channel<int>, first: int, second: int = 4) {
                send(ch, first * 10 + second)
            }
        }
        fn main() {
            let ch: Channel<int> = chan(2)
            let worker = Worker {}
            spawn worker.emit(second: 8, ch: ch, first: 3)
            spawn worker.emit(first: 4, ch: ch)
            println(recv(ch))
            println(recv(ch))
        }
        "#;
    assert_vm_aot_jit(source, "38\n44");
}

#[test]
fn spawn_preserves_value_copy_and_reference_identity_boundaries() {
    let source = r#"
        struct ValueBox { value: int }
        fn worker(ch: Channel<int>, value: ValueBox, reference: [int]) {
            send(ch, value.value + reference[0])
        }
        fn main() {
            let value = ValueBox { value: 2 }
            let reference = [3]
            let ch: Channel<int> = chan(1)
            spawn worker(ch, value, reference)
            value.value = 20
            reference[0] = 30
            let got = recv(ch)
            if got != 32 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "32");
}

#[test]
fn class_instance_spawn_uses_virtual_dispatch() {
    let source = r#"
        class Animal {
            fn emit(self, ch: Channel<int>) { send(ch, 1) }
        }
        class Dog extends Animal {
            override fn emit(self, ch: Channel<int>) { send(ch, 2) }
        }
        fn main() {
            let ch: Channel<int> = chan(1)
            let dog = Dog {}
            spawn dog.emit(ch)
            let got = recv(ch)
            if got != 2 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "2");
}

#[test]
fn spawn_operand_is_evaluated_once() {
    let source = r#"
        var calls: int = 0
        fn make(ch: Channel<int>) -> Channel<int> {
            calls = calls + 1
            return ch
        }
        fn worker(ch: Channel<int>) { send(ch, calls) }
        fn main() {
            let ch: Channel<int> = chan(1)
            spawn worker(make(ch))
            let got = recv(ch)
            if got != 1 || calls != 1 { println(1 / 0) }
            println(got)
            println(calls)
        }
        "#;
    assert_vm_aot_jit(source, "1\n1");
}

#[test]
fn spawn_block_runs_in_child_and_discards_value() {
    let source = r#"
        fn main() {
            let ch: Channel<int> = chan(1)
            spawn {
                send(ch, 17)
                99
            }
            let got = recv(ch)
            if got != 17 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "17");
}

#[test]
fn spawn_builtin_send_evaluates_operands_once() {
    let source = r#"
        var calls: int = 0
        fn make() -> int {
            calls = calls + 1
            return 23
        }
        fn main() {
            let ch: Channel<int> = chan(1)
            spawn send(ch, make())
            let got = recv(ch)
            if got != 23 || calls != 1 { println(1 / 0) }
            println(got)
            println(calls)
        }
        "#;
    assert_vm_aot_jit(source, "23\n1");
}

#[test]
fn spawn_builtin_send_preserves_struct_copy_and_reference_identity() {
    let source = r#"
        struct ValueBox { value: int }
        fn main() {
            let value_ch: Channel<ValueBox> = chan(1)
            let ref_ch: Channel<[int]> = chan(1)
            let value = ValueBox { value: 2 }
            let reference = [3]
            spawn send(value_ch, value)
            spawn send(ref_ch, reference)
            value.value = 20
            reference[0] = 30
            let copied = recv(value_ch)
            let aliased = recv(ref_ch)
            if copied.value != 2 || aliased[0] != 30 { println(1 / 0) }
            println(copied.value)
            println(aliased[0])
        }
        "#;
    assert_vm_aot_jit(source, "2\n30");
}

#[test]
fn spawn_block_captures_immutable_and_nested_shadowed_locals() {
    let source = r#"
        fn main() {
            let ch: Channel<int> = chan(1)
            let captured: int = 4
            spawn {
                let shadowed: int = 6
                {
                    send(ch, captured + shadowed)
                    1000
                }
            }
            let got = recv(ch)
            if got != 10 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "10");
}

#[test]
fn spawn_block_select_receive_binder_shadows_outer_name() {
    let source = r#"
        fn main() {
            let input: Channel<int> = chan(1)
            let output: Channel<int> = chan(1)
            let value: int = 99
            send(input, 7)
            spawn {
                select {
                    value = <-input => send(output, value)
                }
            }
            let got = recv(output)
            if got != 7 { println(1 / 0) }
            println(got)
        }
        "#;
    assert_vm_aot_jit(source, "7");
}

#[test]
fn negative_spawn_arity_is_rejected_at_compiler_boundary() {
    let source = r#"
        fn worker(value: int) { println(value) }
        fn main() { spawn worker() }
    "#;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("program.li");
    std::fs::write(&path, source).expect("source");
    let error = common::build_aot(&path, source).expect_err("invalid spawn must be rejected");
    assert!(
        error.contains("Expected at least") || error.contains("arguments"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn negative_spawn_type_mismatch_is_rejected_at_compiler_boundary() {
    let source = r#"
        fn worker(value: int) { println(value) }
        fn main() { spawn worker("wrong") }
    "#;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("program.li");
    std::fs::write(&path, source).expect("source");
    let error = common::build_aot(&path, source).expect_err("invalid spawn must be rejected");
    assert!(error.contains("Argument type mismatch"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
