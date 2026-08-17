use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every translation unit in `liblira_rt`, in one place so the rerun triggers
/// and the build itself cannot drift apart.
const SOURCES: &[&str] = &[
    "lira_rt.c",
    "lira_gc.c",
    "lira_any.c",
    "lira_fiber.c",
    "lira_io.c",
    "lira_math.c",
    "lira_string.c",
    "lira_os.c",
    "lira_map.c",
    "lira_encoding.c",
    "lira_net.c",
    "lira_ctx.S",
];

/// Symbols that form the Rust native-runtime ABI consumed by generated AOT
/// objects.  Checking these in addition to the source marker prevents a
/// truncated or otherwise incompatible archive from being embedded.
const REQUIRED_NATIVE_SYMBOLS: &[&str] = &[
    "lira_rt_regex_match",
    "lira_rt_regex_find",
    "lira_rt_regex_find_all",
    "lira_rt_regex_replace",
    "lira_rt_regex_replace_all",
    "lira_rt_regex_split",
    "lira_rt_regex_captures",
    "lira_rt_regex_is_valid",
    "lira_rt_json_parse",
    "lira_rt_json_stringify",
    "lira_rt_json_stringify_pretty",
    "lira_rt_http_get",
    "lira_rt_http_post",
    "lira_rt_http_request",
];

