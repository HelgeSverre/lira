//! End-to-end tests for sample programs
//!
//! Tests that all sample programs compile and run correctly.

/// Helper to compile a program
fn compile(source: &str) -> Result<Vec<u8>, String> {
    let tokens = lirac::lexer::tokenize(source).map_err(|e| e.to_string())?;
    let ast = lirac::parser::parse(&tokens).map_err(|e| e.to_string())?;
    let typed_ast = lirac::checker::check(&ast).map_err(|e| e.to_string())?;
    lirac::codegen::generate(&typed_ast).map_err(|e| e.to_string())
}

/// Helper to compile and get AST
fn compile_ast(source: &str) -> Result<lirac::ast::Program, String> {
    let tokens = lirac::lexer::tokenize(source).map_err(|e| e.to_string())?;
    lirac::parser::parse(&tokens).map_err(|e| e.to_string())
}

/// Helper to compile and run a program
fn run(source: &str) -> Result<(i32, Vec<String>), String> {
    let bytecode = compile(source)?;
    liravm::run_with_capture(&bytecode)
}

#[test]
fn test_hello_world() {
    let source = r#"
println("Hello, Lira!")
println("Welcome to the Lira Playground!")
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    assert_eq!(output.len(), 2);
    assert_eq!(output[0], "Hello, Lira!");
    assert_eq!(output[1], "Welcome to the Lira Playground!");
}

