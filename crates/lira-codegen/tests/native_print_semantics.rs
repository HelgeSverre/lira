//! Bounded AOT/JIT parity for core `print` and `println`.

use std::path::Path;

mod common;

const SOURCE: &str = r#"
print("a")
print(2)
println("c")
print("left\n")
println("right")
"#;

const EXPECTED: &[u8] = b"a2c\nleft\nright\n";

const AGGREGATE_SOURCE: &str = r#"
struct Record {
    z: int
    a: string
}

struct UnicodeRecord {
    å: int
    β: string
}

enum Choice {
    Empty,
    One(int),
    Pair(int, string)
}

fn ok() -> Result<int, string> { return Result::Ok(5) }
fn err() -> Result<int, string> { return Result::Err("bad") }
fn maybe_record(present: bool) -> Record? {
    if present {
        return Record { z: 3, a: "yes" }
    }
    return null
}

println([3, 1])
println((7, "x"))
println((9,))
println({ "z": 3, "a": 4 })
println(Record { z: 3, a: "yes" })
println(UnicodeRecord { å: 6, β: "ok" })
println(Choice::Empty)
println(Choice::One(8))
println(Choice::Pair(2, "two"))
println(ok())
println(err())
println(maybe_record(true))
println(maybe_record(false))
"#;

const AGGREGATE_EXPECTED: &str = "[3, 1]\n\
(7, x)\n\
(9,)\n\
{a: 4, z: 3}\n\
{a: yes, z: 3}\n\
{å: 6, β: ok}\n\
{__enum: Choice, __variant: Empty}\n\
{__data: 8, __enum: Choice, __variant: One}\n\
{__data: [2, two], __enum: Choice, __variant: Pair}\n\
{__data: 5, __enum: Result, __variant: Ok}\n\
{__data: bad, __enum: Result, __variant: Err}\n\
{a: yes, z: 3}\n\
null";

const OPAQUE_HANDLE_SOURCE: &str = r#"
fn double(value: int) -> int { return value * 2 }

let amount = 3
let closure = |value: int| value + amount
let channel: Channel<int> = chan(1)
println(double)
println(closure)
println(channel)
close(channel)
"#;

const OPAQUE_HANDLE_EXPECTED: &str = "<function>\n<function>\n<channel>";

const RECURSIVE_AGGREGATE_SOURCE: &str = r#"
struct Node {
    next: Node?
    value: int
}

let n9 = Node { next: null, value: 9 }
let n8 = Node { next: n9, value: 8 }
let n7 = Node { next: n8, value: 7 }
let n6 = Node { next: n7, value: 6 }
let n5 = Node { next: n6, value: 5 }
let n4 = Node { next: n5, value: 4 }
let n3 = Node { next: n4, value: 3 }
let n2 = Node { next: n3, value: 2 }
let n1 = Node { next: n2, value: 1 }
let n0 = Node { next: n1, value: 0 }
println(n0)
"#;

const RECURSIVE_AGGREGATE_EXPECTED: &str = "{next: {next: {next: {next: {next: {next: {next: {next: {...}, value: 7}, value: 6}, value: 5}, value: 4}, value: 3}, value: 2}, value: 1}, value: 0}";

// The untyped parameter forces both backends through their dynamic `Any`
// aggregate renderers. It covers the depth boundary independently of nominal
// recursive structs, keeps the UTF-8 field-name descriptor path live, and
// verifies that the built-in `Map<K, V>` annotation remains a native map
// rather than being mistaken for a user-defined generic aggregate.
const DYNAMIC_NESTED_AGGREGATE_SOURCE: &str = r#"
struct UnicodeRecord {
    å: int
    β: string
}

fn render(value) {
    println(value)
}

let nested = [[[[[[[[[0]]]]]]]]]
let typed_map: Map<string, int> = { "z": 3, "a": 4 }
render(nested)
render(UnicodeRecord { å: 6, β: "ok" })
println(len(typed_map))
println(typed_map)
render(typed_map)
"#;

const DYNAMIC_NESTED_AGGREGATE_EXPECTED: &str =
    "[[[[[[[[[...]]]]]]]]]\n{å: 6, β: ok}\n2\n{a: 4, z: 3}\n{a: 4, z: 3}";

// Keep the map-to-Any call boundary isolated from the other aggregate cases:
// direct rendering proves the typed map is populated before either call, and
// the two functions cover both inferred and explicit dynamic parameters.
const TYPED_MAP_ANY_CALL_SOURCE: &str = r#"
fn render(value) {
    println(value)
}

fn render_explicit(value: any) {
    println(value)
}

