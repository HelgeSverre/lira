//! Compiled-source coverage for value-producing native `select` expressions.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

fn source_path(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-select-values-{}-{}-{}.li",
        std::process::id(),
        label,
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn select_values_match_vm_aot_and_jit() {
    let source = r#"
fn returning_select() -> int {
    let channel: Channel<int> = chan(1)
    send(channel, 42)
    return select {
        received = <-channel => { return received }
        _ => 7
    }
}
println(returning_select())

fn break_select() -> int {
    let channel: Channel<int> = chan(1)
    send(channel, 1)
    loop {
        select {
            received = <-channel => { break }
            _ => 0
        }
    }
    return 5
}
println(break_select())

fn continue_select() -> int {
    let channel: Channel<int> = chan(1)
    send(channel, 1)
    while true {
        select {
            received = <-channel => { continue }
            _ => { break }
        }
    }
    return 7
}
println(continue_select())

let ints = chan(1)
send(ints, 42)
let int_value = select { value = <-ints => value }
println(int_value)

let strings: Channel<string> = chan(1)
send(strings, "ready")
let string_value = select { value = <-strings => value }
println(string_value)

let dynamic: Channel<any> = chan(1)
send(dynamic, 7)
let dynamic_value = select {
    value = <-dynamic => value
    _ => "fallback"
}
println(dynamic_value)

let sent: Channel<int> = chan(1)
let send_value = select { 5 -> sent => 13 }
println(send_value)
println(recv(sent))

let empty: Channel<int> = chan(1)
let default_value = select {
    value = <-empty => value
    _ => 9
}
println(default_value)

let blocked: Channel<int> = chan()
fn producer(channel: Channel<int>) { send(channel, 11) }
spawn producer(blocked)
let blocked_value = select { value = <-blocked => value }
println(blocked_value)
"#;
    let path = source_path("all");
    std::fs::write(&path, source).expect("write select source");

    let bytecode = lirac::compile_with_imports(path.to_str().expect("utf-8 path"), source)
        .expect("select values compile for the VM");
    let (vm_status, vm_lines) =
        liravm::run_with_capture(&bytecode).expect("run select values on VM");
    assert_eq!(vm_status, 0);
    assert_eq!(
        vm_lines,
        ["42", "5", "7", "42", "ready", "7", "13", "5", "9", "11"]
    );

    let native = common::run_aot(&path, source).expect("run select values through AOT");
    native
        .assert_complete_output()
        .expect("AOT output is bounded");
    assert!(
        native.status.success(),
        "AOT failed: {}",
        native.stderr_text()
    );
    assert_eq!(
        native.stdout_text(),
        "42\n5\n7\n42\nready\n7\n13\n5\n9\n11\n"
    );

    let (jit_status, jit_output) =
        common::run_jit_capture(path.to_str().expect("utf-8 path"), source)
            .expect("run select values through JIT");
    assert_eq!(jit_status, 0);
    assert_eq!(
        String::from_utf8_lossy(&jit_output),
        "42\n5\n7\n42\nready\n7\n13\n5\n9\n11\n"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn select_value_mismatch_is_rejected_in_both_arm_orders() {
    let cases = [
        r#"
let channel = chan(1)
send(channel, 1)
let value = select {
    received = <-channel => received
    _ => "wrong"
}
"#,
        r#"
let channel = chan(1)
send(channel, 1)
        let value = select {
            _ => "wrong"
            received = <-channel => received
        }
"#,
        r#"
let channel = chan(1)
send(channel, 1)
let value = select {
    received = <-channel => received
    _ => println("void")
}
"#,
        r#"
let channel = chan(1)
send(channel, 1)
let value = select {
    _ => println("void")
    received = <-channel => received
}
"#,
    ];

    for source in cases {
        let error = lirac::check(source).expect_err("incompatible select arms must fail");
        assert!(
            error.contains("Select arm type mismatch")
                && error.contains("int")
                && (error.contains("string") || error.contains("void")),
            "unexpected select diagnostic: {error}"
        );
    }
}