fn main() {
    let (_host, target) = require_host_target_match();
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime");

    for file in SOURCES.iter().chain(["lira_rt.h"].iter()) {
        println!("cargo:rerun-if-changed={}", runtime.join(file).display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    for name in [
        "LIRA_NATIVE_RUNTIME_ARCHIVE",
        "DEP_LIRA_NATIVE_RUNTIME_ARCHIVE_DIR",
        "DEP_LIRA_NATIVE_RUNTIME_STATICLIB_DIR",
        "DEP_LIRA_NATIVE_RUNTIME_SEMANTIC_MARKER",
        "DEP_LIRA_NATIVE_RUNTIME_TARGET",
        "CARGO_CFG_TARGET_OS",
        "CARGO_CFG_TARGET_ENV",
        "TARGET",
        "MACOSX_DEPLOYMENT_TARGET",
        "NM",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    {
        println!("cargo:rerun-if-env-changed=NM_{target}");
        println!("cargo:rerun-if-env-changed=NM_{}", target.replace('-', "_"));
        println!(
            "cargo:rerun-if-env-changed=NM_{}",
            target.replace('-', "_").to_uppercase()
        );
    }

    let target_os = target_os().unwrap_or_else(|error| panic!("{error}"));
    let target_env = target_env();
    ensure_supported_target_os(&target_os).unwrap_or_else(|error| panic!("{error}"));

    // The resulting archive is linked into this crate (so the JIT can resolve
    // runtime symbols in-process) and embedded verbatim in the binary (so
    // `lira build` can hand it to the system linker without a sysroot).
    let mut build = cc::Build::new();
    for file in SOURCES {
        build.file(runtime.join(file));
    }
    configure_macos_deployment_target(&mut build, &target);
    build
        .include(&runtime)
        .opt_level(2)
        .warnings(true)
        .compile("lira_rt");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    println!(
        "cargo:rustc-env=LIRA_RT_ARCHIVE={}/{}",
        out_dir,
        c_archive_name(&target_os, &target_env)
    );
    let semantic_marker = std::env::var("DEP_LIRA_NATIVE_RUNTIME_SEMANTIC_MARKER")
        .unwrap_or_else(|_| {
            panic!(
                "lira-native-runtime did not publish a semantic archive marker; rebuild the dependency with its current build script"
            )
        });
    let runtime_target = std::env::var("DEP_LIRA_NATIVE_RUNTIME_TARGET").unwrap_or_else(|_| {
        panic!(
            "lira-native-runtime did not publish its Cargo target; refusing to guess whether its archive is host- or target-built"
        )
    });
    if runtime_target != target {
        panic!(
            "lira-native-runtime was built for {runtime_target}, but lira-codegen targets {target}; refusing to embed a host archive for a different target"
        );
    }
    let rust_archive = find_native_runtime_archive(
        Path::new(&out_dir),
        &target_os,
        &target_env,
        &semantic_marker,
    )
    .unwrap_or_else(|message| {
        panic!("could not locate a compatible lira-native-runtime static archive: {message}")
    });
    let embedded = PathBuf::from(&out_dir).join(native_archive_name(&target_os, &target_env));
    fs::copy(&rust_archive, &embedded).unwrap_or_else(|error| {
        panic!(
            "could not copy {} to {}: {error}",
            rust_archive.display(),
            embedded.display()
        )
    });
    println!("cargo:rerun-if-changed={}", rust_archive.display());
    println!(
        "cargo:rustc-env=LIRA_NATIVE_RUNTIME_ARCHIVE={}",
        embedded.display()
    );
}

fn require_host_target_match() -> (String, String) {
    let host = std::env::var("HOST").unwrap_or_else(|_| "<unknown>".to_owned());
    let target = std::env::var("TARGET").unwrap_or_else(|_| "<unknown>".to_owned());
    if host == "<unknown>" || target == "<unknown>" {
        panic!(
            "lira-codegen's native backend requires Cargo HOST and TARGET; got HOST={host}, TARGET={target}"
        );
    }
    if host != target {
        panic!(
            "lira-codegen's native backend is host-only: HOST={host}, TARGET={target}; cross-target native code generation is unsupported"
        );
    }
    (host, target)
}

fn target_os() -> Result<String, String> {
    std::env::var("CARGO_CFG_TARGET_OS").map_err(|_| {
        format!(
            "CARGO_CFG_TARGET_OS is not set for TARGET={}",
            std::env::var("TARGET").unwrap_or_else(|_| "<unknown>".to_owned())
        )
    })
}

fn target_env() -> String {
    std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default()
}

fn ensure_supported_target_os(target_os: &str) -> Result<(), String> {
    if target_os == "windows" {
        Err("lira-codegen's native backend does not support Windows targets: the C/assembly runtime is unavailable".to_owned())
    } else {
        Ok(())
    }
}

fn configure_macos_deployment_target(build: &mut cc::Build, target: &str) {
    let explicit = std::env::var("MACOSX_DEPLOYMENT_TARGET").ok();
    if let Some(version) = macos_deployment_target(target, explicit.as_deref()) {
        let flag = format!("-mmacosx-version-min={version}");
        build.flag(&flag).asm_flag(&flag);
    }
}

fn macos_deployment_target(target: &str, explicit: Option<&str>) -> Option<String> {
    if !target.ends_with("-apple-darwin") {
        return None;
    }
    explicit
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
        .or_else(|| match target {
            "aarch64-apple-darwin" => Some("11.0".to_owned()),
            "x86_64-apple-darwin" => Some("10.12".to_owned()),
            _ => None,
        })
}

fn is_msvc(target_os: &str, target_env: &str) -> bool {
    target_os == "windows" && target_env == "msvc"
}

fn c_archive_name(target_os: &str, target_env: &str) -> &'static str {
    if is_msvc(target_os, target_env) {
        "lira_rt.lib"
    } else {
        "liblira_rt.a"
    }
}

fn native_archive_name(target_os: &str, target_env: &str) -> &'static str {
    if is_msvc(target_os, target_env) {
        "lira_native_runtime.lib"
    } else {
        "liblira_native_runtime.a"
    }
}

fn archive_pattern(target_os: &str, target_env: &str) -> (&'static str, &'static str) {
    if is_msvc(target_os, target_env) {
        ("lira_native_runtime-", ".lib")
    } else {
        ("liblira_native_runtime-", ".a")
    }
}

/// Cargo hashes dependency artifact names in `target/<profile>/deps`.  Stable
/// Cargo does not expose the exact staticlib path, so the dependency build
/// script publishes the target-aware archive directory and a source marker.
/// Only archives with the current semantic marker and the complete native ABI are
/// eligible; mtime is deliberately never used for selection.
fn find_native_runtime_archive(
    out_dir: &Path,
    target_os: &str,
    target_env: &str,
    semantic_marker: &str,
) -> Result<PathBuf, String> {
    let profile_dir = profile_dir(out_dir).ok_or_else(|| {
        format!(
            "could not derive target/profile directory from OUT_DIR {}",
            out_dir.display()
        )
    })?;
    let expected_archive_dir = profile_dir.join("deps");
    let (prefix, suffix) = archive_pattern(target_os, target_env);
    if let Some(override_path) = std::env::var_os("LIRA_NATIVE_RUNTIME_ARCHIVE") {
        let path = PathBuf::from(override_path);
        println!("cargo:rerun-if-changed={}", path.display());
        validate_archive(&path, suffix, &[semantic_marker]).map_err(|reason| {
            format!(
                "LIRA_NATIVE_RUNTIME_ARCHIVE={} is not a compatible {target_os} archive: {reason}",
                path.display()
            )
        })?;
        return Ok(path);
    }

    let archive_dir = std::env::var_os("DEP_LIRA_NATIVE_RUNTIME_ARCHIVE_DIR")
        .or_else(|| std::env::var_os("DEP_LIRA_NATIVE_RUNTIME_STATICLIB_DIR"))
        .map(PathBuf::from)
        .ok_or_else(|| {
            "DEP_LIRA_NATIVE_RUNTIME_ARCHIVE_DIR is not set; ".to_owned()
                + "the native runtime build script must publish its target-aware archive directory"
        })?;
    ensure_same_directory(&expected_archive_dir, &archive_dir)?;

    println!("cargo:rerun-if-changed={}", archive_dir.display());
    let entries = fs::read_dir(&archive_dir)
        .map_err(|error| format!("cannot read {}: {error}", archive_dir.display()))?;
    let mut candidates = Vec::new();
    let mut rejected = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "cannot inspect an entry in {}: {error}",
                archive_dir.display()
            )
        })?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(prefix) || !name.ends_with(suffix) {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "cannot inspect archive candidate {}: {error}",
                path.display()
            )
        })?;
        if !file_type.is_file() {
            rejected.push(format!("{} (not a regular file)", path.display()));
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        match validate_archive(&path, suffix, &[semantic_marker]) {
            Ok(()) => candidates.push(path),
            Err(reason) => rejected.push(format!("{} ({reason})", path.display())),
        }
    }

    if candidates.is_empty() {
        let detail = if rejected.is_empty() {
            format!("no {prefix}*{suffix} files were found")
        } else {
            format!("all candidates were rejected: {}", rejected.join(", "))
        };
        return Err(format!(
            "{}; expected current semantic marker {semantic_marker} in {}",
            detail,
            archive_dir.display()
        ));
    }

    choose_archive(candidates, semantic_marker, &archive_dir)
}

