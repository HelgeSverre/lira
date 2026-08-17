//! Native AOT/JIT coverage for class upcasts and virtual dispatch.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;

const SOURCE: &str = r#"
class Animal {
    fn speak(self) -> string { return "Animal" }
}

class Dog extends Animal {
    override fn speak(self) -> string { return "Dog" }
}

class Puppy extends Dog {
    override fn speak(self) -> string { return "Puppy" }
}

fn speak_as_animal(animal: Animal) -> string { return animal.speak() }
fn return_animal(dog: Dog) -> Animal { return dog }

fn main() {
    let dog = Dog {}
    let animal: Animal = dog
    println(speak_as_animal(dog))
    println(animal.speak())
    println(return_animal(dog).speak())

    let puppy = Puppy {}
    if speak_as_animal(puppy) != "Puppy" { println(1 / 0) }
}

main()
"#;

fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "lira-native-class-upcast-{}-{id}",
        std::process::id()
    ))
}

#[test]
fn class_upcasts_and_virtual_dispatch_run_in_aot() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, SOURCE).expect("write source");

    let output = common::run_aot(&source_path, SOURCE).expect("AOT class upcast source runs");
    assert!(output.status.success(), "AOT stderr: {:?}", output.stderr);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Dog\nDog\nDog\n");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn class_upcasts_and_virtual_dispatch_run_in_jit() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, SOURCE).expect("write source");

    let status = common::run_jit(source_path.to_str().expect("UTF-8 source path"), SOURCE)
        .expect("JIT class upcast source returns a status");
    assert_eq!(status, 0, "JIT virtual dispatch assertion failed");

    let _ = std::fs::remove_dir_all(dir);
}
