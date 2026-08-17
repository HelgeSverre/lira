//! Deterministic, compiled-source coverage for VM `select` arbitration.
//!
//! These tests configure the VM seed directly so fairness claims do not rely
//! on statistical timing or scheduler order.

fn run_source_with_seed(name: &str, source: &str, seed: u64) -> Vec<String> {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    let program =
        liravm::bytecode::load(&bytecode).unwrap_or_else(|error| panic!("load {name}: {error}"));
    let mut vm = liravm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);
    vm.set_select_seed(seed);
    vm.run()
        .unwrap_or_else(|error| panic!("run {name}: {error}"));
    vm.get_output().to_vec()
}

fn run_source_error(name: &str, source: &str) -> String {
    let bytecode = lirac::compile_with_imports(name, source)
        .unwrap_or_else(|error| panic!("compile {name}: {error}"));
    let program =
        liravm::bytecode::load(&bytecode).unwrap_or_else(|error| panic!("load {name}: {error}"));
    let mut vm = liravm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);
    vm.run().expect_err("source should deadlock")
}

#[test]
fn ready_recv_arms_are_seeded_and_source_order_independent() {
    let source = r#"
let a = chan(1)
let b = chan(1)
send(a, 11)
send(b, 22)
select {
    x = <-a => println("a=" + x)
    y = <-b => println("b=" + y)
}
"#;

    let seed_one = run_source_with_seed("select_seed_one", source, 1);
    let seed_two = run_source_with_seed("select_seed_two", source, 2);
    assert_ne!(
        seed_one, seed_two,
        "distinct seeds must choose distinct ready arms"
    );

    let reversed = r#"
let a = chan(1)
let b = chan(1)
send(a, 11)
send(b, 22)
select {
    y = <-b => println("b=" + y)
    x = <-a => println("a=" + x)
}
    "#;
    let reversed_seed_one = run_source_with_seed("select_reversed", reversed, 1);
    assert!(seed_one == ["a=11"] || seed_one == ["b=22"]);
    assert!(reversed_seed_one == ["a=11"] || reversed_seed_one == ["b=22"]);
}

#[test]
fn mixed_ready_recv_and_send_commits_exactly_one_arm() {
    let source = r#"
let recv_ch = chan(1)
let send_ch = chan()
send(recv_ch, 7)
fn receiver(c: Channel<int>) {
    select { value = <-c => println("received=" + value) }
}
spawn receiver(send_ch)
fiber_yield()
select {
    x = <-recv_ch => println("recv=" + x)
    9 -> send_ch => println("send")
}
fiber_yield()
"#;

    let output = run_source_with_seed("select_mixed_ready", source, 1);
    assert!(output == ["recv=7"] || output == ["send", "received=9"]);
}

#[test]
fn default_runs_only_when_no_communication_arm_is_ready() {
    let source = r#"
let ch = chan(1)
select {
    <-ch => println("recv")
    _ => println("default")
}
"#;
    assert_eq!(
        run_source_with_seed("select_default_priority", source, 9),
        vec!["default"],
    );
}

#[test]
fn same_channel_send_receive_with_default_does_not_self_rendezvous() {
    let source = r#"
let ch = chan()
select {
    value = <-ch => println("recv=" + value)
    1 -> ch => println("send")
    _ => println("default")
}
"#;
    assert_eq!(
        run_source_with_seed("same_channel_default", source, 1),
        vec!["default"],
    );
}

#[test]
fn same_channel_send_receive_without_default_deadlocks_without_livelock() {
    let source = r#"
let ch = chan()
select {
    value = <-ch => println("recv=" + value)
    1 -> ch => println("send")
}
    "#;
    assert!(run_source_error("same_channel_deadlock", source).contains("deadlock"));
}

#[test]
fn closed_receive_is_ready_and_returns_null() {
    let source = r#"
let ch = chan(1)
close(ch)
select {
    value = <-ch => println(value == null)
    _ => println(false)
}
"#;
    assert_eq!(
        run_source_with_seed("select_closed_receive", source, 5),
        vec!["true"],
    );
}

#[test]
fn closing_a_channel_does_not_fail_a_parked_select_send_arm() {
    let source = r#"
let ch = chan()
fn closer(c: Channel<int>) { close(c) }
spawn closer(ch)
select {
    value = <-ch => println(value)
    1 -> ch => println("sent")
}
"#;
    assert_eq!(
        run_source_with_seed("select_closed_send", source, 13),
        vec!["null"],
    );
}