fn choose_archive(
    mut candidates: Vec<PathBuf>,
    semantic_marker: &str,
    archive_dir: &Path,
) -> Result<PathBuf, String> {
    // Cargo can compile the same library/build-script unit more than once for
    // normal and test graphs, producing distinct archive hashes that carry the
    // same semantic marker. After target-directory, target-triple,
    // semantic-input, archive-format, and complete-ABI validation, those
    // candidates are equivalent for Lira's exported runtime ABI. Never use
    // mtime: sort the remaining Cargo hashes so incremental builds are
    // deterministic.
    candidates.sort_by(|left, right| left.as_os_str().cmp(right.as_os_str()));
    if candidates.len() > 1 {
        println!(
            "cargo:warning={} compatible archives carry semantic marker {semantic_marker}; selecting {} deterministically",
            candidates.len(),
            candidates[0].display()
        );
    }
    candidates.into_iter().next().ok_or_else(|| {
        format!(
            "no archive remained after marker and ABI validation in {}",
            archive_dir.display()
        )
    })
}

fn profile_dir(out_dir: &Path) -> Option<PathBuf> {
    // Cargo's OUT_DIR is target[/<triple>]/<profile>/build/<package-hash>/out.
    out_dir.parent()?.parent()?.parent().map(Path::to_path_buf)
}

fn ensure_same_directory(expected: &Path, actual: &Path) -> Result<(), String> {
    let expected = fs::canonicalize(expected).map_err(|error| {
        format!(
            "cannot resolve Cargo's target archive directory {}: {error}",
            expected.display()
        )
    })?;
    let actual = fs::canonicalize(actual).map_err(|error| {
        format!(
            "cannot resolve native runtime archive directory {}: {error}",
            actual.display()
        )
    })?;
    if expected != actual {
        return Err(format!(
            "native runtime archive directory {} does not match this build target's {}",
            actual.display(),
            expected.display()
        ));
    }
    Ok(())
}

