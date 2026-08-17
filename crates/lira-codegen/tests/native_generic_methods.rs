//! Bounded VM/AOT/JIT parity for generic struct methods.

use std::path::Path;

mod common;

fn run_vm(source: &str) -> Result<String, String> {
    let bytecode = lirac::compile(source)?;
    let (status, lines) = liravm::run_with_capture(&bytecode)?;
    if status != 0 {
        return Err(format!("VM status {status}"));
    }
    Ok(lines.join("\n"))
}

fn run_aot(source: &str) -> Result<String, String> {
    let output = common::run_aot(Path::new("native_generic_methods.li"), source)
        .map_err(|error| format!("run AOT: {error}"))?;
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
    let (status, stdout) = common::run_jit_capture("native_generic_methods.li", source)?;
    if status != 0 {
        return Err(format!("JIT status {status}"));
    }
    Ok(String::from_utf8_lossy(&stdout).trim_end().to_string())
}

#[test]
fn generic_owner_and_method_type_parameters_match_across_backends() {
    let source = r#"
        struct Box<T> { value: T }
        impl<T> Box<T> {
            fn get(self) -> T { return self.value }
            fn map<U>(self, f: fn(T) -> U) -> Box<U> {
                return Box { value: f(self.value) }
            }
        }

        let factor = 3
        let number = Box { value: 4 }
        println(number.get())
        println(number.map(|value: int| value * factor).get())
        println(number.map(|value: int| "n=" + value).get())
        println(Box { value: 5 }.map(|value: int| value + 1).get())
    "#;
    let expected = "4\n12\nn=4\n6";
    assert_eq!(run_vm(source).expect("generic method VM run"), expected);
    assert_eq!(run_aot(source).expect("generic method AOT run"), expected);
    assert_eq!(run_jit(source).expect("generic method JIT run"), expected);
}

#[test]
fn generic_method_rejects_a_callback_with_the_wrong_input_type() {
    let source = r#"
        struct Box<T> { value: T }
        impl<T> Box<T> {
            fn map<U>(self, f: fn(T) -> U) -> Box<U> {
                return Box { value: f(self.value) }
            }
        }
        let number = Box { value: 4 }
        let invalid = number.map(|value: string| value)
    "#;
    let error = lirac::compile(source).expect_err("wrong generic callback must be rejected");
    assert!(
        error.contains("Type mismatch") || error.contains("type mismatch"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn generic_and_source_names_that_sanitise_similarly_keep_distinct_symbols() {
    let source = r#"
        struct Box<T> { value: T }
        impl<T> Box<T> {
            fn get(self) -> T { return self.value }
        }

        struct Box_int { value: int }
        impl Box_int {
            fn get(self) -> int { return self.value + 100 }
        }

        struct A<T> { value: T }
        impl<T> A<T> {
            fn foo(self) -> T { return self.value }
        }

        struct A_x24_int_x3a {}
        impl A_x24_int_x3a {
            fn x3a_foo(self) -> int { return 107 }
        }

        let generic = Box { value: 2 }
        let source_named = Box_int { value: 3 }
        println(generic.get())
        println(source_named.get())
        println(A { value: 5 }.foo())
        println(A_x24_int_x3a {}.x3a_foo())
    "#;
    let expected = "2\n103\n5\n107";
    assert_eq!(run_vm(source).expect("symbol collision VM run"), expected);
    assert_eq!(run_aot(source).expect("symbol collision AOT run"), expected);
    assert_eq!(run_jit(source).expect("symbol collision JIT run"), expected);
}
