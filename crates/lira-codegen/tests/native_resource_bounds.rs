//! End-to-end resource exhaustion tests for generated native programs.

mod common;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn scratch_dir(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "lira-native-resource-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn build_and_run(source: &str) -> Result<common::BoundedOutput, String> {
    build_and_run_with_limits(source, Duration::from_secs(20))
}

fn build_and_run_with_limits(
    source: &str,
    wall_time: Duration,
) -> Result<common::BoundedOutput, String> {
    let dir = scratch_dir("aot");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let source_path = dir.join("program.li");
    std::fs::write(&source_path, source).map_err(|error| error.to_string())?;
    let result = common::run_aot_with_limits(&source_path, source, 16 * 1024 * 1024, 16, wall_time);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[test]
fn fast_child_exit_is_not_misclassified_as_a_monitor_failure() {
    for attempt in 0..64 {
        let mut command = Command::new("/usr/bin/true");
        let output = common::run_command_with_wall_time(&mut command, Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("fast child attempt {attempt} failed: {error}"));
        assert!(
            output.status.success(),
            "fast child attempt {attempt} exited with {}",
            output.status
        );
    }
}

#[test]
fn tight_loop_is_killed_by_the_parent_deadline() {
    let source = r#"
        fn main() {
            while true {}
        }
    "#;
    let wall_time = Duration::from_secs(1);

    let vm_error = common::run_vm_capture_with_wall_time(
        std::path::Path::new("tight-loop.li"),
        source,
        wall_time,
    )
    .expect_err("a CPU-only VM loop must be killed by its parent deadline");
    assert!(
        vm_error.contains("wall-time limit"),
        "unexpected VM deadline diagnostic: {vm_error}"
    );

    let aot_error = build_and_run_with_limits(source, wall_time)
        .expect_err("a CPU-only AOT loop must be killed by its parent deadline");
    assert!(
        aot_error.contains("wall-time limit"),
        "unexpected AOT deadline diagnostic: {aot_error}"
    );

    let jit_error =
        common::run_jit_with_limits("tight-loop.li", source, 16 * 1024 * 1024, 16, wall_time)
            .expect_err("a CPU-only JIT loop must be killed by its parent deadline");
    assert!(
        jit_error.contains("wall-time limit"),
        "unexpected JIT deadline diagnostic: {jit_error}"
    );
}

#[test]
fn allocation_storm_fails_inside_the_native_memory_budget() {
    let source = r#"
        fn main() {
            let values: [int] = []
            while true {
                push(values, 1)
            }
        }
    "#;
    let output = build_and_run(source).expect("bounded AOT execution");
    assert!(
        !output.status.success(),
        "allocation storm unexpectedly exited cleanly"
    );
    assert!(
        output
            .stderr_text()
            .contains("native memory limit exceeded"),
        "unexpected resource diagnostic: {}",
        output.stderr_text()
    );
    assert_eq!(
        common::run_jit_with_runtime_limits("allocation-storm.li", source, 16 * 1024 * 1024, 16,),
        Ok(1),
        "the isolated JIT must report resource exhaustion"
    );
}

#[test]
fn spawn_storm_fails_inside_the_native_fiber_budget() {
    let source = r#"
        fn wait_forever(ch: Channel<int>) {
            recv(ch)
        }

        fn main() {
            let ch: Channel<int> = chan(0)
            while true {
                spawn wait_forever(ch)
            }
        }
    "#;
    let output = build_and_run(source).expect("bounded AOT execution");
    assert!(
        !output.status.success(),
        "spawn storm unexpectedly exited cleanly"
    );
    assert!(
        output.stderr_text().contains("native fiber limit exceeded"),
        "unexpected resource diagnostic: {}",
        output.stderr_text()
    );
    assert_eq!(
        common::run_jit_with_runtime_limits("spawn-storm.li", source, 16 * 1024 * 1024, 16),
        Ok(1),
        "the isolated JIT must report resource exhaustion"
    );
}
