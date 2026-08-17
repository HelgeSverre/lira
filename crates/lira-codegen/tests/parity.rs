//! Cross-backend parity: every example the native backend accepts must produce
//! exactly the output the bytecode VM produces.
//!
//! The two backends share a front end but nothing else — one interprets tagged
//! values, the other emits unboxed machine code — so agreeing on output is the
//! strongest evidence that the lowering is faithful.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

mod common;

/// Examples that both backends must agree on. Adding a construct to the backend
/// should mean adding entries here.
//
// Output parity intentionally excludes only fixtures whose observable result
// depends on external state or nondeterministic scheduling: file, environment,
// network, clock, and random/UUID examples; fiber stress/concurrency demos;
// intentional runtime-error fixtures; and large cycle/recursion stress tests.
// These are execution-scope exclusions, never feature waivers.
// These examples are still required to compile to native objects by the
// exhaustive frontend-valid gate below.
const PARITY_EXAMPLES: &[&str] = &[
    "all_binary_ops.li",
    "array_ops.li",
    "array_types.li",
    "arithmetic_edge_cases.li",
    "bitwise_ops.li",
    "block_expressions.li",
    "channel_basic.li",
    "char_literals.li",
    "class_inheritance.li",
    "class_methods.li",
    "class_super.li",
    "class_this.li",
    "classes_basic.li",
    "compound_assign.li",
    "const_declarations.li",
    "control_flow.li",
    "control_flow_test.li",
    "default_identifier.li",
    "default_params.li",
    "empty_array_annotation.li",
    "enum_data.li",
    "enums_basic.li",
    "factorial.li",
    "factorial_debug.li",
    "fiber_basic.li",
    "fibonacci.li",
    "for_loop.li",
    "function_types.li",
    "generic_enum.li",
    "generic_methods.li",
    "generics_basic.li",
    "hello.li",
    "if_expressions.li",
    "impl_block.li",
    "import_selective.li",
    "integer_types.li",
    "interface_basic.li",
    "lambda.li",
    "loop_control.li",
    "loop_infinite.li",
    "main_entry_point.li",
    "map_literals.li",
    "match_binding.li",
    "match_exhaustive_enum.li",
    "match_or.li",
    "match_range.li",
    "match_struct.li",
    "math_inf_nan.li",
    "math_test.li",
    "method_chaining.li",
    "method_named_args.li",
    "mutual_recursion.li",
    "named_args.li",
    "named_args_defaults.li",
    "named_arguments.li",
    "nested_structures.li",
    "null_and_optionals.li",
    "operator_comprehensive.li",
    "optional_access.li",
    "optional_chaining.li",
    "pattern_constructor.li",
    "pattern_constructor_verify.li",
    "pattern_guards.li",
    "pattern_match.li",
    "pattern_tuple.li",
    "pattern_tuple_literals.li",
    "pattern_tuple_simple.li",
    "power_operator.li",
    "prime_checker.li",
    "range_expressions.li",
    "result_propagation.li",
    "select_basic.li",
    "static_method_params.li",
    "string_escapes.li",
    "string_interpolation.li",
    "string_ops.li",
    "structs.li",
    "supertraits.li",
    "sync_mutex_waitgroup.li",
    "sync_semaphore.li",
    "sync_try_lock.li",
    "sync_with_closure.li",
    "tail_call_regressions.li",
    "test_collections.li",
    "test_collections_deep.li",
    "test_core.li",
    "test_base64.li",
    "test_hash.li",
    "test_http.li",
    "test_json.li",
    "test_math.li",
    "test_path.li",
    "test_regex.li",
    "test_string.li",
    "test_test.li",
    "test_url.li",
    "traits_basic.li",
    "try_operator.li",
    "tuple_types.li",
    "turbofish.li",
    "type_alias.li",
    "type_expressions.li",
    "unary_operators.li",
    "untyped_function_ops.li",
];

const MAX_CORPUS_DEPTH: usize = 32;
const MAX_CORPUS_FILES: usize = 4096;
const MAX_CORPUS_ENTRIES: usize = 16384;

