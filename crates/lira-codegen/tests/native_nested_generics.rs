//! Native coverage for recursive materialization of nested generic layouts.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

mod common;

static AOT_LOCK: Mutex<()> = Mutex::new(());

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-nested-generics-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn run_vm(source: &str) -> Result<String, String> {
    let bytecode = lirac::compile(source)?;
    let (status, lines) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM status {status}"));
    }
    Ok(lines.join("\n"))
}

fn run_aot(source: &str) -> Result<String, String> {
    let _guard = AOT_LOCK
        .lock()
        .map_err(|error| format!("AOT lock poisoned: {error}"))?;
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    let result = (|| {
        std::fs::write(&path, source).map_err(|error| error.to_string())?;
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

fn run_jit(source: &str) -> Result<i32, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("program.li");
    let result = (|| {
        std::fs::write(&path, source).map_err(|error| error.to_string())?;
        common::run_jit(path.to_str().ok_or("non-utf8 source path")?, source)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[test]
fn nested_generic_struct_fields_are_materialized_for_all_native_backends() {
    let source = r#"
        struct Box<T> { value: T }
        struct Pair<T> { inner: Box<T> }

        fn make<T>(x: T) -> Pair<T> {
            return Pair { inner: Box { value: x } }
        }

        fn main() {
            println(make(7).inner.value)
            println(make("ok").inner.value)
        }
    "#;
    let expected = "7\nok";
    assert_eq!(run_vm(source).expect("nested generic VM run"), expected);
    assert_eq!(run_aot(source).expect("nested generic AOT run"), expected);
    assert_eq!(run_jit(source).expect("nested generic JIT run"), 0);
}

#[test]
fn nested_generic_enum_and_optional_payloads_are_materialized() {
    let source = r#"
        enum Maybe<T> { Some(T), None }
        struct Holder<T> { value: Maybe<T>? }

        fn make<T>(x: T) -> Holder<T> {
            return Holder { value: Maybe::Some(x) }
        }

        fn main() {
            let holder = make(7)
            match holder.value {
                Maybe::Some(value) => println(value),
                Maybe::None => println("none"),
                _ => println("missing")
            }
            let missing: Maybe<int>? = null
            match missing {
                Maybe::Some(value) => println(value),
                _ => println("missing")
            }
        }
    "#;
    let expected = "7\nmissing";
    assert_eq!(run_vm(source).expect("nested enum VM run"), expected);
    assert_eq!(run_aot(source).expect("nested enum AOT run"), expected);
    assert_eq!(run_jit(source).expect("nested enum JIT run"), 0);
}

#[test]
fn recursive_generic_layouts_terminate_at_the_reserved_placeholder() {
    let source = r#"
        struct Node<T> { value: T, next: Node<T>? }

        fn make<T>(x: T) -> Node<T> {
            return Node { value: x, next: null }
        }

        fn main() {
            println(make(9).value)
        }
    "#;
    let expected = "9";
    assert_eq!(run_vm(source).expect("recursive generic VM run"), expected);
    assert_eq!(
        run_aot(source).expect("recursive generic AOT run"),
        expected
    );
    assert_eq!(run_jit(source).expect("recursive generic JIT run"), 0);
}

#[test]
fn concrete_nested_field_type_mismatches_are_rejected() {
    let source = r#"
        struct Box<T> { value: T }
        struct Pair<T> { inner: Box<T> }
        struct Concrete { value: int }

        let bad = Concrete { value: "wrong" }
    "#;
    let error = lirac::check(source).expect_err("concrete field mismatch must be rejected");
    assert!(
        error.contains("Type mismatch") && error.contains("int") && error.contains("string"),
        "unexpected diagnostic: {error}"
    );
}
