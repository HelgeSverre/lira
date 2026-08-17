use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const MARKER_SOURCE: &str = "archive_marker.rs";

fn main() {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("lira-native-runtime is located below the workspace root");

    let tracked_files = semantic_files(&manifest_dir, &workspace_root);
    for path in &tracked_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    for name in [
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for (name, _) in env::vars().filter(|(name, _)| is_semantic_env(name)) {
        println!("cargo:rerun-if-env-changed={name}");
    }

    let semantic_marker = semantic_marker(&tracked_files);
    let unit_marker = unit_marker(&semantic_marker, &out_dir);
    let marker_source = out_dir.join(MARKER_SOURCE);
    fs::write(
        &marker_source,
        marker_source_text(&semantic_marker, &unit_marker),
    )
    .unwrap_or_else(|error| {
        panic!(
            "could not write archive marker {}: {error}",
            marker_source.display()
        )
    });
    // The unit marker identifies one Cargo build-script unit for diagnostics.
    // The dependent selects archives by the semantic marker instead: Cargo
    // must run this package once as a build dependency before lira-codegen's
    // build script, while the normal dependency that supplies DEP_* metadata
    // is a distinct unit. Equivalent units therefore have different unit
    // hashes even though their source, features, target and ABI are identical.
    // The semantic marker intentionally omits profile/optimisation settings
    // and the target-directory/unit hash, none of which changes the exported
    // native ABI.
    println!("cargo:archive_marker={unit_marker}");
    println!("cargo:semantic_marker={semantic_marker}");

    let profile_dir = profile_dir(&out_dir).unwrap_or_else(|| {
        panic!(
            "could not derive target/profile directory from OUT_DIR {}",
            out_dir.display()
        )
    });
    let target_dir = profile_dir
        .parent()
        .map_or_else(|| profile_dir.clone(), Path::to_path_buf);
    let archive_dir = profile_dir.join("deps");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());

    // These stable metadata keys become DEP_LIRA_NATIVE_RUNTIME_* variables
    // for a dependent package because this package declares `links` above.
    // The archive directory is derived from Cargo's target-aware OUT_DIR, so a
    // cross build cannot accidentally make a host archive look like a target
    // archive.
    println!("cargo:root={}", target_dir.display());
    println!("cargo:target_dir={}", target_dir.display());
    println!("cargo:profile_dir={}", profile_dir.display());
    println!("cargo:archive_dir={}", archive_dir.display());
    println!("cargo:staticlib_dir={}", archive_dir.display());
    println!("cargo:profile={profile}");
    println!("cargo:target={target}");
}

fn profile_dir(out_dir: &Path) -> Option<PathBuf> {
    // Cargo's OUT_DIR is target[/<triple>]/<profile>/build/<package-hash>/out.
    out_dir.parent()?.parent()?.parent().map(Path::to_path_buf)
}

