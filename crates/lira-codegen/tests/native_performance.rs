//! Manual release benchmark comparing the stock VM with native execution.
//!
//! Run with:
//! `CARGO_BUILD_JOBS=1 cargo build --release -p lira -p liravm`
//! `CARGO_BUILD_JOBS=1 cargo test --release -p lira-codegen --test native_performance -- --ignored --nocapture --test-threads=1`
//!
//! The measured scopes are precompiled bytecode + process for the stock VM,
//! a prebuilt executable + process for AOT, compile + execute for JIT,
//! in-process frontend compilation, and AOT compile + link.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const EXPECTED_OUTPUT: &[u8] = b"4345129\n";
const WARMUP_RUNS: usize = 2;
const MEASURED_RUNS: usize = 7;

struct BenchmarkScratch(PathBuf);

impl BenchmarkScratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lira-native-benchmark-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create benchmark scratch directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for BenchmarkScratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug)]
struct Summary {
    minimum: Duration,
    median: Duration,
    mean: Duration,
    maximum: Duration,
}

fn summarize(mut samples: Vec<Duration>) -> Summary {
    assert_eq!(samples.len(), MEASURED_RUNS);
    samples.sort_unstable();
    let total = samples.iter().map(Duration::as_secs_f64).sum::<f64>();
    Summary {
        minimum: samples[0],
        median: samples[samples.len() / 2],
        mean: Duration::from_secs_f64(total / samples.len() as f64),
        maximum: samples[samples.len() - 1],
    }
}

fn measure(mut run: impl FnMut() -> Duration) -> Summary {
    for _ in 0..WARMUP_RUNS {
        run();
    }
    summarize((0..MEASURED_RUNS).map(|_| run()).collect())
}

fn assert_bounded_success(output: &common::BoundedOutput, expected_stdout: Option<&[u8]>) {
    assert!(!output.timed_out, "bounded child timed out");
    assert!(
        !output.memory_exceeded,
        "bounded child exceeded memory limit"
    );
    output
        .assert_complete_output()
        .expect("bounded child output must be complete");
    assert!(
        output.status.success(),
        "bounded child failed with {}; stderr: {}",
        output.status,
        output.stderr_text()
    );
    if let Some(expected_stdout) = expected_stdout {
        assert_eq!(output.stdout, expected_stdout);
    }
}

fn release_binary(workspace: &Path, name: &str) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"));
    let path = target.join("release").join(name);
    assert!(
        path.is_file(),
        "missing release binary {}; build the release CLI and VM first",
        path.display()
    );
    path
}

fn expected_vm_stdout(bytecode_path: &Path) -> Vec<u8> {
    let mut output = format!("Running {}\n", bytecode_path.display()).into_bytes();
    output.extend_from_slice(EXPECTED_OUTPUT);
    output
}

fn print_summary(label: &str, summary: &Summary) {
    println!(
        "{label:30} median {:>9.3} ms  mean {:>9.3} ms  range {:>9.3}..{:>9.3} ms",
        summary.median.as_secs_f64() * 1_000.0,
        summary.mean.as_secs_f64() * 1_000.0,
        summary.minimum.as_secs_f64() * 1_000.0,
        summary.maximum.as_secs_f64() * 1_000.0,
    );
}

#[test]
#[ignore = "manual, serialized release benchmark"]
fn compare_stock_vm_with_native_backends() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("lira-codegen must be inside the workspace crates directory")
        .to_owned();
    let source_path = workspace.join("benchmarks/native_vm_integer_hot_loop.li");
    let source = fs::read_to_string(&source_path).expect("read benchmark source");
    let source_name = source_path.to_str().expect("UTF-8 benchmark source path");
    let lira = release_binary(&workspace, "lira");
    let liravm = release_binary(&workspace, "liravm");
    let scratch = BenchmarkScratch::new();
    let bytecode_path = scratch.path().join("integer-hot-loop.lic");
    let aot_path = scratch.path().join("integer-hot-loop-aot");
    let aot_build_path = scratch.path().join("integer-hot-loop-aot-build");

    let bytecode =
        lirac::compile_with_imports(source_name, &source).expect("compile benchmark for stock VM");
    fs::write(&bytecode_path, bytecode).expect("write benchmark bytecode");
    let expected_vm_output = expected_vm_stdout(&bytecode_path);

    let mut build_aot = Command::new(&lira);
    build_aot
        .arg("build")
        .arg(&source_path)
        .arg("-o")
        .arg(&aot_path);
    let build_aot = common::run_command(&mut build_aot).expect("build bounded AOT executable");
    assert_bounded_success(&build_aot, None);
    assert!(
        aot_path.is_file(),
        "release AOT build did not produce {}",
        aot_path.display()
    );

    let mut verify_vm = Command::new(&liravm);
    verify_vm.arg("run").arg(&bytecode_path);
    let verify_vm = common::run_command(&mut verify_vm).expect("run bounded stock VM");
    assert_bounded_success(&verify_vm, Some(&expected_vm_output));

    let mut verify_aot = Command::new(&aot_path);
    let verify_aot = common::run_command(&mut verify_aot).expect("run bounded AOT executable");
    assert_bounded_success(&verify_aot, Some(EXPECTED_OUTPUT));

    let (jit_status, jit_stdout) =
        common::run_jit_capture(source_name, &source).expect("run bounded JIT benchmark");
    assert_eq!(jit_status, 0);
    assert_eq!(jit_stdout, EXPECTED_OUTPUT);

    let frontend = measure(|| {
        let started = Instant::now();
        let bytecode = lirac::compile_with_imports(source_name, &source)
            .expect("compile benchmark during measurement");
        assert!(!bytecode.is_empty());
        started.elapsed()
    });

    let vm = measure(|| {
        let mut command = Command::new(&liravm);
        command.arg("run").arg(&bytecode_path);
        let output = common::run_command(&mut command).expect("run bounded stock VM measurement");
        assert_bounded_success(&output, Some(&expected_vm_output));
        output.elapsed
    });

    let aot = measure(|| {
        let mut command = Command::new(&aot_path);
        let output = common::run_command(&mut command)
            .expect("run bounded prebuilt AOT execution measurement");
        assert_bounded_success(&output, Some(EXPECTED_OUTPUT));
        output.elapsed
    });

    let jit = measure(|| {
        let (status, elapsed) =
            common::run_jit_timed(source_name, &source).expect("run bounded JIT measurement");
        assert_eq!(status, 0);
        elapsed
    });

    let aot_build = measure(|| {
        let mut command = Command::new(&lira);
        command
            .arg("build")
            .arg(&source_path)
            .arg("-o")
            .arg(&aot_build_path);
        let output = common::run_command(&mut command).expect("run bounded AOT build measurement");
        assert_bounded_success(&output, None);
        assert!(
            aot_build_path.is_file(),
            "AOT compile + link did not produce {}",
            aot_build_path.display()
        );
        output.elapsed
    });

    println!("\nLira release benchmark ({WARMUP_RUNS} warmups, {MEASURED_RUNS} measured runs)");
    println!("workload: 4,000,000 integer loop iterations; checksum 4345129");
    print_summary("frontend compile (in process)", &frontend);
    print_summary("stock VM (precompiled bytecode + process)", &vm);
    print_summary("AOT (prebuilt executable + process)", &aot);
    print_summary("JIT (compile + execute)", &jit);
    print_summary("AOT (compile + link)", &aot_build);
    println!(
        "AOT median speedup over stock VM: {:.2}x",
        vm.median.as_secs_f64() / aot.median.as_secs_f64()
    );
}
