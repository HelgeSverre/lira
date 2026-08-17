//! The `examples/game_of_life.li` example is a real, compute-heavy,
//! deterministic program: a toroidal cellular automaton stepped by a pool of
//! cooperative band worker fibers. These tests pin down its cross-backend
//! parity, golden frames for classic patterns, and the invariant that the
//! parallel band decomposition produces exactly what a single band produces.
//!
//! Config is injected with a single `LIRA_GOL_CFG` environment variable so
//! every run is hermetic and reproducible on VM, AOT, and JIT alike.

use std::path::{Path, PathBuf};

mod common;

fn example_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/game_of_life.li")
}

fn example_source() -> String {
    let path = example_path();
    common::read_source_bounded(&path).expect("read examples/game_of_life.li")
}

fn trimmed(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes)
        .map(|output| output.trim_end().to_owned())
        .map_err(|error| format!("program stdout is not valid UTF-8: {error}"))
}

fn run_vm(cfg: &str) -> Result<String, String> {
    match common::run_vm_capture_with_env(&example_path(), &example_source(), "LIRA_GOL_CFG", cfg)?
    {
        common::VmRunOutcome::Success { status: 0, output } => trimmed(output),
        other => Err(format!("bytecode VM returned {other:?}")),
    }
}

fn run_aot(cfg: &str) -> Result<String, String> {
    let output = common::run_aot_with_env(&example_path(), &example_source(), "LIRA_GOL_CFG", cfg)?;
    output.assert_complete_output()?;
    if !output.status.success() {
        return Err(format!("native executable exited with {}", output.status));
    }
    trimmed(output.stdout)
}

fn run_jit(cfg: &str) -> Result<String, String> {
    let (status, stdout) = common::run_jit_capture_with_env(
        example_path().to_str().expect("utf-8 path"),
        &example_source(),
        "LIRA_GOL_CFG",
        cfg,
    )?;
    if status != 0 {
        return Err(format!("JIT exited with status {status}"));
    }
    trimmed(stdout)
}

/// The frame series without the leading `game_of_life: ...` banner line. The
/// banner reports the configured band count, which legitimately differs
/// between band-split comparisons, so it is excluded there.
fn frames_without_banner(output: &str) -> Vec<&str> {
    output.lines().skip(1).collect()
}

/// Every backend must produce the exact same frame series for the same config.
#[test]
fn all_three_backends_agree_on_glider_parity() {
    let cfg = "rows=8,cols=12,bands=3,gens=4,pattern=glider";
    let vm = run_vm(cfg).unwrap_or_else(|e| panic!("VM failed: {e}"));
    let aot = run_aot(cfg).unwrap_or_else(|e| panic!("AOT failed: {e}"));
    let jit = run_jit(cfg).unwrap_or_else(|e| panic!("JIT failed: {e}"));
    assert_eq!(vm, jit, "VM and JIT glider frames diverged");
    assert_eq!(vm, aot, "VM and AOT glider frames diverged");
}

const BLINKER_GOLDEN: &str = r#"game_of_life: 5x5, 2 generations, 2 bands, pattern blinker
generation 0 (live 3)
.....
.....
.OOO.
.....
.....

generation 1 (live 3)
.....
..O..
..O..
..O..
.....

generation 2 (live 3)
.....
.....
.OOO.
.....
.....
"#;

/// A blinker must be period-2: horizontal, vertical, horizontal, with exactly
/// three live cells throughout. Pinned verbatim on both backends.
#[test]
fn blinker_is_period_two_with_matching_golden_frames() {
    let cfg = "rows=5,cols=5,bands=2,gens=2,pattern=blinker";
    let vm = run_vm(cfg).unwrap_or_else(|e| panic!("VM failed: {e}"));
    let jit = run_jit(cfg).unwrap_or_else(|e| panic!("JIT failed: {e}"));
    assert_eq!(
        vm.trim_end(),
        BLINKER_GOLDEN.trim_end(),
        "VM blinker frames diverge from the golden"
    );
    assert_eq!(
        jit.trim_end(),
        BLINKER_GOLDEN.trim_end(),
        "JIT blinker frames diverge from the golden"
    );
}

