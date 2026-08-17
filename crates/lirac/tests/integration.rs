//! Integration tests for Lira compiler and VM
//!
//! These tests verify that example programs compile and run correctly.
//! Expected output is parsed from comments in the source files:
//! - `// @expect: <output>` - expect this exact line in output
//! - `// @expect-contains: <text>` - output should contain this text
//! - `// @expect-error` - expect error at either compile or runtime
//! - `// @expect-compile-error` - error must come from the compiler
//! - `// @expect-runtime-error` - must compile but error at runtime
//! - `// @skip` - skip this test (for known issues)

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Test result for a single example file
#[derive(Debug)]
struct TestResult {
    name: String,
    passed: bool,
    message: String,
}

/// What kind of error is expected by the test
#[derive(Debug, Clone, Copy, PartialEq)]
enum ErrorExpectation {
    None,
    Any,
    Compile,
    Runtime,
}

/// Error from compile or runtime with stage info
struct StageError {
    message: String,
    is_compile: bool,
}

#[derive(Debug)]
struct LocalCrawlerReport {
    paths: Vec<String>,
    unknown_paths: Vec<String>,
    error: Option<String>,
}

struct LocalCrawlerServer {
    base_url: String,
    report_rx: Receiver<LocalCrawlerReport>,
    join: Option<JoinHandle<()>>,
}

impl LocalCrawlerServer {
    fn start(expected_requests: usize) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let (report_tx, report_rx) = mpsc::channel();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let unknown_paths = Arc::new(Mutex::new(Vec::new()));
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut error = None;
            loop {
                let path_count = paths
                    .lock()
                    .map(|paths| paths.len())
                    .unwrap_or(expected_requests);
                if path_count >= expected_requests {
                    break;
                }
                if Instant::now() >= deadline {
                    error = Some(format!(
                        "local crawler server timed out after {} of {} requests",
                        path_count, expected_requests
                    ));
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Err(connection_error) =
                            handle_local_crawler_connection(stream, &paths, &unknown_paths)
                        {
                            error = Some(connection_error);
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(accept_error) => {
                        error = Some(format!("local crawler accept failed: {accept_error}"));
                        break;
                    }
                }
            }
            let report = LocalCrawlerReport {
                paths: paths.lock().map(|paths| paths.clone()).unwrap_or_default(),
                unknown_paths: unknown_paths
                    .lock()
                    .map(|paths| paths.clone())
                    .unwrap_or_default(),
                error,
            };
            let _ = report_tx.send(report);
        });
        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}"),
            report_rx,
            join: Some(join),
        })
    }

    fn finish(mut self) -> Result<LocalCrawlerReport, String> {
        let report = self
            .report_rx
            .recv_timeout(Duration::from_secs(6))
            .map_err(|error| format!("local crawler server did not finish: {error}"))?;
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| "local crawler server panicked".to_string())?;
        }
        Ok(report)
    }
}