#[derive(Debug, Clone)]
struct FrontendValidExample {
    name: String,
    path: PathBuf,
    source: String,
    directives: SourceDirectives,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn examples_dir() -> PathBuf {
    repo_root().join("examples")
}

fn collect_lira_files(
    dir: &Path,
    depth: usize,
    entries_seen: &mut usize,
    files: &mut Vec<PathBuf>,
    failures: &mut Vec<String>,
) {
    if depth > MAX_CORPUS_DEPTH {
        failures.push(format!(
            "{}: corpus traversal exceeded the maximum depth of {}",
            dir.display(),
            MAX_CORPUS_DEPTH
        ));
        return;
    }
    let metadata = match std::fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) => {
            failures.push(format!(
                "{}: corpus entry metadata could not be read: {}",
                dir.display(),
                error
            ));
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        failures.push(format!(
            "{}: symlink ignored by corpus traversal",
            dir.display()
        ));
        return;
    }
    if !metadata.file_type().is_dir() {
        failures.push(format!("{}: corpus root is not a directory", dir.display()));
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(format!(
                "{}: could not read source directory: {}",
                dir.display(),
                error
            ));
            return;
        }
    };

    let mut entry_paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => entry_paths.push(entry),
            Err(error) => failures.push(format!(
                "{}: directory entry could not be read: {}",
                dir.display(),
                error
            )),
        }
    }
    let mut entries = entry_paths;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        *entries_seen = (*entries_seen).saturating_add(1);
        if *entries_seen > MAX_CORPUS_ENTRIES {
            failures.push(format!(
                "{}: corpus traversal exceeded the maximum of {} entries",
                dir.display(),
                MAX_CORPUS_ENTRIES
            ));
            break;
        }
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                failures.push(format!(
                    "{}: file type could not be read: {}",
                    path.display(),
                    error
                ));
                continue;
            }
        };
        if file_type.is_symlink() {
            failures.push(format!(
                "{}: symlink ignored by corpus traversal",
                path.display()
            ));
        } else if file_type.is_dir() {
            collect_lira_files(&path, depth + 1, entries_seen, files, failures);
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("li")
        {
            if files.len() >= MAX_CORPUS_FILES {
                failures.push(format!(
                    "{}: corpus traversal exceeded the maximum of {} .li files",
                    path.display(),
                    MAX_CORPUS_FILES
                ));
                break;
            }
            files.push(path);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("li") {
            failures.push(format!(
                "{}: non-regular .li entry ignored by corpus traversal",
                path.display()
            ));
        }
    }
}

fn lira_files_under(dir: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut failures = Vec::new();
    let mut entries_seen = 0;
    collect_lira_files(dir, 0, &mut entries_seen, &mut files, &mut failures);
    files.sort();
    failures.sort();
    (files, failures)
}

/// Run one example on the bytecode VM.
fn without_final_line_ending(output: &str) -> &str {
    match output.strip_suffix('\n') {
        Some(output) => output.strip_suffix('\r').unwrap_or(output),
        None => output,
    }
}

#[test]
fn output_normalization_removes_only_one_line_ending() {
    assert_eq!(without_final_line_ending("value\n"), "value");
    assert_eq!(without_final_line_ending("value\r\n"), "value");
    assert_eq!(without_final_line_ending("value  \n"), "value  ");
    assert_eq!(without_final_line_ending("value\n\n"), "value\n");
    assert_eq!(without_final_line_ending("value  "), "value  ");
}

#[test]
fn exact_output_directives_require_complete_line_count() {
    let directives = SourceDirectives {
        exact: vec!["first".to_owned(), "second".to_owned()],
        ..SourceDirectives::default()
    };
    let mut failures = Vec::new();
    validate_backend_output(
        "sample.li",
        "AOT",
        "first\nsecond",
        &directives,
        &mut failures,
    );
    assert!(failures.is_empty());

    let mut failures = Vec::new();
    validate_backend_output(
        "sample.li",
        "AOT",
        "first\nsecond\nextra",
        &directives,
        &mut failures,
    );
    assert_eq!(failures.len(), 1);

    let mut failures = Vec::new();
    validate_backend_output("sample.li", "AOT", "first", &directives, &mut failures);
    assert_eq!(failures.len(), 1);
}

