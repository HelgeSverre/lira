//! Cross-backend parity: every example the native backend accepts must produce
//! exactly the output the bytecode VM produces.
//!
//! The two backends share a front end but nothing else — one interprets tagged
//! values, the other emits unboxed machine code — so agreeing on output is the
//! strongest evidence that the lowering is faithful. Examples the native backend
//! declines are counted, not failed: the backend is deliberately partial, and it
//! is required to say so rather than mis-compile.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Examples that both backends must agree on. Adding a construct to the backend
/// should mean adding entries here.
const PARITY_EXAMPLES: &[&str] = &[
    "hello.li",
    "fibonacci.li",
    "factorial.li",
    "prime_checker.li",
    "control_flow.li",
    "loop_control.li",
    "loop_infinite.li",
    "if_expressions.li",
    "block_expressions.li",
    "compound_assign.li",
    "bitwise_ops.li",
    "structs.li",
    "impl_block.li",
    "static_method_params.li",
    "enums_basic.li",
    "enum_data.li",
    "pattern_match.li",
    "pattern_guards.li",
    "match_binding.li",
    "match_or.li",
    "match_range.li",
    "match_struct.li",
    "pattern_constructor.li",
    "string_interpolation.li",
    "string_escapes.li",
    "mutual_recursion.li",
    "main_entry_point.li",
    "integer_types.li",
    "range_expressions.li",
    "type_alias.li",
    "test_core.li",
    "test_math.li",
    "test_collections.li",
    "test_base64.li",
    "test_hash.li",
    "channel_basic.li",
    "lambda.li",
    "function_types.li",
    "pattern_tuple.li",
    "pattern_tuple_simple.li",
    "pattern_tuple_literals.li",
    "optional_chaining.li",
    "optional_access.li",
    "try_operator.li",
    "result_propagation.li",
    "map_literals.li",
    "select_basic.li",
];

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lira-parity-{}-{}", std::process::id(), id))
}

/// Run one example on the bytecode VM.
fn run_bytecode(path: &Path, source: &str) -> Result<String, String> {
    let bytecode = lirac::compile_with_imports(path.to_str().expect("utf-8 path"), source)?;
    let (_, lines) = liravm::run_with_capture(&bytecode)?;
    Ok(lines.join("\n"))
}

/// Compile one example to a native executable and run it.
fn run_native(path: &Path, source: &str) -> Result<String, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let binary = dir.join("program");
    let result = (|| {
        lira_codegen::build_native(path.to_str().expect("utf-8 path"), source, &binary)?;
        let output = Command::new(&binary).output().map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[test]
fn listed_examples_produce_identical_output_on_both_backends() {
    let dir = examples_dir();
    let mut failures = Vec::new();

    for name in PARITY_EXAMPLES {
        let path = dir.join(name);
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(e) => {
                failures.push(format!("{}: could not read: {}", name, e));
                continue;
            }
        };

        let expected = match run_bytecode(&path, &source) {
            Ok(output) => output,
            Err(e) => {
                failures.push(format!("{}: the bytecode VM failed: {}", name, e));
                continue;
            }
        };
        let actual = match run_native(&path, &source) {
            Ok(output) => output,
            Err(e) => {
                failures.push(format!("{}: the native backend failed: {}", name, e));
                continue;
            }
        };

        if expected.trim_end() != actual.trim_end() {
            failures.push(format!(
                "{}: output differs\n  bytecode: {:?}\n  native:   {:?}",
                name,
                expected.trim_end(),
                actual.trim_end()
            ));
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn every_example_either_compiles_or_is_declined_with_a_reason() {
    // A panic, an internal error, or a silent mis-compile would all be bugs. A
    // clear "not lowered yet" is the contract for the parts of the language the
    // native backend does not cover.
    let dir = examples_dir();
    let entries = std::fs::read_dir(&dir).expect("examples directory");
    let mut internal_errors = Vec::new();
    let mut accepted = 0usize;
    let mut declined = 0usize;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("li") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Only consider examples the front end itself accepts; the rest are
        // deliberately invalid programs used to test diagnostics.
        if lirac::check_with_imports(path.to_str().expect("utf-8 path"), &source).is_err() {
            continue;
        }

        let analysis =
            match lirac::analyze_with_imports(path.to_str().expect("utf-8 path"), &source) {
                Ok(analysis) => analysis,
                Err(_) => continue,
            };
        match lira_codegen::aot::compile_object(&analysis.program, &analysis.sema) {
            Ok(_) => accepted += 1,
            Err(lira_codegen::CodegenError::Unsupported { .. }) => declined += 1,
            Err(other) => internal_errors.push(format!(
                "{}: {}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                other
            )),
        }
    }

    assert!(
        internal_errors.is_empty(),
        "the native backend reported internal errors instead of clean diagnostics:\n{}",
        internal_errors.join("\n")
    );
    assert!(
        accepted > 60,
        "expected the native backend to accept most examples, got {} accepted / {} declined",
        accepted,
        declined
    );
}
