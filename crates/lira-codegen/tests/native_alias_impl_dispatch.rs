//! Alias-owner impl dispatch must produce identical VM, AOT, and JIT results.

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
    let output = common::run_aot(Path::new("native_alias_impl_dispatch.li"), source)?;
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
    let (status, stdout) = common::run_jit_capture("native_alias_impl_dispatch.li", source)?;
    if status != 0 {
        return Err(format!("JIT status {status}"));
    }
    Ok(String::from_utf8_lossy(&stdout).trim_end().to_string())
}

#[test]
fn alias_impl_instance_static_aggregate_and_array_calls_match_all_backends() {
    let source = r#"
        type Integer = int
        impl Integer {
            fn bump(self) -> int { return self + 1 }
            fn answer() -> int { return 42 }
        }

        struct Point { x: int }
        type Position = Point
        impl Position {
            fn value(self) -> int { return self.x }
        }

        type IntegerAlias = int
        type Ints = [IntegerAlias]
        impl Ints {
            fn first_plus(self) -> int { return self[0] + 1 }
        }

        let x: Integer = 41
        let p: Position = Point { x: 42 }
        let xs: Ints = [41]
        println(x.bump())
        println(Integer.answer())
        println(p.value())
        println(xs.first_plus())
    "#;
    let expected = "42\n42\n42\n42";
    assert_eq!(run_vm(source).expect("VM alias impl run"), expected);
    assert_eq!(run_aot(source).expect("AOT alias impl run"), expected);
    assert_eq!(run_jit(source).expect("JIT alias impl run"), expected);
}