fn run_bytecode(path: &Path, source: &str) -> Result<String, String> {
    match common::run_vm_capture(path, source)? {
        common::VmRunOutcome::Success { status: 0, output } => String::from_utf8(output)
            .map(|output| without_final_line_ending(&output).to_owned())
            .map_err(|error| error.to_string()),
        common::VmRunOutcome::Success { status, .. } => {
            Err(format!("bytecode VM exited with status {status}"))
        }
        common::VmRunOutcome::CompileError(error) => {
            Err(format!("bytecode compilation failed: {error}"))
        }
        common::VmRunOutcome::RuntimeError { message, .. } => {
            Err(format!("bytecode VM runtime error: {message}"))
        }
    }
}

/// Compile one example to a native executable and run it.
fn run_native(path: &Path, source: &str) -> Result<String, String> {
    let output = common::run_aot(path, source)?;
    output.assert_complete_output()?;
    if !output.status.success() {
        return Err(format!(
            "native executable exited with {}: {}",
            output.status,
            output.stderr_text().trim_end()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("native executable stdout is not valid UTF-8: {error}"))?;
    Ok(without_final_line_ending(&stdout).to_owned())
}

fn run_jit(path: &Path, source: &str) -> Result<String, String> {
    let (status, stdout) = common::run_jit_capture(path.to_str().expect("utf-8 path"), source)?;
    if status != 0 {
        return Err(format!("JIT exited with status {status}"));
    }
    let stdout = String::from_utf8(stdout)
        .map_err(|error| format!("JIT stdout is not valid UTF-8: {error}"))?;
    Ok(without_final_line_ending(&stdout).to_owned())
}

/// Run `task` over every item on a small worker pool and concatenate the
/// per-item failure lists in input order.
///
/// Without this the exhaustive parity gates serialize thousands of native
/// builds and runs inside a single test thread. The pool is sized to the same
/// bounded lane count the helper children use, so the parent threads never
/// outrun the global concurrency cap: at most `execution_lane_count` bounded
/// children exist at once on the machine while every example is still checked.
fn run_items_parallel<T: Sync>(
    items: &[T],
    task: impl Fn(&T) -> Vec<String> + Sync,
) -> Vec<String> {
    if items.is_empty() {
        return Vec::new();
    }
    // One worker when there is nothing to gain: a single item, or a pool that
    // cannot run more than one child anyway.
    let workers = common::execution_lane_count().min(items.len().max(1));
    if workers <= 1 {
        return items.iter().flat_map(task).collect();
    }

    let next = Mutex::new(0usize);
    let results: Vec<Mutex<Option<Vec<String>>>> =
        (0..items.len()).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = {
                    let mut next = next.lock().expect("work-queue mutex not poisoned");
                    let index = *next;
                    *next += 1;
                    index
                };
                if index >= items.len() {
                    break;
                }
                let failures = task(&items[index]);
                *results[index]
                    .lock()
                    .expect("result-slot mutex not poisoned") = Some(failures);
            });
        }
    });
    results
        .into_iter()
        .flat_map(|slot| {
            // Mutex::into_inner returns Result<Option<Vec<String>>, _>; the
            // first flatten collapses the Option layer, the second the Vec.
            slot.into_inner()
                .expect("every item was processed by a worker")
        })
        .flatten()
        .collect()
}