let values: Map<string, int> = { "z": 3, "a": 4 }
println(len(values))
println(values)
render(values)
render_explicit(values)
"#;

const TYPED_MAP_ANY_CALL_EXPECTED: &str = "2\n{a: 4, z: 3}\n{a: 4, z: 3}\n{a: 4, z: 3}";

const OVERSIZED_RENDER_SOURCE: &str = r#"
var values = ["0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"]
var count = 1
while count < 32514 {
    push(values, "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
    count = count + 1
}
println(values)
"#;

const CYCLIC_AGGREGATE_SOURCE: &str = r#"
fn render(value) { return value as string }
var node = json_parse("[0]")
push(node, node)
println(render(node))
"#;

fn run_vm(source: &str) -> Result<String, String> {
    let bytecode = lirac::compile(source)?;
    let (status, lines) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM exited with status {status}"));
    }
    Ok(lines.join("\n"))
}

#[test]
fn native_print_and_println_preserve_exact_stream_boundaries() {
    let aot = common::run_aot(Path::new("native_print_semantics.li"), SOURCE)
        .expect("bounded AOT output source runs");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(aot.stdout, EXPECTED);

    let (jit_status, jit_stdout) = common::run_jit_capture("native_print_semantics.li", SOURCE)
        .expect("bounded JIT output source runs");
    assert_eq!(jit_status, 0);
    assert_eq!(jit_stdout, EXPECTED);
}

#[test]
fn aggregate_printing_matches_the_vm_exactly() {
    assert_eq!(
        run_vm(AGGREGATE_SOURCE).expect("VM aggregate print source runs"),
        AGGREGATE_EXPECTED
    );

    let aot = common::run_aot(
        Path::new("native_aggregate_print_semantics.li"),
        AGGREGATE_SOURCE,
    )
    .expect("bounded AOT aggregate print source runs");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(aot.stdout_text().trim_end(), AGGREGATE_EXPECTED);

    let (jit_status, jit_stdout) =
        common::run_jit_capture("native_aggregate_print_semantics.li", AGGREGATE_SOURCE)
            .expect("bounded JIT aggregate print source runs");
    assert_eq!(jit_status, 0);
    assert_eq!(
        String::from_utf8_lossy(&jit_stdout).trim_end(),
        AGGREGATE_EXPECTED
    );
}

#[test]
fn opaque_function_closure_and_channel_printing_is_backend_independent() {
    assert_eq!(
        run_vm(OPAQUE_HANDLE_SOURCE).expect("VM opaque-handle print source runs"),
        OPAQUE_HANDLE_EXPECTED
    );

    let aot = common::run_aot(
        Path::new("native_opaque_handle_print_semantics.li"),
        OPAQUE_HANDLE_SOURCE,
    )
    .expect("bounded AOT opaque-handle print source runs");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(aot.stdout_text().trim_end(), OPAQUE_HANDLE_EXPECTED);

    let (jit_status, jit_stdout) = common::run_jit_capture(
        "native_opaque_handle_print_semantics.li",
        OPAQUE_HANDLE_SOURCE,
    )
    .expect("bounded JIT opaque-handle print source runs");
    assert_eq!(jit_status, 0);
    assert_eq!(
        String::from_utf8_lossy(&jit_stdout).trim_end(),
        OPAQUE_HANDLE_EXPECTED
    );
}

#[test]
fn recursive_aggregate_printing_uses_the_same_bounded_shape() {
    assert_eq!(
        run_vm(RECURSIVE_AGGREGATE_SOURCE).expect("VM recursive aggregate source runs"),
        RECURSIVE_AGGREGATE_EXPECTED
    );

    let aot = common::run_aot(
        Path::new("native_recursive_aggregate_print_semantics.li"),
        RECURSIVE_AGGREGATE_SOURCE,
    )
    .expect("bounded AOT recursive aggregate source runs");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(aot.stdout_text().trim_end(), RECURSIVE_AGGREGATE_EXPECTED);

    let (jit_status, jit_stdout) = common::run_jit_capture(
        "native_recursive_aggregate_print_semantics.li",
        RECURSIVE_AGGREGATE_SOURCE,
    )
    .expect("bounded JIT recursive aggregate source runs");
    assert_eq!(jit_status, 0);
    assert_eq!(
        String::from_utf8_lossy(&jit_stdout).trim_end(),
        RECURSIVE_AGGREGATE_EXPECTED
    );
}

