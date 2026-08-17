//! Native erased-class identity through direct and nested aggregate boundaries.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

const SOURCE: &str = r#"
class Animal {}
class Dog extends Animal {}
class Puppy extends Dog {}
class Cat extends Animal {}

fn make_animal() -> Animal {
    println("once")
    return Puppy {}
}

let animal: Animal = make_animal()
let erased: any = animal
println(erased is Animal)
println(erased is Dog)
println(erased is Puppy)
println(erased is Cat)

let animals: [Animal] = [Dog {}]
let erased_animals: any = animals
let erased_element: any = erased_animals[0]
println(erased_element is Animal)
println(erased_element is Dog)
println(erased_element is Puppy)
println(erased_element is Cat)
"#;

fn source_path(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-erased-class-{label}-{}-{}.li",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn with_source_path<T>(label: &str, f: impl FnOnce(&Path) -> T) -> T {
    let path = source_path(label);
    std::fs::write(&path, SOURCE).expect("write source");
    let result = f(&path);
    let _ = std::fs::remove_file(path);
    result
}

const NATIVE_EXPECTED: &str = "once\ntrue\ntrue\ntrue\nfalse\ntrue\ntrue\nfalse\nfalse";

#[test]
fn erased_classes_keep_concrete_identity_in_aot_and_jit() {
    with_source_path("native", |path| {
        let aot = common::run_aot(path, SOURCE).expect("bounded AOT execution");
        aot.assert_complete_output().expect("complete AOT output");
        assert!(aot.status.success(), "AOT stderr: {}", aot.stderr_text());
        assert_eq!(
            String::from_utf8_lossy(&aot.stdout).trim_end(),
            NATIVE_EXPECTED
        );

        let (status, output) =
            common::run_jit_capture(path.to_str().expect("UTF-8 source path"), SOURCE)
                .expect("bounded JIT execution");
        assert_eq!(status, 0, "JIT execution failed");
        assert_eq!(String::from_utf8_lossy(&output).trim_end(), NATIVE_EXPECTED);
    });
}

#[test]
fn erased_classes_run_through_bounded_vm_with_coarse_tag_expectations() {
    // The bytecode VM intentionally stores only the coarse object TypeIs tag;
    // it therefore reports every nominal class check as true. Native AOT/JIT
    // assertions above are the exact identity contract, including siblings.
    let vm_expected = "once\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue";
    with_source_path("vm", |path| {
        let outcome = common::run_vm_capture(path, SOURCE).expect("bounded VM execution");
        match outcome {
            common::VmRunOutcome::Success { status, output } => {
                assert_eq!(status, 0, "VM execution failed");
                assert_eq!(String::from_utf8_lossy(&output).trim_end(), vm_expected);
            }
            other => panic!("unexpected VM outcome: {other:?}"),
        }
    });
}
