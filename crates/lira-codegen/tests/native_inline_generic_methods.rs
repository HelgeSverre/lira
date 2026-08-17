//! Bounded VM/AOT/JIT coverage for generic methods declared in a struct body.

use std::path::Path;

mod common;

const SOURCE: &str = r#"
struct Box<T> {
    value: T

    fn get(self) -> T {
        return self.value
    }

    fn map<U>(self, callback: fn(T) -> U) -> Box<U> {
        return Box { value: callback(self.value) }
    }
}

let factor = 3
let number = Box { value: 4 }
println(number.get())
println(number.map(|value: int| value * factor).get())
println(number.map(|value: int| "n=" + value).get())
"#;

fn run_vm(source: &str) -> Result<String, String> {
    let bytecode = lirac::compile(source)?;
    let (status, output) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM status {status}"));
    }
    Ok(output.join("\n"))
}

fn run_aot(source: &str) -> Result<String, String> {
    let output = common::run_aot(Path::new("native_inline_generic_methods.li"), source)
        .map_err(|error| format!("AOT run: {error}"))?;
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
}

fn run_jit(source: &str) -> Result<String, String> {
    let (status, output) = common::run_jit_capture("native_inline_generic_methods.li", source)?;
    if status != 0 {
        return Err(format!("JIT status {status}"));
    }
    Ok(String::from_utf8_lossy(&output).trim_end().to_string())
}

#[test]
fn inline_generic_methods_match_vm_aot_and_jit() {
    let expected = "4\n12\nn=4";
    assert_eq!(run_vm(SOURCE).expect("VM run"), expected);
    assert_eq!(run_aot(SOURCE).expect("AOT run"), expected);
    assert_eq!(run_jit(SOURCE).expect("JIT run"), expected);
}

#[test]
fn inline_generic_method_rejects_wrong_callback_type() {
    let source = r#"
struct Box<T> {
    value: T
    fn map<U>(self, callback: fn(T) -> U) -> Box<U> {
        return Box { value: callback(self.value) }
    }
}
let number = Box { value: 4 }
let invalid = number.map(|value: string| value)
"#;
    let error = lirac::compile(source).expect_err("wrong callback must be rejected");
    assert!(
        error.contains("Argument type mismatch"),
        "unexpected diagnostic: {error}"
    );
}
