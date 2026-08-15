use std::path::PathBuf;

fn main() {
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime");

    for file in ["lira_rt.c", "lira_fiber.c", "lira_ctx.S", "lira_rt.h"] {
        println!("cargo:rerun-if-changed={}", runtime.join(file).display());
    }
    println!("cargo:rerun-if-changed=build.rs");

    // The resulting archive is linked into this crate (so the JIT can resolve
    // runtime symbols in-process) and embedded verbatim in the binary (so
    // `lira build` can hand it to the system linker without a sysroot).
    cc::Build::new()
        .file(runtime.join("lira_rt.c"))
        .file(runtime.join("lira_fiber.c"))
        .file(runtime.join("lira_ctx.S"))
        .include(&runtime)
        .opt_level(2)
        .warnings(true)
        .compile("lira_rt");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    println!("cargo:rustc-env=LIRA_RT_ARCHIVE={}/liblira_rt.a", out_dir);
}
