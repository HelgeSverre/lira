//! Real-source parity tests for native struct value semantics.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

mod common;

static AOT_LOCK: Mutex<()> = Mutex::new(());

const SOURCE: &str = r#"
struct Point { x: int }
struct Outer { inner: Point }
class Counter { x: int }

fn change(point: Point) -> Point {
    point.x = 8
    return point
}

fn update(value) {
    value.x = 12
    return value.x
}

let original = Point { x: 2 }
let changed = change(original)
println(original.x)
println(changed.x)

let outer = Outer { inner: Point { x: 13 } }
let outer_copy = outer
outer_copy.inner.x = 14
println(outer.inner.x)
println(outer_copy.inner.x)

// A same-typed identifier in a struct literal is not fresh and must still be
// copied. This guards the nested-literal fast path from becoming overbroad.
let source_inner = Point { x: 15 }
let outer_from_alias = Outer { inner: source_inner }
source_inner.x = 16
println(outer_from_alias.inner.x)

let first = Point { x: 3 }
let second = first
second.x = 9
println(first.x)
println(second.x)

let points = [first]
let extracted = points[0]
extracted.x = 10
println(first.x)
println(points[0].x)
println(extracted.x)

let table = { "point": first }
let mapped = table["point"]
mapped.x = 11
println(first.x)
println(table["point"].x)
println(mapped.x)

println(update(original))
println(original.x)

let counter = Counter { x: 5 }
let counter_alias = counter
counter_alias.x = 7
println(counter.x)
println(counter_alias.x)
println(update(counter))
println(counter.x)
"#;

// A recursive value with a null edge exercises the helper's null guard. The
// assignment is intentionally a value snapshot, so it does not create a
// reachable cycle: `node.next` remains a finite copy ending in null.
const RECURSIVE_NULL_SOURCE: &str = r#"
struct Node { next: Node? }
var node = Node { next: null }
node.next = node
println(node.next == null)
if node.next == null { println(1 / 0) }
"#;

fn deep_recursive_source() -> String {
    // Each nested literal contributes both a struct and a field-expression
    // node to the parser's 64-level budget. This remains beyond the native
    // recursive-render descriptor cutoff while keeping the executable small.
    const NESTING: usize = 24;

    let mut declarations = String::new();
    for level in 0..=NESTING {
        if level == 0 {
            declarations.push_str("struct Node0 { value: int }\n");
        } else {
            declarations.push_str(&format!(
                "struct Node{level} {{ value: int, next: Node{} }}\n",
                level - 1
            ));
        }
    }

    let mut literal = String::from("Node0 { value: 0 }");
    for level in 1..=NESTING {
        literal = format!("Node{level} {{ value: 0, next: {literal} }}");
    }

    let mut copy_path = String::from("copy");
    let mut original_path = String::from("original");
    for _ in 0..NESTING {
        copy_path.push_str(".next");
        original_path.push_str(".next");
    }
    copy_path.push_str(".value");
    original_path.push_str(".value");

    format!(
        "{declarations}var original = {literal}\n\
let copy = original\n\
{original_path} = 999\n\
println({copy_path})\n\
println({original_path})\n\
if {copy_path} != 0 {{ println(1 / 0) }}\n\
if {original_path} != 999 {{ println(1 / 0) }}\n"
    )
}

const JIT_SOURCE: &str = r#"
struct Point { x: int }
struct Outer { inner: Point }
class Counter { x: int }
fn change(point: Point) -> int {
    point.x = 8
    return point.x
}
fn update(value) -> int {
    value.x = 12
    return value.x
}
let point = Point { x: 2 }
change(point)
if point.x != 2 { println(1 / 0) }
let counter = Counter { x: 5 }
let alias = counter
alias.x = 7
if counter.x != 7 { println(1 / 0) }
update(point)
if point.x != 2 { println(1 / 0) }
update(counter)
if counter.x != 12 { println(1 / 0) }
let outer = Outer { inner: Point { x: 13 } }
let outer_copy = outer
outer_copy.inner.x = 14
if outer.inner.x != 13 { println(1 / 0) }
if outer_copy.inner.x != 14 { println(1 / 0) }
let source_inner = Point { x: 15 }
let outer_from_alias = Outer { inner: source_inner }
source_inner.x = 16
if outer_from_alias.inner.x != 15 { println(1 / 0) }
"#;