fn run_local_crawler_vm(path: &Path, source: &str, base_url: &str) -> Result<String, String> {
    match common::run_vm_capture_with_env(path, source, "LIRA_CRAWLER_BASE_URL", base_url)? {
        common::VmRunOutcome::Success { status: 0, output } => String::from_utf8(output)
            .map(|output| without_final_line_ending(&output).to_owned())
            .map_err(|error| error.to_string()),
        common::VmRunOutcome::Success { status, .. } => {
            Err(format!("bytecode VM exited with status {status}"))
        }
        common::VmRunOutcome::CompileError(error) => {
            Err(format!("bytecode compilation failed: {error}"))
        }
        common::VmRunOutcome::RuntimeError { message, .. } => {
            Err(format!("bytecode VM runtime error: {message}"))
        }
    }
}

type LocalCrawlerRuns = (
    Result<String, String>,
    Result<String, String>,
    Result<String, String>,
    Result<common::LocalCrawlerReport, String>,
);

fn run_local_crawler(path: &Path, source: &str) -> LocalCrawlerRuns {
    let server = match common::LocalCrawlerServer::start(12) {
        Ok(server) => server,
        Err(error) => {
            let error = Err(error);
            return (
                error.clone(),
                error.clone(),
                error,
                Err("server did not start".to_string()),
            );
        }
    };
    let base_url = server.base_url.clone();
    let vm = run_local_crawler_vm(path, source, &base_url);
    let aot = common::run_aot_with_env(path, source, "LIRA_CRAWLER_BASE_URL", &base_url).and_then(
        |output| {
            output.assert_complete_output()?;
            if !output.status.success() {
                return Err(format!(
                    "native executable exited with {}: {}",
                    output.status,
                    output.stderr_text().trim_end()
                ));
            }
            let stdout = String::from_utf8(output.stdout)
                .map_err(|error| format!("native executable stdout is not valid UTF-8: {error}"))?;
            Ok(without_final_line_ending(&stdout).to_owned())
        },
    );
    let jit = common::run_jit_capture_with_env(
        path.to_str().expect("utf-8 path"),
        source,
        "LIRA_CRAWLER_BASE_URL",
        &base_url,
    )
    .and_then(|(status, stdout)| {
        if status != 0 {
            return Err(format!("JIT exited with status {status}"));
        }
        let stdout = String::from_utf8(stdout)
            .map_err(|error| format!("JIT stdout is not valid UTF-8: {error}"))?;
        Ok(without_final_line_ending(&stdout).to_owned())
    });
    let report = server.finish();
    (vm, aot, jit, report)
}

