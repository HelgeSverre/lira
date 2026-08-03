//! Integration tests for Lira compiler and VM
//!
//! These tests verify that example programs compile and run correctly.
//! Expected output is parsed from comments in the source files:
//! - `// @expect: <output>` - expect this exact line in output
//! - `// @expect-contains: <text>` - output should contain this text
//! - `// @expect-error` - expect compilation to fail
//! - `// @skip` - skip this test (for known issues)

use std::fs;
use std::path::Path;

/// Test result for a single example file
#[derive(Debug)]
struct TestResult {
    name: String,
    passed: bool,
    message: String,
}

/// Parse expected output from source file comments
fn parse_expectations(source: &str) -> (Vec<String>, Vec<String>, bool, bool) {
    let mut expect_lines = Vec::new();
    let mut expect_contains = Vec::new();
    let mut expect_error = false;
    let mut skip = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("// @expect:") {
            let value = trimmed.strip_prefix("// @expect:").unwrap().trim();
            expect_lines.push(value.to_string());
        } else if trimmed.starts_with("// @expect-contains:") {
            let value = trimmed.strip_prefix("// @expect-contains:").unwrap().trim();
            expect_contains.push(value.to_string());
        } else if trimmed.starts_with("// @expect-error") {
            expect_error = true;
        } else if trimmed.starts_with("// @skip") {
            skip = true;
        }
    }

    (expect_lines, expect_contains, expect_error, skip)
}

/// Compile and run a Lira source file, returning captured output
fn compile_and_run(source_path: &str) -> Result<Vec<String>, String> {
    let source = fs::read_to_string(source_path)
        .map_err(|e| format!("Failed to read {}: {}", source_path, e))?;

    // Compile with import support
    let bytecode = lirac::compile_with_imports(source_path, &source)?;

    // Run and capture output
    let (_exit_code, output) = liravm::run_with_capture(&bytecode)?;

    Ok(output)
}

/// Test a single example file
fn test_example(path: &Path) -> TestResult {
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    let source_path = path.to_string_lossy().to_string();

    // Read source and parse expectations
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return TestResult {
                name,
                passed: false,
                message: format!("Failed to read file: {}", e),
            };
        }
    };

    let (expect_lines, expect_contains, expect_error, skip) = parse_expectations(&source);

    // Skip if marked
    if skip {
        return TestResult {
            name,
            passed: true,
            message: "SKIPPED".to_string(),
        };
    }

    // Try to compile and run
    let result = compile_and_run(&source_path);

    match result {
        Ok(output) => {
            if expect_error {
                return TestResult {
                    name,
                    passed: false,
                    message: "Expected compilation error but succeeded".to_string(),
                };
            }

            // Check expected lines
            for (i, expected) in expect_lines.iter().enumerate() {
                if i >= output.len() {
                    return TestResult {
                        name,
                        passed: false,
                        message: format!(
                            "Expected output line {}: '{}' but only got {} lines",
                            i,
                            expected,
                            output.len()
                        ),
                    };
                }
                if output[i] != *expected {
                    return TestResult {
                        name,
                        passed: false,
                        message: format!(
                            "Line {} mismatch:\n  expected: '{}'\n  actual:   '{}'",
                            i, expected, output[i]
                        ),
                    };
                }
            }

            // Check expected contains
            let full_output = output.join("\n");
            for expected in &expect_contains {
                if !full_output.contains(expected) {
                    return TestResult {
                        name,
                        passed: false,
                        message: format!(
                            "Output should contain '{}'\nActual output:\n{}",
                            expected, full_output
                        ),
                    };
                }
            }

            // If no expectations, just verify it compiles and runs
            TestResult {
                name,
                passed: true,
                message: format!("OK ({} lines of output)", output.len()),
            }
        }
        Err(e) => {
            if expect_error {
                TestResult {
                    name,
                    passed: true,
                    message: "Expected error (got it)".to_string(),
                }
            } else {
                TestResult {
                    name,
                    passed: false,
                    message: format!("Error: {}", e),
                }
            }
        }
    }
}