fn semantic_files(manifest_dir: &Path, workspace_root: &Path) -> Vec<PathBuf> {
    let mut files = vec![
        manifest_dir.join("Cargo.toml"),
        manifest_dir.join("build.rs"),
    ];
    collect_files(&manifest_dir.join("src"), &mut files).unwrap_or_else(|error| {
        panic!(
            "could not collect lira-native-runtime source files below {}: {error}",
            manifest_dir.display()
        )
    });
    files.push(workspace_root.join("Cargo.toml"));
    files.push(workspace_root.join("Cargo.lock"));
    files.sort();
    files.dedup();
    files
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn semantic_marker(files: &[PathBuf]) -> String {
    let mut hasher = MarkerHasher::default();
    let target = env::var_os("TARGET").unwrap_or_default();
    let host = env::var_os("HOST").unwrap_or_default();
    hasher.field("target", &target.to_string_lossy());
    hasher.field("host", &host.to_string_lossy());

    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let rustc_version = Command::new(&rustc)
        .arg("-Vv")
        .output()
        .unwrap_or_else(|error| panic!("could not query {} -Vv: {error}", rustc.to_string_lossy()));
    if !rustc_version.status.success() {
        panic!(
            "{} -Vv failed with {}: {}",
            rustc.to_string_lossy(),
            rustc_version.status,
            String::from_utf8_lossy(&rustc_version.stderr)
        );
    }
    hasher.bytes(&rustc_version.stdout);

    let mut semantic_env = env::vars_os()
        .filter(|(name, _)| is_semantic_env(&name.to_string_lossy()))
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<Vec<_>>();
    semantic_env.sort();
    for (name, value) in semantic_env {
        hasher.field(&name, &value);
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    for path in files {
        let label = path
            .strip_prefix(&manifest_dir)
            .or_else(|_| {
                workspace_root
                    .as_deref()
                    .ok_or(())
                    .and_then(|root| path.strip_prefix(root).map_err(|_| ()))
            })
            .map_or_else(
                |_| path.display().to_string(),
                |relative| relative.display().to_string(),
            );
        let contents = fs::read(path).unwrap_or_else(|error| {
            panic!("could not read semantic input {}: {error}", path.display())
        });
        hasher.bytes(label.as_bytes());
        hasher.bytes(&contents);
    }

    format!(
        "lira_native_runtime_semantic_marker_{:016x}{:016x}",
        hasher.first, hasher.second
    )
}

fn unit_marker(semantic_marker: &str, out_dir: &Path) -> String {
    let mut hasher = MarkerHasher::default();
    hasher.field("semantic_marker", semantic_marker);
    let unit = out_dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-cargo-unit");
    // Cargo's package/unit hash distinguishes normal, test, feature, and
    // target-specific dependency units. Keep only that final component, not
    // the absolute target directory, so custom target roots do not change the
    // marker while stale artifacts from another Cargo unit cannot match it.
    hasher.field("cargo_unit", unit);
    format!(
        "lira_native_runtime_unit_marker_{:016x}{:016x}",
        hasher.first, hasher.second
    )
}

fn is_semantic_env(name: &str) -> bool {
    matches!(
        name,
        "RUSTFLAGS"
            | "CARGO_ENCODED_RUSTFLAGS"
            | "RUSTC"
            | "RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
    ) || name.starts_with("CARGO_CFG_")
        || name.starts_with("CARGO_FEATURE_")
        || (name.starts_with("CARGO_TARGET_") && name != "CARGO_TARGET_DIR")
}

fn marker_source_text(semantic_marker: &str, unit_marker: &str) -> String {
    format!(
        "#[doc(hidden)]\n#[export_name = \"{semantic_marker}\"]\npub extern \"C\" fn __lira_native_runtime_semantic_marker() {{}}\n#[doc(hidden)]\n#[export_name = \"{unit_marker}\"]\npub extern \"C\" fn __lira_native_runtime_unit_marker() {{}}\n"
    )
}

#[derive(Debug, Clone, Copy)]
struct MarkerHasher {
    first: u64,
    second: u64,
}

impl Default for MarkerHasher {
    fn default() -> Self {
        Self {
            first: 0xcbf29ce484222325,
            second: 0x84222325cbf29ce4,
        }
    }
}

impl MarkerHasher {
    fn field(&mut self, name: &str, value: &str) {
        self.bytes(name.as_bytes());
        self.bytes(value.as_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        let length = (bytes.len() as u64).to_le_bytes();
        self.update(&length);
        for byte in bytes {
            self.first ^= u64::from(*byte);
            self.first = self.first.wrapping_mul(0x100000001b3);
            self.second ^= u64::from(*byte).rotate_left(1);
            self.second = self.second.wrapping_mul(0x100000001b3);
            self.second ^= self.first.rotate_right(17);
        }
    }

    fn update(&mut self, bytes: &[u8; 8]) {
        for byte in bytes {
            self.first ^= u64::from(*byte);
            self.first = self.first.wrapping_mul(0x100000001b3);
            self.second ^= u64::from(*byte).rotate_left(1);
            self.second = self.second.wrapping_mul(0x100000001b3);
            self.second ^= self.first.rotate_right(17);
        }
    }
}