fn frontend_valid_examples() -> (Vec<FrontendValidExample>, Vec<String>) {
    let mut examples = Vec::new();
    let mut failures = Vec::new();
    let mut files = Vec::new();
    for relative_dir in ["examples", "tests/samples"] {
        let (mut dir_files, mut dir_failures) = lira_files_under(&repo_root().join(relative_dir));
        files.append(&mut dir_files);
        failures.append(&mut dir_failures);
    }
    files.sort();

    for path in files {
        let name = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        let source = match common::read_source_bounded(&path) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!("{}: could not read: {}", name, error));
                continue;
            }
        };

        let directives = source_directives(&source);

        // Sources rejected by the front end are diagnostic fixtures, not
        // frontend-valid programs and therefore do not enter this gate.
        let preflight = match common::run_frontend_preflight(&path, &source) {
            Ok(preflight) => preflight,
            Err(error) => {
                failures.push(format!("{}: frontend preflight failed: {}", name, error));
                continue;
            }
        };
        match preflight {
            common::FrontendPreflightOutcome::Accepted if directives.expect_compile_error => {
                failures.push(format!(
                    "{}: marked @expect-compile-error but the front end accepted it",
                    name
                ));
                continue;
            }
            common::FrontendPreflightOutcome::Accepted => {}
            common::FrontendPreflightOutcome::CompileError(_diagnostics)
                if directives.expect_compile_error || directives.expect_error =>
            {
                // Explicit error fixtures are expected to be rejected before
                // execution. @expect-error also permits a compile-time error,
                // matching the compiler integration test convention.
                continue;
            }
            common::FrontendPreflightOutcome::CompileError(diagnostics) => {
                let diagnostics = diagnostics
                    .iter()
                    .map(|diagnostic| {
                        format!(
                            "{}:{}: {}",
                            diagnostic.line, diagnostic.column, diagnostic.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                failures.push(format!(
                    "{}: front end rejected an unmarked source: {}",
                    name, diagnostics
                ));
                continue;
            }
        }
        examples.push(FrontendValidExample {
            name,
            path,
            source,
            directives,
        });
    }

    examples.sort_by(|left, right| left.name.cmp(&right.name));
    failures.sort();
    (examples, failures)
}

#[derive(Debug, Default, Clone)]
struct SourceDirectives {
    exact: Vec<String>,
    contains: Vec<String>,
    expect_error: bool,
    expect_compile_error: bool,
    expect_runtime_error: bool,
    skip: bool,
    local_crawler: bool,
}

fn source_directives(source: &str) -> SourceDirectives {
    let mut directives = SourceDirectives::default();
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("// @expect-compile-error") {
            directives.expect_compile_error = true;
        } else if line.starts_with("// @expect-runtime-error") {
            directives.expect_runtime_error = true;
        } else if line.starts_with("// @expect-error") {
            directives.expect_error = true;
        } else if line.starts_with("// @skip") {
            directives.skip = true;
        } else if line.starts_with("// @test-local-crawler") {
            directives.local_crawler = true;
        } else if let Some(value) = line.strip_prefix("// @expect:") {
            directives.exact.push(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("// @expect-contains:") {
            directives.contains.push(value.trim().to_string());
        }
    }
    directives
}

fn output_expectations(source: &str) -> SourceDirectives {
    source_directives(source)
}

fn validate_backend_output(
    name: &str,
    backend: &str,
    output: &str,
    directives: &SourceDirectives,
    failures: &mut Vec<String>,
) {
    if !directives.exact.is_empty() {
        let expected = directives.exact.join("\n");
        if output != expected {
            failures.push(format!(
                "{}: {} output differs from complete @expect directives\n  expected: {:?}\n  actual:   {:?}",
                name, backend, expected, output
            ));
        }
    }
    for expected in &directives.contains {
        if !output.contains(expected) {
            failures.push(format!(
                "{}: {} output does not contain {:?}\n  {}: {:?}",
                name, backend, expected, backend, output
            ));
        }
    }
}

fn validate_backend_result(
    name: &str,
    backend: &str,
    result: Result<String, String>,
    directives: &SourceDirectives,
    failures: &mut Vec<String>,
) {
    let expects_runtime_error = directives.expect_error || directives.expect_runtime_error;
    match (expects_runtime_error, result) {
        (true, Ok(output)) => failures.push(format!(
            "{}: {} succeeded but @expect-error requires a failure (output: {:?})",
            name, backend, output
        )),
        // A plain `Err(String)` cannot prove that the program itself failed:
        // the bounded native helpers use the same channel for timeout,
        // memory, output, monitoring, and wrapper failures. Fail closed until
        // a stage-typed outcome is available for every backend.
        (true, Err(error)) => failures.push(format!(
            "{}: {} returned an unclassified error while a program runtime error was expected: {}",
            name, backend, error
        )),
        (false, Ok(output)) => {
            validate_backend_output(name, backend, &output, directives, failures)
        }
        (false, Err(error)) => {
            failures.push(format!("{}: {} execution failed: {}", name, backend, error))
        }
    }
}

fn validate_local_crawler_report(
    name: &str,
    report: Result<common::LocalCrawlerReport, String>,
    failures: &mut Vec<String>,
) {
    match report {
        Ok(report) => {
            let expected = ["/", "/page/1", "/page/2", "/page/3"];
            let mut counts = expected
                .iter()
                .map(|path| {
                    (
                        *path,
                        report
                            .paths
                            .iter()
                            .filter(|seen| seen.as_str() == *path)
                            .count(),
                    )
                })
                .collect::<Vec<_>>();
            counts.sort_by_key(|(path, _)| *path);
            if report.error.is_some()
                || !report.unknown_paths.is_empty()
                || counts.iter().any(|(_, count)| *count != 3)
                || report.paths.len() != 12
            {
                failures.push(format!(
                    "{}: local crawler request report invalid: {:?}, counts: {:?}",
                    name, report, counts
                ));
            }
        }
        Err(error) => failures.push(format!("{name}: local crawler server failed: {error}")),
    }
}

#[test]
fn concurrent_crawler_is_hermetic_across_vm_aot_and_jit() {
    let path = examples_dir().join("concurrent_crawler.li");
    let source = common::read_source_bounded(&path).expect("read concurrent crawler example");
    let directives = source_directives(&source);
    assert!(directives.local_crawler);
    assert!(!directives.skip);

    let (vm, aot, jit, report) = run_local_crawler(&path, &source);
    let mut failures = Vec::new();
    validate_backend_result(
        "examples/concurrent_crawler.li",
        "VM",
        vm,
        &directives,
        &mut failures,
    );
    validate_backend_result(
        "examples/concurrent_crawler.li",
        "AOT",
        aot,
        &directives,
        &mut failures,
    );
    validate_backend_result(
        "examples/concurrent_crawler.li",
        "JIT",
        jit,
        &directives,
        &mut failures,
    );
    validate_local_crawler_report("examples/concurrent_crawler.li", report, &mut failures);
    assert!(
        failures.is_empty(),
        "hermetic crawler parity failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn unreachable_tcp_connect_returns_within_the_bounded_vm_aot_jit_gate() {
    let path = examples_dir().join("test_net.li");
    let source = common::read_source_bounded(&path).expect("read network example");
    let directives = source_directives(&source);
    let mut failures = Vec::new();
    validate_backend_result(
        "examples/test_net.li",
        "VM",
        run_bytecode(&path, &source),
        &directives,
        &mut failures,
    );
    validate_backend_result(
        "examples/test_net.li",
        "AOT",
        run_native(&path, &source),
        &directives,
        &mut failures,
    );
    validate_backend_result(
        "examples/test_net.li",
        "JIT",
        run_jit(&path, &source),
        &directives,
        &mut failures,
    );
    assert!(
        failures.is_empty(),
        "bounded network example failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn frontend_preflight_resolves_imports_using_the_logical_source_path() {
    let path = examples_dir().join("import_selective.li");
    let source = common::read_source_bounded(&path).expect("read import example");
    let outcome = common::run_frontend_preflight(&path, &source)
        .expect("frontend preflight child should return a typed outcome");
    assert_eq!(outcome, common::FrontendPreflightOutcome::Accepted);
}

#[test]
fn every_frontend_valid_example_executes_on_vm_aot_and_jit_and_matches_directives() {
    let (examples, mut failures) = frontend_valid_examples();
    let skipped = examples
        .iter()
        .filter(|example| example.directives.skip)
        .map(|example| example.name.clone())
        .collect::<Vec<_>>();
    if !skipped.is_empty() {
        failures.push(format!(
            "frontend-valid examples must not contain @skip: {skipped:?}"
        ));
    }

    let example_failures = run_items_parallel(&examples, |example| {
        // Run all three backends even after one fails so the aggregate report
        // identifies every broken backend and every broken source in one run.
        if example.directives.local_crawler {
            let (vm, aot, jit, report) = run_local_crawler(&example.path, &example.source);
            let mut failures = Vec::new();
            validate_backend_result(&example.name, "VM", vm, &example.directives, &mut failures);
            validate_backend_result(
                &example.name,
                "AOT",
                aot,
                &example.directives,
                &mut failures,
            );
            validate_backend_result(
                &example.name,
                "JIT",
                jit,
                &example.directives,
                &mut failures,
            );
            validate_local_crawler_report(&example.name, report, &mut failures);
            return failures;
        }

        let vm = run_bytecode(&example.path, &example.source);
        let aot = run_native(&example.path, &example.source);
        let jit = run_jit(&example.path, &example.source);

        let mut failures = Vec::new();
        validate_backend_result(&example.name, "VM", vm, &example.directives, &mut failures);
        validate_backend_result(
            &example.name,
            "AOT",
            aot,
            &example.directives,
            &mut failures,
        );
        validate_backend_result(
            &example.name,
            "JIT",
            jit,
            &example.directives,
            &mut failures,
        );
        failures
    });
    failures.extend(example_failures);

    failures.sort();
    assert!(
        failures.is_empty(),
        "every frontend-valid example must execute on VM, bounded AOT, and bounded JIT and satisfy its directives:\n{}",
        failures.join("\n")
    );
}

#[test]
fn samples_execute_on_aot_and_match_their_directives() {
    let (files, mut failures) = lira_files_under(&repo_root().join("tests/samples"));
    let sample_failures = run_items_parallel(&files, |path| {
        let mut failures = Vec::new();
        let name = path
            .strip_prefix(repo_root())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let source = match common::read_source_bounded(path) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!("{}: could not read: {}", name, error));
                return failures;
            }
        };
        let expectations = output_expectations(&source);
        if expectations.exact.is_empty() && expectations.contains.is_empty() {
            failures.push(format!("{}: no output directives found", name));
            return failures;
        }

        let native = match run_native(path, &source) {
            Ok(output) => output,
            Err(error) => {
                failures.push(format!("{}: native execution failed: {}", name, error));
                return failures;
            }
        };

        if !expectations.exact.is_empty() {
            let expected = expectations.exact.join("\n");
            if native != expected {
                failures.push(format!(
                    "{}: native output differs from exact directives\n  expected: {:?}\n  native:   {:?}",
                    name, expected, native
                ));
            }

            match run_bytecode(path, &source) {
                Ok(bytecode) if bytecode == native => {}
                Ok(bytecode) => failures.push(format!(
                    "{}: deterministic sample differs between VM and native\n  bytecode: {:?}\n  native:   {:?}",
                    name, bytecode, native
                )),
                Err(error) => failures.push(format!("{}: bytecode VM failed: {}", name, error)),
            }
        }

        let native_output = native.as_str();
        for expected in expectations.contains {
            if !native_output.contains(&expected) {
                failures.push(format!(
                    "{}: native output does not contain {:?}\n  native: {:?}",
                    name, expected, native_output
                ));
            }
        }
        failures
    });
    failures.extend(sample_failures);

    failures.sort();
    assert!(
        failures.is_empty(),
        "sample AOT execution must satisfy output directives:\n{}",
        failures.join("\n")
    );
}

#[test]
fn samples_execute_on_jit_and_match_their_directives() {
    let (files, mut failures) = lira_files_under(&repo_root().join("tests/samples"));
    let sample_failures = run_items_parallel(&files, |path| {
        let mut failures = Vec::new();
        let name = path
            .strip_prefix(repo_root())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let source = match common::read_source_bounded(path) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!("{}: could not read: {}", name, error));
                return failures;
            }
        };
        let expectations = output_expectations(&source);
        if expectations.exact.is_empty() && expectations.contains.is_empty() {
            failures.push(format!("{}: no output directives found", name));
            return failures;
        }

        let jit = match run_jit(path, &source) {
            Ok(output) => output,
            Err(error) => {
                failures.push(format!("{}: JIT execution failed: {}", name, error));
                return failures;
            }
        };

        if !expectations.exact.is_empty() {
            let expected = expectations.exact.join("\n");
            if jit != expected {
                failures.push(format!(
                    "{}: JIT output differs from exact directives\n  expected: {:?}\n  JIT:      {:?}",
                    name, expected, jit
                ));
            }

            match run_bytecode(path, &source) {
                Ok(bytecode) if bytecode == jit => {}
                Ok(bytecode) => failures.push(format!(
                    "{}: deterministic sample differs between VM and JIT\n  bytecode: {:?}\n  JIT:      {:?}",
                    name, bytecode, jit
                )),
                Err(error) => failures.push(format!("{}: bytecode VM failed: {}", name, error)),
            }
        }

        let jit_output = jit.as_str();
        for expected in expectations.contains {
            if !jit_output.contains(&expected) {
                failures.push(format!(
                    "{}: JIT output does not contain {:?}\n  JIT: {:?}",
                    name, expected, jit_output
                ));
            }
        }
        failures
    });
    failures.extend(sample_failures);

    failures.sort();
    assert!(
        failures.is_empty(),
        "sample JIT execution must satisfy output directives:\n{}",
        failures.join("\n")
    );
}

