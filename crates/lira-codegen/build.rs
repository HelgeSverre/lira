use std::path::PathBuf;

/// Every translation unit in `liblira_rt`, in one place so the rerun triggers
/// and the build itself cannot drift apart.
const SOURCES: &[&str] = &[
    "lira_rt.c",
    "lira_fiber.c",
    "lira_math.c",
    "lira_string.c",
    "lira_os.c",
    "lira_map.c",
    "lira_encoding.c",
    "lira_net.c",
    "lira_ctx.S",
];

fn main() {
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime");

    for file in SOURCES.iter().chain(["lira_rt.h"].iter()) {
        println!("cargo:rerun-if-changed={}", runtime.join(file).display());
    }
    println!("cargo:rerun-if-changed=build.rs");

    // The resulting archive is linked into this crate (so the JIT can resolve
    // runtime symbols in-process) and embedded verbatim in the binary (so
    // `lira build` can hand it to the system linker without a sysroot).
    let mut build = cc::Build::new();
    for file in SOURCES {
        build.file(runtime.join(file));
    }
    build
        .include(&runtime)
        .opt_level(2)
        .warnings(true)
        .compile("lira_rt");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    println!("cargo:rustc-env=LIRA_RT_ARCHIVE={}/liblira_rt.a", out_dir);
}