const ANY_COPY_SOURCE: &str = r#"
struct Point { x: int }
class Counter { x: int }
fn pass(value: any) -> any { return value }

let point: any = Point { x: 7 }
let copy: any = pass(point)
copy.x = 9
println(point.x)
println(copy.x)

let counter: any = Counter { x: 3 }
let alias: any = pass(counter)
alias.x = 4
println(counter.x)
println(alias.x)

let values: [any] = [point]
let element: any = values[0]
element.x = 11
println(point.x)

let matched: any = match point {
    value => value
}
matched.x = 12
println(point.x)
"#;

const ANY_COPY_JIT_SOURCE: &str = r#"
struct Point { x: int }
class Counter { x: int }
fn pass(value: any) -> any { return value }

let point: any = Point { x: 7 }
let copy: any = pass(point)
copy.x = 9
if point.x != 7 { println(1 / 0) }
if copy.x != 9 { println(1 / 0) }

let counter: any = Counter { x: 3 }
let alias: any = pass(counter)
alias.x = 4
if counter.x != 4 { println(1 / 0) }
if alias.x != 4 { println(1 / 0) }
"#;

const ANY_CHANNEL_COPY_SOURCE: &str = r#"
struct Point { x: int }
class Counter { x: int }

fn main() {
    let ordinary: Channel<any> = chan(2)
    let point: any = Point { x: 1 }
    send(ordinary, point)
    point.x = 2
    let received_point: any = recv(ordinary)
    println(received_point.x)

    let counter: any = Counter { x: 3 }
    send(ordinary, counter)
    counter.x = 4
    let received_counter: any = recv(ordinary)
    println(received_counter.x)

    let selected: Channel<any> = chan(1)
    let selected_point: any = Point { x: 5 }
    select { selected_point -> selected => {} }
    selected_point.x = 6
    let received_selected: any = recv(selected)
    println(received_selected.x)
}
"#;

const ANY_CHANNEL_COPY_JIT_SOURCE: &str = r#"
struct Point { x: int }
class Counter { x: int }

fn main() {
    let ordinary: Channel<any> = chan(2)
    let point: any = Point { x: 1 }
    send(ordinary, point)
    point.x = 2
    let received_point: any = recv(ordinary)
    if received_point.x != 1 { println(1 / 0) }

    let counter: any = Counter { x: 3 }
    send(ordinary, counter)
    counter.x = 4
    let received_counter: any = recv(ordinary)
    if received_counter.x != 4 { println(1 / 0) }

    let selected: Channel<any> = chan(1)
    let selected_point: any = Point { x: 5 }
    select { selected_point -> selected => {} }
    selected_point.x = 6
    let received_selected: any = recv(selected)
    if received_selected.x != 5 { println(1 / 0) }
}
"#;

fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "lira-native-struct-value-{}-{id}",
        std::process::id()
    ))
}

fn run_vm() -> (i32, Vec<String>) {
    run_vm_source(SOURCE)
}

fn run_vm_source(source: &str) -> (i32, Vec<String>) {
    let bytecode = lirac::compile(source).expect("struct value source compiles for VM");
    liravm::run_with_capture(&bytecode).expect("struct value source runs on VM")
}

fn run_aot() -> (i32, String, String) {
    run_aot_source(SOURCE)
}