#[test]
fn listed_examples_produce_identical_output_on_both_backends() {
    let dir = examples_dir();
    let example_failures = run_items_parallel(PARITY_EXAMPLES, |name| {
        let name = *name;
        let mut failures = Vec::new();
        let path = dir.join(name);
        let source = match common::read_source_bounded(&path) {
            Ok(source) => source,
            Err(e) => {
                failures.push(format!("{}: could not read: {}", name, e));
                return failures;
            }
        };

        let expected = match run_bytecode(&path, &source) {
            Ok(output) => output,
            Err(e) => {
                failures.push(format!("{}: the bytecode VM failed: {}", name, e));
                return failures;
            }
        };
        let actual = match run_native(&path, &source) {
            Ok(output) => output,
            Err(e) => {
                failures.push(format!("{}: the native backend failed: {}", name, e));
                return failures;
            }
        };

        if expected != actual {
            failures.push(format!(
                "{}: output differs\n  bytecode: {:?}\n  native:   {:?}",
                name, expected, actual
            ));
        }
        failures
    });

    assert!(
        example_failures.is_empty(),
        "\n{}",
        example_failures.join("\n")
    );
}

#[test]
fn every_frontend_valid_example_builds_as_a_bounded_native_artifact() {
    let (examples, mut failures) = frontend_valid_examples();
    let example_failures = run_items_parallel(&examples, |example| {
        match common::build_aot(&example.path, &example.source) {
            Ok(()) => Vec::new(),
            Err(error) => {
                vec![format!(
                    "{}: bounded native build failed: {}",
                    example.name, error
                )]
            }
        }
    });
    failures.extend(example_failures);

    failures.sort();
    assert!(
        failures.is_empty(),
        "frontend-valid examples must all build as bounded native artifacts:\n{}",
        failures.join("\n")
    );
}