#[test]
fn cross_select_parked_sender_arbitrates_before_rendezvous() {
    let source = r#"
let a = chan()
let b = chan(1)
fn parked_sender(x: Channel<int>, y: Channel<int>) {
    select {
        1 -> x => println("sender=a")
        value = <-y => println("sender=b=" + value)
    }
}
fn fill(y: Channel<int>) { send(y, 7) }
fn fallback(x: Channel<int>) {
    fiber_yield()
    send(x, 9)
}
spawn parked_sender(a, b)
fiber_yield()
spawn fill(b)
fiber_yield()
spawn fallback(a)
select { value = <-a => println("receiver=" + value) }
fiber_yield()
"#;

    let first = run_source_with_seed("cross_select_sender_one", source, 1);
    let second = run_source_with_seed("cross_select_sender_two", source, 2);
    assert_ne!(first, second);
    assert!(first
        .iter()
        .any(|line| line == "sender=a" || line == "sender=b=7"));
    assert!(second
        .iter()
        .any(|line| line == "sender=a" || line == "sender=b=7"));
}

#[test]
fn cross_select_parked_receiver_arbitrates_before_rendezvous() {
    let source = r#"
let a = chan()
let b = chan()
fn parked_receiver(x: Channel<int>, y: Channel<int>) {
    select {
        value = <-x => println("receiver=a=" + value)
        1 -> y => println("receiver=b")
    }
}
fn helper(y: Channel<int>) {
    select { value = <-y => println("helper=" + value) }
}
fn fallback(x: Channel<int>) {
    fiber_yield()
    recv(x)
}
spawn parked_receiver(a, b)
fiber_yield()
spawn helper(b)
fiber_yield()
spawn fallback(a)
select { 9 -> a => println("sender=a") }
fiber_yield()
"#;

    assert_eq!(
        run_source_with_seed("cross_select_receiver", source, 1),
        vec!["helper=1", "receiver=b", "sender=a"],
    );
}

#[test]
fn duplicate_same_channel_arms_use_seeded_tie_breaking() {
    let source = r#"
let ch = chan(1)
send(ch, 4)
select {
    first = <-ch => println("first=" + first)
    second = <-ch => println("second=" + second)
}
"#;
    let first = run_source_with_seed("select_duplicate_one", source, 1);
    let second = run_source_with_seed("select_duplicate_two", source, 2);
    assert_ne!(first, second);
    assert!(first == ["first=4"] || first == ["second=4"]);
    assert!(second == ["first=4"] || second == ["second=4"]);
}

#[test]
fn parked_duplicate_receive_arms_choose_body_with_seed() {
    let source = r#"
let ch = chan()
fn parked(c: Channel<int>) {
    select {
        first = <-c => println("first=" + first)
        second = <-c => println("second=" + second)
    }
}
spawn parked(ch)
fiber_yield()
select { 7 -> ch => println("sender done") }
fiber_yield()
"#;
    let first = run_source_with_seed("parked_duplicate_recv_one", source, 1);
    let second = run_source_with_seed("parked_duplicate_recv_two", source, 2);
    assert_ne!(first, second);
    assert!(first == ["sender done", "first=7"] || first == ["sender done", "second=7"]);
    assert!(second == ["sender done", "first=7"] || second == ["sender done", "second=7"]);
}

#[test]
fn parked_duplicate_send_arms_choose_body_with_seed() {
    let source = r#"
let ch = chan()
fn parked(c: Channel<int>) {
    select {
        7 -> c => println("first")
        7 -> c => println("second")
    }
}
spawn parked(ch)
fiber_yield()
select { value = <-ch => println("receiver=" + value) }
fiber_yield()
"#;
    let first = run_source_with_seed("parked_duplicate_send_one", source, 7);
    let second = run_source_with_seed("parked_duplicate_send_two", source, 8);
    assert_ne!(first, second);
    assert!(
        first == ["receiver=7", "first"]
            || first == ["receiver=7", "second"]
            || first == ["first", "receiver=7"]
            || first == ["second", "receiver=7"]
    );
    assert!(
        second == ["receiver=7", "first"]
            || second == ["receiver=7", "second"]
            || second == ["first", "receiver=7"]
            || second == ["second", "receiver=7"]
    );
}