fn run_aot_source(source: &str) -> (i32, String, String) {
    let _guard = AOT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("create AOT scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).expect("write AOT source");
    let result = {
        let output = common::run_aot(&source_path, source).expect("run native struct value binary");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_jit_source(source: &str) -> Result<i32, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).map_err(|error| error.to_string())?;
    let result = common::run_jit(
        source_path
            .to_str()
            .ok_or_else(|| "UTF-8 path".to_owned())?,
        source,
    );
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[test]
fn struct_values_copy_across_native_boundaries_but_classes_alias() {
    let expected = vec![
        "2", "8", "13", "14", "15", "3", "9", "3", "3", "10", "3", "3", "11", "12", "2", "7", "7",
        "12", "12",
    ];
    let (vm_status, vm_lines) = run_vm();
    assert_eq!(vm_status, 0);
    assert_eq!(vm_lines, expected);

    let (aot_status, stdout, stderr) = run_aot();
    assert_eq!(aot_status, 0, "AOT stderr: {stderr}");
    assert!(stderr.is_empty(), "unexpected AOT stderr: {stderr}");
    assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
}

#[test]
fn jit_struct_values_have_the_same_mutation_boundaries() {
    let result = run_jit_source(JIT_SOURCE);
    assert_eq!(result, Ok(0), "JIT struct value source failed: {result:?}");
}

#[test]
fn recursive_struct_copy_handles_null_edges_and_deep_chains() {
    let (vm_status, vm_lines) = run_vm_source(RECURSIVE_NULL_SOURCE);
    assert_eq!(vm_status, 0);
    assert_eq!(vm_lines, vec!["false"]);

    let (aot_status, aot_stdout, aot_stderr) = run_aot_source(RECURSIVE_NULL_SOURCE);
    assert_eq!(aot_status, 0, "recursive AOT stderr: {aot_stderr}");
    assert_eq!(aot_stdout.lines().collect::<Vec<_>>(), vec!["false"]);
    assert_eq!(run_jit_source(RECURSIVE_NULL_SOURCE), Ok(0));

    let deep_source = deep_recursive_source();
    let (deep_vm_status, deep_vm_lines) = run_vm_source(&deep_source);
    assert_eq!(deep_vm_status, 0);
    let deep_expected = vec!["0", "999"];
    assert_eq!(deep_vm_lines, deep_expected);
    let (deep_aot_status, deep_aot_stdout, deep_aot_stderr) = run_aot_source(&deep_source);
    assert_eq!(deep_aot_status, 0, "deep AOT stderr: {deep_aot_stderr}");
    assert_eq!(deep_aot_stdout.lines().collect::<Vec<_>>(), deep_expected);
    assert_eq!(run_jit_source(&deep_source), Ok(0));
}

#[test]
fn any_struct_values_copy_and_reference_payloads_alias_natively() {
    let expected = vec!["7", "9", "4", "4", "7", "7"];
    let (vm_status, vm_lines) = run_vm_source(ANY_COPY_SOURCE);
    assert_eq!(vm_status, 0);
    assert_eq!(vm_lines, expected);

    let (aot_status, aot_stdout, aot_stderr) = run_aot_source(ANY_COPY_SOURCE);
    assert_eq!(aot_status, 0, "Any-copy AOT stderr: {aot_stderr}");
    assert_eq!(aot_stdout.lines().collect::<Vec<_>>(), expected);
    assert_eq!(run_jit_source(ANY_COPY_JIT_SOURCE), Ok(0));
}

#[test]
fn any_channel_payloads_copy_structs_and_preserve_class_identity() {
    let expected = vec!["1", "4", "5"];
    let (vm_status, vm_lines) = run_vm_source(ANY_CHANNEL_COPY_SOURCE);
    assert_eq!(vm_status, 0);
    assert_eq!(vm_lines, expected);

    let (aot_status, aot_stdout, aot_stderr) = run_aot_source(ANY_CHANNEL_COPY_SOURCE);
    assert_eq!(aot_status, 0, "Any-channel AOT stderr: {aot_stderr}");
    assert_eq!(aot_stdout.lines().collect::<Vec<_>>(), expected);
    assert_eq!(run_jit_source(ANY_CHANNEL_COPY_JIT_SOURCE), Ok(0));
}