#[test]
fn dynamic_nested_aggregates_unicode_fields_and_typed_maps_match_the_vm_exactly() {
    assert_eq!(
        run_vm(DYNAMIC_NESTED_AGGREGATE_SOURCE).expect("VM dynamic aggregate print source runs"),
        DYNAMIC_NESTED_AGGREGATE_EXPECTED
    );

    let aot = common::run_aot(
        Path::new("native_dynamic_nested_aggregate_print_semantics.li"),
        DYNAMIC_NESTED_AGGREGATE_SOURCE,
    )
    .expect("bounded AOT dynamic aggregate print source runs");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(
        aot.stdout_text().trim_end(),
        DYNAMIC_NESTED_AGGREGATE_EXPECTED
    );

    let (jit_status, jit_stdout) = common::run_jit_capture(
        "native_dynamic_nested_aggregate_print_semantics.li",
        DYNAMIC_NESTED_AGGREGATE_SOURCE,
    )
    .expect("bounded JIT dynamic aggregate print source runs");
    assert_eq!(jit_status, 0);
    assert_eq!(
        String::from_utf8_lossy(&jit_stdout).trim_end(),
        DYNAMIC_NESTED_AGGREGATE_EXPECTED
    );
}

#[test]
fn typed_map_crossing_inferred_and_explicit_any_calls_preserves_entries() {
    assert_eq!(
        run_vm(TYPED_MAP_ANY_CALL_SOURCE).expect("VM typed-map Any call source runs"),
        TYPED_MAP_ANY_CALL_EXPECTED
    );

    let aot = common::run_aot(
        Path::new("native_typed_map_any_call.li"),
        TYPED_MAP_ANY_CALL_SOURCE,
    )
    .expect("bounded AOT typed-map Any call source runs");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(aot.stdout_text().trim_end(), TYPED_MAP_ANY_CALL_EXPECTED);

    let (jit_status, jit_stdout) =
        common::run_jit_capture("native_typed_map_any_call.li", TYPED_MAP_ANY_CALL_SOURCE)
            .expect("bounded JIT typed-map Any call source runs");
    assert_eq!(jit_status, 0);
    assert_eq!(
        String::from_utf8_lossy(&jit_stdout).trim_end(),
        TYPED_MAP_ANY_CALL_EXPECTED
    );
}

#[test]
fn oversized_aggregate_rendering_fails_instead_of_truncating() {
    let bytecode =
        lirac::compile(OVERSIZED_RENDER_SOURCE).expect("VM oversized-render source compiles");
    let vm_error = liravm::run_with_capture(&bytecode)
        .expect_err("VM oversized render must fail instead of producing partial output");
    assert_eq!(
        vm_error,
        "8:1: one printed value exceeded the 8388608 byte output limit"
    );

    let aot = common::run_aot(
        Path::new("native_oversized_print_semantics.li"),
        OVERSIZED_RENDER_SOURCE,
    )
    .expect("bounded AOT oversized-render source executes");
    assert!(!aot.status.success(), "oversized AOT render succeeded");
    assert!(aot.stdout.is_empty(), "AOT emitted partial rendered output");
    assert!(
        aot.stderr_text()
            .contains("one printed value exceeded the 8388608 byte output limit"),
        "unexpected AOT render-limit error: {}",
        aot.stderr_text()
    );

    let (jit_status, jit_stdout) = common::run_jit_capture(
        "native_oversized_print_semantics.li",
        OVERSIZED_RENDER_SOURCE,
    )
    .expect("bounded JIT oversized-render source executes");
    assert_eq!(jit_status, 1);
    assert!(jit_stdout.is_empty(), "JIT emitted partial rendered output");
}

#[test]
fn cyclic_aggregate_printing_is_bounded_and_matches_exactly() {
    const EXPECTED: &str = "[0, [...]]";
    assert_eq!(
        run_vm(CYCLIC_AGGREGATE_SOURCE).expect("VM cyclic aggregate source runs"),
        EXPECTED
    );

    let aot = common::run_aot(
        Path::new("native_cyclic_aggregate_print_semantics.li"),
        CYCLIC_AGGREGATE_SOURCE,
    )
    .expect("bounded AOT cyclic aggregate source runs");
    assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
    assert_eq!(aot.stdout_text().trim_end(), EXPECTED);

    let (jit_status, jit_stdout) = common::run_jit_capture(
        "native_cyclic_aggregate_print_semantics.li",
        CYCLIC_AGGREGATE_SOURCE,
    )
    .expect("bounded JIT cyclic aggregate source runs");
    assert_eq!(jit_status, 0);
    assert_eq!(String::from_utf8_lossy(&jit_stdout).trim_end(), EXPECTED);
}