#[test]
fn parked_duplicate_send_values_match_seeded_bodies() {
    let source = r#"
let ch = chan()
fn parked(c: Channel<int>) {
    select {
        11 -> c => println("first")
        22 -> c => println("second")
    }
}
spawn parked(ch)
fiber_yield()
select { value = <-ch => println("receiver=" + value) }
fiber_yield()
"#;
    let first = run_source_with_seed("parked_duplicate_values_one", source, 7);
    let second = run_source_with_seed("parked_duplicate_values_two", source, 8);
    assert!(first == ["receiver=11", "first"] || first == ["first", "receiver=11"]);
    assert!(second == ["receiver=22", "second"] || second == ["second", "receiver=22"]);
}

#[test]
fn parked_same_channel_opposite_arms_do_not_self_ready() {
    let source = r#"
let ch = chan()
fn parked(c: Channel<int>) {
    select {
        1 -> c => println("parked send")
        value = <-c => println("parked recv=" + value)
    }
}
spawn parked(ch)
fiber_yield()
select { 7 -> ch => println("active send") }
fiber_yield()
"#;
    assert_eq!(
        run_source_with_seed("parked_self_readiness", source, 1),
        vec!["active send", "parked recv=7"],
    );
}

#[test]
fn ordinary_sender_behind_deferred_select_sender_still_progresses() {
    let source = r#"
let a = chan()
let b = chan(1)
fn parked(c: Channel<int>, alternate: Channel<int>) {
    select {
        1 -> c => println("select sender")
        value = <-alternate => println("select alternate=" + value)
    }
}
fn ordinary(c: Channel<int>) {
    send(c, 2)
    println("ordinary done")
}
spawn parked(a, b)
fiber_yield()
send(b, 9)
spawn ordinary(a)
fiber_yield()
println(recv(a))
fiber_yield()
"#;
    assert_eq!(
        run_source_with_seed("ordinary_sender_after_select", source, 1),
        vec!["select alternate=9", "2", "ordinary done"],
    );
}

#[test]
fn ordinary_sender_refills_buffer_behind_deferred_select_sender() {
    let source = r#"
let a = chan(1)
let b = chan(1)
send(a, 1)
fn parked(c: Channel<int>, alternate: Channel<int>) {
    select {
        3 -> c => println("select sender")
        value = <-alternate => println("select alternate=" + value)
    }
}

fn ordinary(c: Channel<int>) {
    send(c, 2)
    println("ordinary done")
}
spawn parked(a, b)
fiber_yield()
send(b, 9)
spawn ordinary(a)
fiber_yield()
println(recv(a))
println(recv(a))
fiber_yield()
"#;
    assert_eq!(
        run_source_with_seed("ordinary_buffer_refill_after_select", source, 1),
        vec!["select alternate=9", "1", "2", "ordinary done"],
    );
}

#[test]
fn deferred_parked_sender_withdrawal_wakes_active_default() {
    let source = r#"
let a = chan()
let b = chan(1)
fn parked(c: Channel<int>, alternate: Channel<int>) {
    select {
        1 -> c => println("parked sender")
        value = <-alternate => println("parked alternate=" + value)
    }
}
spawn parked(a, b)
fiber_yield()
send(b, 9)
select {
    value = <-a => println("active recv=" + value)
    _ => println("active default")
}
fiber_yield()
"#;
    assert_eq!(
        run_source_with_seed("deferred_sender_default", source, 2),
        vec!["parked alternate=9", "active default"],
    );
}

#[test]
fn deferred_parked_receiver_withdrawal_wakes_active_default() {
    let source = r#"
let a = chan()
let b = chan()
fn parked(c: Channel<int>, alternate: Channel<int>) {
    select {
        value = <-c => println("parked receiver=" + value)
        1 -> alternate => println("parked alternate send")
    }
}
fn ordinary_receiver(c: Channel<int>) {
    println("ordinary=" + recv(c))
}
spawn parked(a, b)
fiber_yield()
spawn ordinary_receiver(b)
fiber_yield()
select {
    7 -> a => println("active send")
    _ => println("active default")
}
fiber_yield()
"#;
    assert_eq!(
        run_source_with_seed("deferred_receiver_default", source, 3),
        vec!["parked alternate send", "ordinary=1", "active default"],
    );
}
