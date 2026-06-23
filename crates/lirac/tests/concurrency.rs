//! Integration tests for fiber/channel concurrency.
//!
//! These compile real Lira source via `compile_with_imports` and execute it
//! through `run_with_capture` (the same harness the example tests use), then
//! assert on captured output. The VM now drives the fiber scheduler, so a
//! spawned worker actually runs: it receives its arguments, can yield
//! cooperatively, and can hand values to other fibers over channels.
//!
//! Note: these programs deliberately do NOT call `main()` themselves. Codegen
//! auto-invokes a zero-arg `main`, so an explicit call would run the program
//! twice.

/// Compile and run a Lira source string, returning captured output lines.
fn run_source(name: &str, source: &str) -> Vec<String> {
    let bytecode =
        lirac::compile_with_imports(name, source).unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let (_code, output) =
        liravm::run_with_capture(&bytecode).unwrap_or_else(|e| panic!("run {name}: {e}"));
    output
}

/// Two spawned workers and main interleave via cooperative `fiber_yield`.
/// Cooperative round-robin scheduling makes the transcript deterministic, and
/// this also proves spawn ARGUMENTS reach the worker (each worker prints its
/// id, passed as a spawn arg).
#[test]
fn spawn_args_and_round_robin_is_deterministic() {
    let source = r#"
fn worker(id: int, n: int) {
    var i = 0
    while i < n {
        println("w" + id + ":" + i)
        fiber_yield()
        i = i + 1
    }
}

fn main() {
    spawn worker(1, 3)
    spawn worker(2, 3)
    var t = 0
    while t < 10 {
        fiber_yield()
        t = t + 1
    }
    println("main done")
}
"#;
    let output = run_source("spawn_args_round_robin", source);
    assert_eq!(
        output,
        vec![
            "w1:0".to_string(),
            "w2:0".to_string(),
            "w1:1".to_string(),
            "w2:1".to_string(),
            "w1:2".to_string(),
            "w2:2".to_string(),
            "main done".to_string(),
        ]
    );
}

/// A spawned worker receives a channel as a spawn argument and sends a value
/// over it; main yields, lets the worker run, then receives. The cross-fiber
/// channel rendezvous (`recv` returns ok = true) proves the worker actually
/// executed and the scheduler delivered the message back to main.
#[test]
fn spawn_worker_channel_handoff() {
    let source = r#"
fn worker(ch: Channel<int>) {
    send(ch, 42)
    println("worker sent")
}

fn main() {
    let ch = chan(1)
    spawn worker(ch)
    fiber_yield()
    let ok = recv(ch)
    println("recv ok: " + ok)
}
"#;
    let output = run_source("spawn_worker_channel_handoff", source);
    assert_eq!(
        output,
        vec!["worker sent".to_string(), "recv ok: true".to_string()]
    );
}

/// Several workers each send a value into a shared buffered channel; main lets
/// them all run, then drains the channel. Which worker sends first depends on
/// scheduling, so we assert the order-independent fact that exactly four
/// rendezvous complete (main receives four times without deadlocking).
#[test]
fn fan_in_drains_all_sends() {
    let source = r#"
fn worker(ch: Channel<int>, value: int) {
    send(ch, value)
    println("sent")
}

fn main() {
    let ch = chan(4)
    spawn worker(ch, 10)
    spawn worker(ch, 20)
    spawn worker(ch, 30)
    spawn worker(ch, 40)

    // Let every worker run and fill the buffered channel.
    var t = 0
    while t < 8 {
        fiber_yield()
        t = t + 1
    }

    // Drain exactly four buffered values.
    var received = 0
    while received < 4 {
        let ok = recv(ch)
        received = received + 1
    }
    println("received " + received)
}
"#;
    let output = run_source("fan_in_drains_all_sends", source);
    let sent = output.iter().filter(|l| *l == "sent").count();
    assert_eq!(sent, 4, "all four workers should send: {output:?}");
    assert!(
        output.iter().any(|l| l == "received 4"),
        "main should drain four values: {output:?}"
    );
}