/// Get all .li files in the examples directory
fn get_example_files() -> Vec<std::path::PathBuf> {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples");

    let mut files: Vec<_> = fs::read_dir(&examples_dir)
        .expect("Failed to read examples directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "li"))
        .collect();

    files.sort();
    files
}

// =============================================================================
// Individual example tests - one test per file for clear failure reporting
// =============================================================================

macro_rules! example_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("examples");
            let path = examples_dir.join($file);
            if !path.exists() {
                println!("Skipping {} - file not found", $file);
                return;
            }
            let result = test_example(&path);
            if !result.passed {
                panic!("{}: {}", result.name, result.message);
            }
        }
    };
}

// Core language tests
example_test!(test_hello, "hello.li");
example_test!(test_fibonacci, "fibonacci.li");
example_test!(test_factorial, "factorial.li");
example_test!(test_control_flow, "control_flow.li");
example_test!(test_prime_checker, "prime_checker.li");

// Data types
example_test!(test_integer_types, "integer_types.li");
example_test!(test_string_ops, "string_ops.li");
example_test!(test_string_interpolation, "string_interpolation.li");
example_test!(test_array_ops, "array_ops.li");
example_test!(test_array_types, "array_types.li");
example_test!(test_structs, "structs.li");
example_test!(test_nested_structures, "nested_structures.li");

// Operators
example_test!(test_bitwise_ops, "bitwise_ops.li");
example_test!(test_compound_assign, "compound_assign.li");
example_test!(test_operator_comprehensive, "operator_comprehensive.li");
example_test!(test_arithmetic_edge_cases, "arithmetic_edge_cases.li");

// Control flow
example_test!(test_control_flow_test, "control_flow_test.li");
example_test!(test_for_loop, "for_loop.li");
example_test!(test_loop_infinite, "loop_infinite.li");
example_test!(test_pattern_match, "pattern_match.li");
example_test!(test_pattern_guards, "pattern_guards.li");
example_test!(test_pattern_tuple, "pattern_tuple.li");
example_test!(test_pattern_tuple_simple, "pattern_tuple_simple.li");
example_test!(test_pattern_tuple_literals, "pattern_tuple_literals.li");
example_test!(test_untyped_function_ops, "untyped_function_ops.li");
example_test!(test_pattern_constructor, "pattern_constructor.li");
example_test!(
    test_pattern_constructor_verify,
    "pattern_constructor_verify.li"
);

// Functions and closures
example_test!(test_lambda, "lambda.li");
example_test!(test_mutual_recursion, "mutual_recursion.li");
example_test!(test_recursion_stress, "recursion_stress.li");
example_test!(test_tail_call_regressions, "tail_call_regressions.li");
example_test!(test_named_args, "named_args.li");
example_test!(test_named_args_defaults, "named_args_defaults.li");
example_test!(
    test_named_args_missing_required,
    "named_args_missing_required.li"
);

// Concurrency
example_test!(test_fiber_basic, "fiber_basic.li");
example_test!(test_channel_basic, "channel_basic.li");
example_test!(test_select_basic, "select_basic.li");

// Modules and imports
example_test!(test_import_test, "import_test.li");
example_test!(test_import_selective, "import_selective.li");
example_test!(test_module_comprehensive, "module_comprehensive.li");

// OOP features
example_test!(test_classes_basic, "classes_basic.li");
example_test!(test_class_inheritance, "class_inheritance.li");
example_test!(test_class_methods, "class_methods.li");
example_test!(test_enums_basic, "enums_basic.li");
example_test!(test_enum_data, "enum_data.li");
example_test!(test_generic_enum, "generic_enum.li");
example_test!(test_try_operator, "try_operator.li");
example_test!(test_result_propagation, "result_propagation.li");
example_test!(test_impl_block, "impl_block.li");
example_test!(test_generic_methods, "generic_methods.li");

// Advanced features
example_test!(test_null_and_optionals, "null_and_optionals.li");
example_test!(test_generics_basic, "generics_basic.li");
example_test!(test_type_expressions, "type_expressions.li");
example_test!(test_optional_access, "optional_access.li");
example_test!(test_range_expressions, "range_expressions.li");

// I/O tests
example_test!(test_file_io, "file_io.li");
example_test!(test_stdlib_demo, "stdlib_demo.li");
example_test!(test_math_test, "math_test.li");
example_test!(test_smoke_test_fs, "smoke_test_fs.li");