#[test]
fn listed_examples_produce_identical_output_on_vm_and_jit() {
    let dir = examples_dir();
    let example_failures = run_items_parallel(PARITY_EXAMPLES, |name| {
        let name = *name;
        let mut failures = Vec::new();
        let path = dir.join(name);
        let source = match common::read_source_bounded(&path) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!("{}: could not read: {}", name, error));
                return failures;
            }
        };

        let expected = match run_bytecode(&path, &source) {
            Ok(output) => output,
            Err(error) => {
                failures.push(format!("{}: the bytecode VM failed: {}", name, error));
                return failures;
            }
        };
        let (status, stdout) =
            match common::run_jit_capture(path.to_str().expect("utf-8 path"), &source) {
                Ok(result) => result,
                Err(error) => {
                    failures.push(format!("{}: the JIT failed: {}", name, error));
                    return failures;
                }
            };

        if status != 0 {
            failures.push(format!("{}: the JIT exited with status {}", name, status));
            return failures;
        }

        let actual = match String::from_utf8(stdout) {
            Ok(actual) => without_final_line_ending(&actual).to_owned(),
            Err(error) => {
                failures.push(format!(
                    "{}: JIT stdout is not valid UTF-8: {}",
                    name, error
                ));
                return failures;
            }
        };
        if expected != actual {
            failures.push(format!(
                "{}: output differs\n  bytecode: {:?}\n  JIT:      {:?}",
                name, expected, actual
            ));
        }
        failures
    });

    assert!(
        example_failures.is_empty(),
        "deterministic examples must have identical VM/JIT output:\n{}",
        example_failures.join("\n")
    );
}