impl Drop for LocalCrawlerServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn handle_local_crawler_connection(
    mut stream: TcpStream,
    paths: &Arc<Mutex<Vec<String>>>,
    unknown_paths: &Arc<Mutex<Vec<String>>>,
) -> Result<(), String> {
    stream
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while request.len() < 8192 && !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        let remaining = 8192 - request.len();
        request.extend_from_slice(&chunk[..count.min(remaining)]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(|target| target.split('?').next().unwrap_or(target).to_string())
        .ok_or_else(|| "local crawler received malformed HTTP request".to_string())?;
    paths
        .lock()
        .map_err(|_| "local crawler path lock poisoned".to_string())?
        .push(path.clone());
    let (status, body) = match path.as_str() {
        "/" => (
            "200 OK",
            "<a href=\"/page/1\">1</a><a href=\"/page/2\">2</a><a href=\"/page/3\">3</a><a href=\"http://external.invalid/\">external</a><a href=\"/static/site.css\">asset</a><a href=\"#fragment\">fragment</a>",
        ),
        "/page/1" | "/page/2" | "/page/3" => (
            "200 OK",
            "<a href=\"/\">root</a><a href=\"/page/1\">1</a><a href=\"/page/2\">2</a><a href=\"/page/3\">3</a>",
        ),
        other => {
            unknown_paths
                .lock()
                .map_err(|_| "local crawler unknown-path lock poisoned".to_string())?
                .push(other.to_string());
            ("404 Not Found", "unknown")
        }
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

/// Parse expected output from source file comments
fn parse_expectations(source: &str) -> (Vec<String>, Vec<String>, ErrorExpectation, bool, bool) {
    let mut expect_lines = Vec::new();
    let mut expect_contains = Vec::new();
    let mut expect_error = ErrorExpectation::None;
    let mut skip = false;
    let mut local_crawler = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("// @expect-compile-error") {
            expect_error = ErrorExpectation::Compile;
        } else if trimmed.starts_with("// @expect-runtime-error") {
            expect_error = ErrorExpectation::Runtime;
        } else if trimmed.starts_with("// @expect:") {
            let value = trimmed.strip_prefix("// @expect:").unwrap().trim();
            expect_lines.push(value.to_string());
        } else if trimmed.starts_with("// @expect-contains:") {
            let value = trimmed.strip_prefix("// @expect-contains:").unwrap().trim();
            expect_contains.push(value.to_string());
        } else if trimmed.starts_with("// @expect-error") {
            expect_error = ErrorExpectation::Any;
        } else if trimmed.starts_with("// @skip") {
            skip = true;
        } else if trimmed.starts_with("// @test-local-crawler") {
            local_crawler = true;
        }
    }

    (
        expect_lines,
        expect_contains,
        expect_error,
        skip,
        local_crawler,
    )
}

fn compile_and_run_local_crawler(
    source_path: &str,
    source: &str,
) -> Result<(Vec<String>, LocalCrawlerReport), StageError> {
    let server = LocalCrawlerServer::start(4).map_err(|message| StageError {
        message,
        is_compile: false,
    })?;
    let base_url = server.base_url.clone();
    let result = (|| {
        let bytecode =
            lirac::compile_with_imports(source_path, source).map_err(|message| StageError {
                message,
                is_compile: true,
            })?;
        let program = liravm::bytecode::load(&bytecode).map_err(|message| StageError {
            message,
            is_compile: false,
        })?;
        let mut vm = liravm::VM::new(program);
        vm.set_fiber_mode(true);
        vm.set_capture_output(true);
        vm.set_env_override("LIRA_CRAWLER_BASE_URL", base_url);
        let deadline = Instant::now() + Duration::from_secs(5);
        vm.set_stop_check(move || Instant::now() >= deadline);
        match vm.run() {
            Ok(0) => Ok(vm.get_output().to_vec()),
            Ok(exit_code) => Err(StageError {
                message: format!("VM exited with status {exit_code}"),
                is_compile: false,
            }),
            Err(message) => Err(StageError {
                message,
                is_compile: false,
            }),
        }
    })();
    let report = server.finish().map_err(|message| StageError {
        message,
        is_compile: false,
    })?;
    match result {
        Ok(output) => Ok((output, report)),
        Err(error) => Err(error),
    }
}

/// Compile and run a Lira source file, returning captured output or stage-aware error
fn compile_and_run(source_path: &str) -> Result<Vec<String>, StageError> {
    let source = fs::read_to_string(source_path).map_err(|e| StageError {
        message: format!("Failed to read {}: {}", source_path, e),
        is_compile: false,
    })?;

    let bytecode = match lirac::compile_with_imports(source_path, &source) {
        Ok(bc) => bc,
        Err(e) => {
            return Err(StageError {
                message: e,
                is_compile: true,
            })
        }
    };

    match liravm::run_with_capture(&bytecode) {
        Ok((0, output)) => Ok(output),
        Ok((exit_code, output)) => Err(StageError {
            message: if output.is_empty() {
                format!("VM exited with status {exit_code}")
            } else {
                format!(
                    "VM exited with status {exit_code} after output:\n{}",
                    output.join("\n")
                )
            },
            is_compile: false,
        }),
        Err(e) => Err(StageError {
            message: e,
            is_compile: false,
        }),
    }
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

    let (expect_lines, expect_contains, expect_error, skip, local_crawler) =
        parse_expectations(&source);

    // Skip if marked
    if skip {
        return TestResult {
            name,
            passed: true,
            message: "SKIPPED".to_string(),
        };
    }

    let local_result = if local_crawler {
        Some(compile_and_run_local_crawler(&source_path, &source))
    } else {
        None
    };
    let result = match local_result {
        Some(Ok((output, report))) => {
            let expected = ["/", "/page/1", "/page/2", "/page/3"];
            let mut paths = report.paths.clone();
            paths.sort();
            let mut expected = expected.map(str::to_string).to_vec();
            expected.sort();
            if report.error.is_some() || !report.unknown_paths.is_empty() || paths != expected {
                return TestResult {
                    name,
                    passed: false,
                    message: format!("local crawler server report invalid: {report:?}"),
                };
            }
            Ok(output)
        }
        Some(Err(error)) => Err(error),
        None => compile_and_run(&source_path),
    };

    match result {
        Ok(output) => {
            match expect_error {
                ErrorExpectation::Compile => {
                    return TestResult {
                        name,
                        passed: false,
                        message: "Expected compiler error but succeeded".to_string(),
                    };
                }
                ErrorExpectation::Any => {
                    return TestResult {
                        name,
                        passed: false,
                        message: "Expected error (compile or runtime) but succeeded".to_string(),
                    };
                }
                ErrorExpectation::Runtime => {
                    return TestResult {
                        name,
                        passed: false,
                        message: "Expected runtime error but succeeded".to_string(),
                    };
                }
                ErrorExpectation::None => {}
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
            let passed = match expect_error {
                ErrorExpectation::Compile => e.is_compile,
                ErrorExpectation::Runtime => !e.is_compile,
                ErrorExpectation::Any => true,
                ErrorExpectation::None => false,
            };

            if passed {
                let stage = if e.is_compile { "compile" } else { "runtime" };
                TestResult {
                    name,
                    passed: true,
                    message: format!("Expected {} error (got it)", stage),
                }
            } else {
                let expected = match expect_error {
                    ErrorExpectation::Compile => "Expected compiler error but got runtime error",
                    ErrorExpectation::Runtime => "Expected runtime error but got compiler error",
                    ErrorExpectation::Any => unreachable!(),
                    ErrorExpectation::None => "",
                };
                let detail = if expect_error != ErrorExpectation::None {
                    format!("{}: {}", expected, e.message)
                } else {
                    format!("Error: {}", e.message)
                };
                TestResult {
                    name,
                    passed: false,
                    message: detail,
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
example_test!(test_match_range, "match_range.li");
example_test!(test_match_struct, "match_struct.li");
example_test!(test_match_or, "match_or.li");
example_test!(test_match_binding, "match_binding.li");

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
example_test!(test_class_this, "class_this.li");
example_test!(test_class_super, "class_super.li");
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
example_test!(test_file_seek, "file_seek.li");
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

// Additional example files
example_test!(test_all_binary_ops, "all_binary_ops.li");
example_test!(test_block_expressions, "block_expressions.li");
example_test!(test_char_literals, "char_literals.li");
example_test!(test_concurrent_crawler, "concurrent_crawler.li");
example_test!(test_const_declarations, "const_declarations.li");
example_test!(test_cycle_auto_gc, "cycle_auto_gc.li");
example_test!(test_cycle_stress, "cycle_stress.li");
example_test!(test_default_params, "default_params.li");
example_test!(test_empty_array_annotation, "empty_array_annotation.li");
example_test!(test_factorial_debug, "factorial_debug.li");
example_test!(test_function_types, "function_types.li");
example_test!(test_if_expressions, "if_expressions.li");
example_test!(test_interface_basic, "interface_basic.li");
example_test!(test_loop_control, "loop_control.li");
example_test!(test_main_entry_point, "main_entry_point.li");
example_test!(test_map_literals, "map_literals.li");
example_test!(test_match_exhaustive_enum, "match_exhaustive_enum.li");
example_test!(
    test_match_non_exhaustive_enum,
    "match_non_exhaustive_enum.li"
);
example_test!(test_math_inf_nan, "math_inf_nan.li");
example_test!(test_method_chaining, "method_chaining.li");
example_test!(test_named_arguments, "named_arguments.li");
example_test!(test_optional_chaining, "optional_chaining.li");
example_test!(test_power_operator, "power_operator.li");
example_test!(test_spawn_expression, "spawn_expression.li");
example_test!(test_static_method_params, "static_method_params.li");
example_test!(test_string_escapes, "string_escapes.li");
example_test!(test_traits_basic, "traits_basic.li");
example_test!(test_tuple_types, "tuple_types.li");
example_test!(test_type_alias, "type_alias.li");
example_test!(test_unary_operators, "unary_operators.li");

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

        // Check for expected compile-error marker
        if source.contains("// @expect-compile-error") {
            continue;
        }

        // Check for expected error marker (backward compat: could be compile or runtime)
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
// Sample tests (tests/samples/)
// =============================================================================

/// Get all .li files in the tests/samples directory
fn get_sample_files() -> Vec<std::path::PathBuf> {
    let samples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("samples");

    let mut files: Vec<_> = match fs::read_dir(&samples_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "li"))
            .collect(),
        Err(_) => {
            println!("Warning: tests/samples directory not found");
            return Vec::new();
        }
    };

    files.sort();
    files
}

macro_rules! sample_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let samples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("tests")
                .join("samples");
            let path = samples_dir.join($file);
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

sample_test!(test_sample_hello, "hello.li");
sample_test!(test_sample_arithmetic, "arithmetic.li");
sample_test!(test_sample_control_flow, "control_flow.li");
sample_test!(test_sample_fibers_basic, "fibers-basic.li");
sample_test!(test_sample_ping_pong, "ping-pong.li");
sample_test!(test_sample_producer_consumer, "producer-consumer.li");
sample_test!(test_sample_worker_pool, "worker-pool.li");
sample_test!(test_sample_parallel_sum, "parallel-sum.li");
sample_test!(test_sample_countdown, "countdown.li");

#[test]
fn test_all_samples_compile_and_run() {
    let files = get_sample_files();
    if files.is_empty() {
        println!("No sample files found, skipping");
        return;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    println!("\n=== Lira Sample Tests ===\n");

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
    }

    println!("\n=== Samples Summary ===");
    println!(
        "Total: {} | Passed: {} | Failed: {} | Skipped: {}",
        files.len(),
        passed,
        failed,
        skipped
    );
}

#[test]
fn test_all_samples_compile() {
    let files = get_sample_files();
    let mut compile_errors = Vec::new();

    for path in &files {
        let source = fs::read_to_string(path).expect("Failed to read file");
        let source_path = path.to_string_lossy().to_string();

        if source.contains("// @skip") {
            continue;
        }
        if source.contains("// @expect-compile-error") {
            continue;
        }
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
            "Compilation errors in {} sample files:\n{}",
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
fn test_function_implicit_tail_expression_returns_value() {
    // A function body's final expression is its implicit return.  This uses a
    // match expression because it exercises the value-producing control-flow
    // lowering that previously got popped before the bytecode null fallback.
    let source = r#"
        fn describe(x: int) -> string {
            match x {
                0 => "zero"
                1 => "one"
                _ => "other"
            }
        }

        println(describe(0))
        println(describe(1))
        println(describe(42))
    "#;

    let bytecode = lirac::compile(source).expect("Compilation failed");
    let (_, output) = liravm::run_with_capture(&bytecode).expect("Execution failed");

    assert_eq!(output, vec!["zero", "one", "other"]);
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

#[test]
fn array_pop_global_and_method_forms_preserve_optional_empty_semantics() {
    let source = r#"
        fn id(value) { return value }

        fn main() {
            let dynamic = id([1])
            println(pop(dynamic))
            println(pop(dynamic))
            println(len(dynamic))

            let typed: [string] = ["word"]
            println(typed.pop())
            println(typed.pop())
            println(len(typed))
        }
    "#;
    let bytecode = lirac::compile(source).expect("pop source should compile");
    let (exit_code, output) = liravm::run_with_capture(&bytecode).expect("VM should execute");
    assert_eq!(exit_code, 0);
    assert_eq!(output, ["1", "null", "0", "word", "null", "0"]);
}