fn validate_archive(path: &Path, suffix: &str, markers: &[&str]) -> Result<(), String> {
    let extension = path.extension().and_then(|value| value.to_str());
    let expected_extension = suffix.strip_prefix('.');
    if extension != expected_extension {
        return Err(format!(
            "expected a .{} archive, found {}",
            expected_extension.unwrap_or(suffix),
            path.display()
        ));
    }
    let mut magic = [0_u8; 8];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .map_err(|error| format!("cannot read archive header: {error}"))?;
    if magic != *b"!<arch>\n" {
        return Err("file is not a self-contained ar static archive".to_owned());
    }
    let symbols = archive_symbols(path)?;
    validate_symbols(&symbols, markers)
}

fn validate_symbols(
    symbols: &std::collections::HashSet<String>,
    markers: &[&str],
) -> Result<(), String> {
    let missing_markers = markers
        .iter()
        .copied()
        .filter(|marker| !symbols.contains(*marker))
        .collect::<Vec<_>>();
    if !missing_markers.is_empty() {
        return Err(format!(
            "archive marker symbols are missing: {}",
            missing_markers.join(", ")
        ));
    }
    let missing = REQUIRED_NATIVE_SYMBOLS
        .iter()
        .copied()
        .filter(|symbol| !symbols.contains(*symbol))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "missing exported ABI symbols: {}",
            missing.join(", ")
        ));
    }
    Ok(())
}

fn archive_symbols(path: &Path) -> Result<std::collections::HashSet<String>, String> {
    let mut commands = Vec::new();
    if let Some(path) = std::env::var_os("NM") {
        commands.push(PathBuf::from(path));
    }
    if let Ok(target) = std::env::var("TARGET") {
        for key in [
            format!("NM_{target}"),
            format!("NM_{}", target.replace('-', "_")),
            format!("NM_{}", target.replace('-', "_").to_uppercase()),
        ] {
            if let Some(path) = std::env::var_os(key) {
                commands.push(PathBuf::from(path));
            }
        }
    }
    commands.extend([PathBuf::from("llvm-nm"), PathBuf::from("nm")]);

    let mut failures = Vec::new();
    for command in commands {
        let output = match Command::new(&command).arg("-g").arg(path).output() {
            Ok(output) => output,
            Err(error) => {
                failures.push(format!("{}: {error}", command.display()));
                continue;
            }
        };
        let symbols = defined_symbols(&output.stdout);
        // Apple's `nm` can successfully print every symbol we need and still
        // exit non-zero when a Rust staticlib also contains compiler-builtins
        // bitcode from a newer LLVM.  The caller validates the exact marker
        // and every required ABI symbol, so usable output is stronger evidence
        // than the tool's aggregate exit status for unrelated archive members.
        if !symbols.is_empty() {
            return Ok(symbols);
        }
        failures.push(format!(
            "{} exited with {} and produced no symbols",
            command.display(),
            output.status
        ));
    }
    Err(format!(
        "could not inspect exported symbols in {} ({}); install a target-compatible nm or set NM",
        path.display(),
        failures.join(", ")
    ))
}

fn normalize_symbol(symbol: &str) -> String {
    symbol.strip_prefix('_').unwrap_or(symbol).to_owned()
}