// Standard library tests
example_test!(test_path_module, "test_path.li");
example_test!(test_random_module, "test_random.li");
example_test!(test_string_module, "test_string.li");
example_test!(test_hash_module, "test_hash.li");
example_test!(test_base64_module, "test_base64.li");
example_test!(test_json_module, "test_json.li");
example_test!(test_url_module, "test_url.li");
example_test!(test_net_module, "test_net.li");
example_test!(test_os_module, "test_os.li");
example_test!(test_math_module, "test_math.li");
example_test!(test_time_module, "test_time.li");
example_test!(test_collections_module, "test_collections.li");
example_test!(test_log_module, "test_log.li");
example_test!(test_test_module, "test_test.li");
example_test!(test_env_module, "test_env.li");
example_test!(test_uuid_module, "test_uuid.li");
example_test!(test_http_module, "test_http.li");
example_test!(test_regex_module, "test_regex.li");
example_test!(test_core_module, "test_core.li");
example_test!(test_io_module, "test_io.li");
example_test!(test_collections_deep_module, "test_collections_deep.li");

// std.sync concurrency primitives (Mutex, WaitGroup, Semaphore)
example_test!(test_sync_mutex_waitgroup, "sync_mutex_waitgroup.li");
example_test!(test_sync_semaphore, "sync_semaphore.li");
example_test!(test_sync_with_closure, "sync_with_closure.li");
example_test!(test_sync_try_lock, "sync_try_lock.li");

// Parser/lexer gaps: turbofish, free 'default' identifier, supertraits
example_test!(test_turbofish, "turbofish.li");
example_test!(test_default_identifier, "default_identifier.li");
example_test!(test_supertraits, "supertraits.li");

// =============================================================================
// Aggregate test that runs all examples
// =============================================================================

#[test]
fn test_all_examples_compile_and_run() {
    let files = get_example_files();
    let mut results = Vec::new();
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    println!("\n=== Lira Integration Tests ===\n");

    for path in &files {
        let result = test_example(path);
        if result.message == "SKIPPED" {
            skipped += 1;
            println!("  [SKIP] {}", result.name);
        } else if result.passed {
            passed += 1;
            println!("  [PASS] {} - {}", result.name, result.message);
        } else {
            failed += 1;
            println!("  [FAIL] {} - {}", result.name, result.message);
        }
        results.push(result);
    }

    println!("\n=== Summary ===");
    println!(
        "Total: {} | Passed: {} | Failed: {} | Skipped: {}",
        files.len(),
        passed,
        failed,
        skipped
    );

    // Don't fail the aggregate test - individual tests will show failures
    // This test is for getting a summary view
}

// =============================================================================
// Compilation-only tests (verify examples compile without running)
// =============================================================================

#[test]
fn test_all_examples_compile() {
    let files = get_example_files();
    let mut compile_errors = Vec::new();

    for path in &files {
        let source = fs::read_to_string(path).expect("Failed to read file");
        let source_path = path.to_string_lossy().to_string();

        // Check for skip marker
        if source.contains("// @skip") {
            continue;
        }

        // Check for expected error marker
        if source.contains("// @expect-error") {
            continue;
        }

        match lirac::compile_with_imports(&source_path, &source) {
            Ok(_) => {}
            Err(e) => {
                compile_errors.push(format!("{}: {}", path.display(), e));
            }
        }
    }

    if !compile_errors.is_empty() {
        panic!(
            "Compilation errors in {} files:\n{}",
            compile_errors.len(),
            compile_errors.join("\n")
        );
    }
}

// =============================================================================
// Type checker tests
// =============================================================================

#[test]
fn test_type_error_detection() {
    // Test that type errors are caught
    let bad_code = r#"
        let x: int = "not an int"
    "#;
    assert!(lirac::check(bad_code).is_err());
}

#[test]
fn test_undefined_variable_detection() {
    let bad_code = r#"
        println(undefined_var)
    "#;
    assert!(lirac::check(bad_code).is_err());
}

#[test]
fn test_function_arity_check() {
    let bad_code = r#"
        fn add(a: int, b: int) -> int { return a + b }
        add(1)  // missing argument
    "#;
    assert!(lirac::check(bad_code).is_err());
}

// =============================================================================
// BUG REGRESSION TESTS - TDD for stack underflow bug
// =============================================================================

#[test]
fn test_while_loop_in_function_with_string_concat() {
    // BUG: While loops in functions + string concatenation causes stack underflow
    let source = r#"
        fn count_to(n: int) -> int {
            var i = 0
            while i < n {
                i = i + 1
            }
            return i
        }

        println("Count: " + count_to(3))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let result = liravm::run_with_capture(&bytecode);
    assert!(
        result.is_ok(),
        "While loop + string concat should not cause stack underflow"
    );
    let (_, output) = result.unwrap();
    assert_eq!(output, vec!["Count: 3"]);
}