#[test]
fn test_fibonacci() {
    let source = r#"
fn fib(n: int) -> int {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

var i = 0
while i < 5 {
    let result = fib(i)
    println(result)
    i = i + 1
}
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    // fib(0)=0, fib(1)=1, fib(2)=1, fib(3)=2, fib(4)=3
    assert_eq!(output, vec!["0", "1", "1", "2", "3"]);
}

#[test]
fn test_arrays() {
    let source = r#"
let numbers = [1, 2, 3, 4, 5]
var sum = 0
for n in numbers {
    sum = sum + n
}
println(sum)
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    assert_eq!(output, vec!["15"]);
}

#[test]
fn test_pattern_matching() {
    let source = r#"
fn grade(score: int) -> string {
    return match score {
        100 => "Perfect!",
        90 => "A",
        80 => "B",
        _ => "F"
    }
}

println(grade(100))
println(grade(90))
println(grade(75))
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    assert_eq!(output, vec!["Perfect!", "A", "F"]);
}

#[test]
fn test_enums() {
    let source = r#"
enum Color {
    Red,
    Green,
    Blue
}

let red = Color::Red
println(red.__variant)
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    assert_eq!(output, vec!["Red"]);
}

#[test]
fn test_structs() {
    let source = r#"
struct Point {
    x: int
    y: int

    fn sum(self) -> int {
        return self.x + self.y
    }
}

let point = Point { x: 10, y: 20 }
println(point.x)
println(point.y)
println(point.sum())
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    assert_eq!(output, vec!["10", "20", "30"]);
}

#[test]
fn test_closures() {
    let source = r#"
let double = |x: int| x * 2
println(double(5))
println(double(21))

fn make_adder(n: int) -> fn(int) -> int {
    return |x: int| x + n
}

let add5 = make_adder(5)
println(add5(3))
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    assert_eq!(output, vec!["10", "42", "8"]);
}

#[test]
fn test_control_flow() {
    let source = r#"
fn abs(x: int) -> int {
    if x < 0 {
        return -x
    } else {
        return x
    }
}

println(abs(-5))
println(abs(5))

fn factorial(n: int) -> int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

println(factorial(5))
"#;

    let result = run(source);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (exit_code, output) = result.unwrap();
    assert_eq!(exit_code, 0);
    assert_eq!(output, vec!["5", "5", "120"]);
}

// ===== AST Serialization Tests =====

#[test]
fn test_ast_serialization_hello_world() {
    let source = r#"println("Hello, World!")"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
    let json = json.unwrap();
    assert!(
        json.get("statements").is_some(),
        "AST should have statements"
    );
}

#[test]
fn test_ast_serialization_function() {
    let source = r#"
fn add(a: int, b: int) -> int {
    return a + b
}
"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_struct() {
    let source = r#"
struct Point {
    x: int
    y: int
}
"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_enum() {
    let source = r#"
enum Color {
    Red,
    Green,
    Blue
}
"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_match() {
    let source = r#"
let x = match 1 {
    1 => "one",
    _ => "other"
}
"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_lambda() {
    let source = r#"let double = |x: int| x * 2"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_array() {
    let source = r#"let arr = [1, 2, 3]"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_for_loop() {
    let source = r#"
for x in [1, 2, 3] {
    println(x)
}
"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_while_loop() {
    let source = r#"
var x = 0
while x < 10 {
    x = x + 1
}
"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

#[test]
fn test_ast_serialization_if_else() {
    let source = r#"
if true {
    println("yes")
} else {
    println("no")
}
"#;
    let ast = compile_ast(source).expect("Should parse");
    let json = serde_json::to_value(&ast);
    assert!(json.is_ok(), "AST serialization failed: {:?}", json.err());
}

// ===== Breakpoint/Stepping Tests =====

/// Helper to run with breakpoints and return the breakpoint hit (if any)
fn run_with_breakpoints(
    source: &str,
    breakpoints: Vec<u32>,
) -> Result<(Option<u32>, Vec<String>), String> {
    let bytecode = compile(source)?;
    let mut vm = liravm::create_vm(&bytecode).map_err(|e| e.to_string())?;
    vm.set_capture_output(true);
    vm.set_breakpoints(breakpoints);

    match vm.run() {
        Ok(_exit_code) => Ok((None, vm.get_output().to_vec())),
        Err(e) => {
            if e.starts_with("Breakpoint hit at line ") {
                // Parse breakpoint line from error message
                let line = e
                    .strip_prefix("Breakpoint hit at line ")
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.parse::<u32>().ok());
                Ok((line, vm.get_output().to_vec()))
            } else {
                Err(e)
            }
        }
    }
}

#[test]
fn test_breakpoint_stops_at_first_breakpoint() {
    // Line numbers (1-based):
    // 1: (empty)
    // 2: println("line 2")
    // 3: println("line 3")
    // 4: println("line 4")
    let source = r#"
println("line 2")
println("line 3")
println("line 4")
"#;

    // Set breakpoint at line 3
    let result = run_with_breakpoints(source, vec![3]);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (bp_line, output) = result.unwrap();

    // Should stop at line 3
    assert_eq!(bp_line, Some(3), "Should stop at line 3");
    // Should have output from line 2 only
    assert_eq!(output, vec!["line 2"]);
}

#[test]
fn test_breakpoint_stops_at_first_of_two() {
    let source = r#"
println("line 2")
println("line 3")
println("line 4")
println("line 5")
"#;

    // Set breakpoints at lines 3 and 5
    let result = run_with_breakpoints(source, vec![3, 5]);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (bp_line, output) = result.unwrap();

    // Should stop at line 3 (first breakpoint)
    assert_eq!(bp_line, Some(3), "Should stop at first breakpoint (line 3)");
    // Should have output from line 2 only
    assert_eq!(output, vec!["line 2"]);
}

#[test]
fn test_continue_to_second_breakpoint() {
    let source = r#"
println("line 2")
println("line 3")
println("line 4")
println("line 5")
"#;

    // Simulate "continue" from line 3 to line 5:
    // We only set breakpoint at line 5 (filtering out line 3)
    let result = run_with_breakpoints(source, vec![5]);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (bp_line, output) = result.unwrap();

    // Should stop at line 5
    assert_eq!(bp_line, Some(5), "Should stop at line 5");
    // Should have output from lines 2, 3, 4
    assert_eq!(output, vec!["line 2", "line 3", "line 4"]);
}

#[test]
fn test_step_into_skips_current_line() {
    let source = r#"
println("line 2")
println("line 3")
println("line 4")
println("line 5")
"#;

    // Simulate "step into" from line 3:
    // Set breakpoints at lines 4, 5, 6 (current_line + 1, +2, +3)
    // This should stop at line 4
    let result = run_with_breakpoints(source, vec![4, 5, 6]);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (bp_line, output) = result.unwrap();

    // Should stop at line 4
    assert_eq!(bp_line, Some(4), "Should stop at next line (4)");
    // Should have output from lines 2, 3
    assert_eq!(output, vec!["line 2", "line 3"]);
}

#[test]
fn test_no_breakpoint_runs_to_completion() {
    let source = r#"
println("line 2")
println("line 3")
println("line 4")
"#;

    // No breakpoints
    let result = run_with_breakpoints(source, vec![]);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (bp_line, output) = result.unwrap();

    // Should complete (no breakpoint hit)
    assert_eq!(bp_line, None, "Should run to completion");
    assert_eq!(output, vec!["line 2", "line 3", "line 4"]);
}

#[test]
fn test_breakpoint_after_last_line_runs_to_completion() {
    let source = r#"
println("line 2")
println("line 3")
"#;

    // Breakpoint after all code
    let result = run_with_breakpoints(source, vec![100]);
    assert!(result.is_ok(), "Failed: {:?}", result);
    let (bp_line, output) = result.unwrap();

    // Should complete (breakpoint never reached)
    assert_eq!(bp_line, None, "Should run to completion");
    assert_eq!(output, vec!["line 2", "line 3"]);
}