fn defined_symbols(output: &[u8]) -> std::collections::HashSet<String> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let symbol_type = fields.get(fields.len().checked_sub(2)?)?;
            if symbol_type.len() != 1 || matches!(symbol_type.as_bytes()[0], b'U' | b'u' | b'?') {
                return None;
            }
            fields.last().map(|symbol| normalize_symbol(symbol))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        archive_pattern, c_archive_name, choose_archive, defined_symbols,
        ensure_supported_target_os, macos_deployment_target, native_archive_name, validate_symbols,
        REQUIRED_NATIVE_SYMBOLS,
    };
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn equivalent_cargo_units_are_accepted_with_current_semantics() {
        let semantic_marker = "lira_native_runtime_semantic_marker_current";
        let stale_unit_marker = "lira_native_runtime_unit_marker_stale";
        let mut symbols = REQUIRED_NATIVE_SYMBOLS
            .iter()
            .map(|symbol| (*symbol).to_owned())
            .collect::<HashSet<_>>();
        symbols.insert(semantic_marker.to_owned());
        symbols.insert(stale_unit_marker.to_owned());

        validate_symbols(&symbols, &[semantic_marker])
            .expect("Cargo unit hashes do not change source/target ABI compatibility");
    }

    #[test]
    fn stale_semantic_marker_is_rejected() {
        let current_semantic_marker = "lira_native_runtime_semantic_marker_current";
        let stale_semantic_marker = "lira_native_runtime_semantic_marker_stale";
        let mut symbols = REQUIRED_NATIVE_SYMBOLS
            .iter()
            .map(|symbol| (*symbol).to_owned())
            .collect::<HashSet<_>>();
        symbols.insert(stale_semantic_marker.to_owned());

        let error = validate_symbols(&symbols, &[current_semantic_marker])
            .expect_err("a stale semantic marker must not be accepted");
        assert!(error.contains(current_semantic_marker));
    }

    #[test]
    fn undefined_abi_symbols_are_not_treated_as_exports() {
        let output =
            b"                 U _lira_rt_regex_match\n0000000000000000 T _lira_rt_json_parse\n";
        let symbols = defined_symbols(output);
        assert!(!symbols.contains("lira_rt_regex_match"));
        assert!(symbols.contains("lira_rt_json_parse"));
    }

    #[test]
    fn equivalent_candidates_use_stable_archive_name_order() {
        let selected = choose_archive(
            vec![
                PathBuf::from("liblira_native_runtime-b.a"),
                PathBuf::from("liblira_native_runtime-a.a"),
            ],
            "semantic-marker",
            PathBuf::from("target/debug/deps").as_path(),
        )
        .expect("at least one validated candidate");
        assert_eq!(selected, PathBuf::from("liblira_native_runtime-a.a"));
    }

    #[test]
    fn archive_extensions_follow_target_environment() {
        assert_eq!(c_archive_name("windows", "msvc"), "lira_rt.lib");
        assert_eq!(
            native_archive_name("windows", "msvc"),
            "lira_native_runtime.lib"
        );
        assert_eq!(
            archive_pattern("windows", "msvc"),
            ("lira_native_runtime-", ".lib")
        );
        assert_eq!(c_archive_name("windows", "gnu"), "liblira_rt.a");
        assert_eq!(
            native_archive_name("windows", "gnu"),
            "liblira_native_runtime.a"
        );
        assert_eq!(
            archive_pattern("windows", "gnu"),
            ("liblira_native_runtime-", ".a")
        );
    }

    #[test]
    fn macos_defaults_match_rust_platform_floor() {
        assert_eq!(
            macos_deployment_target("aarch64-apple-darwin", None).as_deref(),
            Some("11.0")
        );
        assert_eq!(
            macos_deployment_target("x86_64-apple-darwin", None).as_deref(),
            Some("10.12")
        );
        assert_eq!(
            macos_deployment_target("aarch64-unknown-linux-gnu", None),
            None
        );
    }

    #[test]
    fn windows_target_is_rejected_before_native_compilation() {
        let error = ensure_supported_target_os("windows")
            .expect_err("Windows native compilation is not implemented");
        assert!(error.contains("does not support Windows"));
        assert!(ensure_supported_target_os("macos").is_ok());
        assert!(ensure_supported_target_os("linux").is_ok());
    }

    #[test]
    fn explicit_macos_deployment_target_wins_over_default() {
        assert_eq!(
            macos_deployment_target("aarch64-apple-darwin", Some("13.3")).as_deref(),
            Some("13.3")
        );
        assert_eq!(
            macos_deployment_target("x86_64-apple-darwin", Some("10.15")).as_deref(),
            Some("10.15")
        );
        assert_eq!(
            macos_deployment_target("aarch64-apple-darwin", Some("")).as_deref(),
            Some("11.0")
        );
    }
}