#[test]
fn test_factorial_with_string_concat() {
    // BUG: factorial with while loop + string concat
    let source = r#"
        fn factorial(n: int) -> int {
            if n <= 1 {
                return 1
            }
            var result = 1
            var i = 2
            while i <= n {
                result = result * i
                i = i + 1
            }
            return result
        }

        println("factorial(5): " + factorial(5))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let result = liravm::run_with_capture(&bytecode);
    assert!(result.is_ok(), "Factorial with string concat should work");
    let (_, output) = result.unwrap();
    assert_eq!(output, vec!["factorial(5): 120"]);
}

#[test]
fn test_multiple_while_function_calls_with_concat() {
    // BUG: Multiple calls to functions with while loops + concat
    let source = r#"
        fn count(n: int) -> int {
            var i = 0
            while i < n {
                i = i + 1
            }
            return i
        }

        println("First: " + count(3))
        println("Second: " + count(5))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let result = liravm::run_with_capture(&bytecode);
    assert!(result.is_ok(), "Multiple while function calls should work");
    let (_, output) = result.unwrap();
    assert_eq!(output, vec!["First: 3", "Second: 5"]);
}

// =============================================================================
// Specific feature tests
// =============================================================================

#[test]
fn test_untyped_parameter() {
    // Untyped parameters should be accepted and treated as Any.
    let source = r#"
        fn f(x) -> int { return 1 }
        println(f(5))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["1"]);
}

#[test]
fn test_untyped_parameter_body_uses_param() {
    // An untyped param used in the body must not trigger spurious type errors.
    let source = r#"
        fn id(x) { return x }
        println(id(42))
        println(id("hello"))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["42", "hello"]);
}

#[test]
fn test_import_std_json_roundtrip() {
    // stdlib/json.li uses untyped parameters; importing it and doing a
    // parse/stringify round-trip must compile and run.
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("_untyped_param_json_test.li");
    let source_path = source_path.to_string_lossy().to_string();
    let source = r#"
        import std.json
        let v = json_parse("{\"a\": 1}")
        println(json_stringify(v))
        println(json_has(v, "a"))
        println(json_get(v, "a"))
    "#;

    let bytecode = lirac::compile_with_imports(&source_path, source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec![r#"{"a":1}"#, "true", "1"]);
}

#[test]
fn test_basic_arithmetic() {
    let source = r#"
        println(1 + 2)
        println(10 - 3)
        println(4 * 5)
        println(20 / 4)
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["3", "7", "20", "5"]);
}

#[test]
fn test_string_concatenation() {
    let source = r#"
        let hello = "Hello, "
        let world = "World!"
        println(hello + world)
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["Hello, World!"]);
}

#[test]
fn test_if_else() {
    let source = r#"
        let x = 10
        if x > 5 {
            println("greater")
        } else {
            println("smaller")
        }
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["greater"]);
}

#[test]
fn test_while_loop() {
    let source = r#"
        var i = 0
        while i < 3 {
            println(i)
            i = i + 1
        }
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["0", "1", "2"]);
}

#[test]
fn test_function_definition_and_call() {
    let source = r#"
        fn double(x: int) -> int {
            return x * 2
        }
        println(double(21))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["42"]);
}

#[test]
fn test_recursive_function() {
    let source = r#"
        fn factorial(n: int) -> int {
            if n <= 1 {
                return 1
            }
            return n * factorial(n - 1)
        }
        println(factorial(5))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["120"]);
}

#[test]
fn test_array_operations() {
    let source = r#"
        let arr = [1, 2, 3, 4, 5]
        println(arr[0])
        println(arr[2])
        println(len(arr))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["1", "3", "5"]);
}

#[test]
fn test_struct_creation_and_access() {
    let source = r#"
        struct Point { x: int, y: int }
        let p = Point { x: 10, y: 20 }
        println(p.x)
        println(p.y)
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["10", "20"]);
}

#[test]
fn test_bitwise_operations() {
    let source = r#"
        println(12 & 10)
        println(12 | 10)
        println(12 ^ 10)
        println(1 << 4)
        println(16 >> 2)
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["8", "14", "6", "16", "4"]);
}