const GLIDER_GOLDEN: &str = r#"game_of_life: 8x12, 3 generations, 3 bands, pattern glider
generation 0 (live 5)
............
............
............
............
.......O....
........O...
......OOO...
............

generation 1 (live 5)
............
............
............
............
............
......O.O...
.......OO...
.......O....

generation 2 (live 5)
............
............
............
............
............
........O...
......O.O...
.......OO...

generation 3 (live 5)
............
............
............
............
............
.......O....
........OO..
.......OO...
"#;

/// A glider must translate down-right without changing its population of five,
/// and every frame must match the golden exactly on both backends.
#[test]
fn glider_translates_with_stable_population() {
    let cfg = "rows=8,cols=12,bands=3,gens=3,pattern=glider";
    let vm = run_vm(cfg).unwrap_or_else(|e| panic!("VM failed: {e}"));
    let jit = run_jit(cfg).unwrap_or_else(|e| panic!("JIT failed: {e}"));
    assert_eq!(
        vm.trim_end(),
        GLIDER_GOLDEN.trim_end(),
        "VM glider frames diverge from the golden"
    );
    assert_eq!(
        jit.trim_end(),
        GLIDER_GOLDEN.trim_end(),
        "JIT glider frames diverge from the golden"
    );
}

/// The whole point of the banded workers: 1 band and 4 bands must produce
/// byte-identical frames, because each band reads the same immutable previous
/// grid and writes its own disjoint rows. This is the strongest guard against
/// a broken parallel decomposition.
#[test]
fn parallel_band_split_matches_single_band_on_both_backends() {
    let single = "rows=12,cols=16,bands=1,gens=6,pattern=glider";
    let parallel = "rows=12,cols=16,bands=4,gens=6,pattern=glider";
    let vm_single = run_vm(single).unwrap_or_else(|e| panic!("VM single-band failed: {e}"));
    let vm_parallel = run_vm(parallel).unwrap_or_else(|e| panic!("VM 4-band failed: {e}"));
    let jit_single = run_jit(single).unwrap_or_else(|e| panic!("JIT single-band failed: {e}"));
    let jit_parallel = run_jit(parallel).unwrap_or_else(|e| panic!("JIT 4-band failed: {e}"));

    assert_eq!(
        frames_without_banner(&vm_single),
        frames_without_banner(&vm_parallel),
        "VM band split diverged from single band"
    );
    assert_eq!(
        frames_without_banner(&jit_single),
        frames_without_banner(&jit_parallel),
        "JIT band split diverged from single band"
    );
    assert_eq!(
        frames_without_banner(&vm_single),
        frames_without_banner(&jit_single),
        "VM/JIT 1-band baseline diverged"
    );
}

/// Rule-level sanity: a lone live cell has no neighbours and dies next
/// generation; a 2x2 block is a still life. Both are asserted through the live
/// counts printed in the generation headers on both backends.
#[test]
fn lone_cell_dies_and_still_life_persists() {
    let dot_cfg = "rows=3,cols=3,bands=1,gens=1,pattern=dot";
    let dot_vm = run_vm(dot_cfg).unwrap_or_else(|e| panic!("VM dot failed: {e}"));
    let dot_jit = run_jit(dot_cfg).unwrap_or_else(|e| panic!("JIT dot failed: {e}"));
    assert!(dot_vm.contains("generation 0 (live 1)"));
    assert!(dot_vm.contains("generation 1 (live 0)"));
    assert_eq!(dot_vm, dot_jit, "VM/JIT lone-cell run diverged");

    let block_cfg = "rows=6,cols=8,bands=2,gens=3,pattern=block";
    let block_vm = run_vm(block_cfg).unwrap_or_else(|e| panic!("VM block failed: {e}"));
    let block_jit = run_jit(block_cfg).unwrap_or_else(|e| panic!("JIT block failed: {e}"));
    assert!(block_vm.contains("generation 0 (live 4)"));
    assert!(block_vm.contains("generation 3 (live 4)"));
    assert_eq!(block_vm, block_jit, "VM/JIT block run diverged");
}