#[test]
fn test_compound_assignment() {
    let source = r#"
        var x = 10
        x += 5
        println(x)
        x -= 3
        println(x)
        x *= 2
        println(x)
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["15", "12", "24"]);
}

#[test]
fn test_lambda_expression() {
    let source = r#"
        let add = |a: int, b: int| a + b
        println(add(3, 4))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["7"]);
}

// =============================================================================
// Runtime error: source locations + readable messages
// =============================================================================

/// Source whose `a / b` (b == 0) divide is a runtime error the checker cannot
/// catch. The divide sits on line 4, column 5 (statement-start indentation).
const DIV_BY_ZERO_SOURCE: &str =
    "fn main() {\n    let a = 10\n    let b = 0\n    let c = a / b\n    print(c)\n}\n";

#[test]
fn test_runtime_error_message_is_location_prefixed() {
    let bytecode = lirac::compile(DIV_BY_ZERO_SOURCE).expect("Compilation failed");
    let err = liravm::run_with_capture(&bytecode)
        .expect_err("division by zero should be a runtime error");
    // First line stays byte-compatible: "line:col: Division by zero" (NOT Debug
    // format). The function-named call stack is appended below it.
    assert_eq!(err.lines().next(), Some("4:5: Division by zero"));
    // The failing function (`main`) is named in the appended call stack.
    assert!(
        err.contains("\n  at main"),
        "expected a call stack naming `main`, got:\n{}",
        err
    );
}

#[test]
fn test_runtime_error_structured_carries_location() {
    let bytecode = lirac::compile(DIV_BY_ZERO_SOURCE).expect("Compilation failed");
    let (_output, err) = liravm::run_with_capture_structured(&bytecode)
        .expect_err("division by zero should be a runtime error");
    assert_eq!(err.message, "Division by zero");
    assert_eq!(err.line, Some(4));
    assert_eq!(err.column, Some(5));
    // The structured error also carries the function-named call stack.
    assert_eq!(err.stack, vec!["main".to_string()]);
}

/// Call frames should be labelled with the real function name from debug info.
#[test]
fn test_call_frame_has_function_name() {
    let source = "\
fn greet(name: string) -> string {
    let msg = \"hi \" + name
    return msg
}

fn main() {
    let g = greet(\"world\")
    print(g)
}

main()
";
    let bytecode = lirac::compile(source).expect("Compilation failed");

    let session = liravm::DebugSession::new();
    session
        .load(source, bytecode)
        .expect("Failed to load program");
    session.set_breakpoints(vec![3]); // inside greet(): `return msg`
    session.start().expect("Failed to start");
    session
        .continue_execution()
        .expect("Failed to continue to breakpoint");

    let snapshot = session.get_snapshot().expect("No snapshot available");
    let names: Vec<Option<String>> = snapshot
        .call_stack
        .iter()
        .map(|f| f.function_name.clone())
        .collect();

    assert!(
        names.iter().any(|n| n.as_deref() == Some("greet")),
        "Expected a call frame named 'greet', got: {:?}",
        names
    );
}

#[test]
fn test_runtime_error_includes_function_named_call_stack() {
    // A runtime error (division by zero) raised inside `inner`, called by
    // `outer` via a tail call (`return inner()`). The tail-call optimisation
    // reuses `outer`'s frame for `inner` and updates the frame metadata,
    // so the stack correctly shows `inner` innermost, then `main`.
    let source = "\
fn inner() -> int {
    let z = 0
    return 1 / z
}

fn outer() -> int {
    return inner()
}

fn main() {
    print(outer())
}
";

    let bytecode = lirac::compile(source).expect("Compilation should succeed");
    let err = liravm::run_with_capture(&bytecode)
        .expect_err("Division by zero should produce a runtime error");

    // First line is unchanged from today: `line:col: message`.
    let first_line = err.lines().next().unwrap_or("");
    assert!(
        first_line.ends_with("Division by zero"),
        "First line should still be the location-prefixed message, got: {:?}",
        first_line
    );

    // inner is innermost, main is outermost. outer's frame was reused by TCO.
    let inner_pos = err.find("at inner").expect("stack should name `inner`");
    let main_pos = err.find("at main").expect("stack should name `main`");
    assert!(
        inner_pos < main_pos,
        "Stack should order inner before main, got:\n{}",
        err
    );
}
