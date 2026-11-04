// Derived from herdr-core crates/ghostty-vt/build.rs (Apache-2.0).
//! Builds libghostty-vt from the pinned Ghostty submodule with Zig.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest
        .join("../../vendor/ghostty")
        .canonicalize()
        .expect("vendor/ghostty missing: run `git submodule update --init --depth 1`");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for path in ["src", "build.zig", "build.zig.zon", "include"] {
        println!("cargo:rerun-if-changed={}", src.join(path).display());
    }
    println!("cargo:rerun-if-env-changed=ZIG");

    let (target, host) = (
        std::env::var("TARGET").unwrap(),
        std::env::var("HOST").unwrap(),
    );
    let zig = PathBuf::from(std::env::var("ZIG").unwrap_or_else(|_| "zig".into()));
    let mut build = Command::new(&zig);
    // Baseline CPU, never this machine's: an hc-tui binary may run on hosts
    // whose CPUs lack this one's features.
    build.current_dir(&src).args([
        "build",
        "-Demit-lib-vt",
        "-Doptimize=ReleaseFast",
        "-Dcpu=baseline",
    ]);
    // Cross-compiling (the static musl Linux build): Zig must target it too.
    if target != host {
        build.arg(format!("-Dtarget={}", zig_target(&target)));
    }
    let status = build
        .arg("--prefix")
        .arg(out.join("zig-out"))
        .arg("--cache-dir")
        .arg(out.join("zig-cache"))
        .status()
        .unwrap_or_else(|e| panic!("cannot run {zig:?} (Zig 0.16 required): {e}"));
    assert!(
        status.success(),
        "zig build of libghostty-vt failed: {status}"
    );

    // Keep only the static archive in the search path so the linker can't pick
    // the shared one.
    let lib = out.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::copy(
        out.join("zig-out/lib").join("libghostty-vt.a"),
        lib.join("libghostty-vt.a"),
    )
    .unwrap();
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");
}

/// Cargo's target triple as Zig names it: `x86_64-unknown-linux-musl` ->
/// `x86_64-linux-musl`, `aarch64-apple-darwin` -> `aarch64-macos`.
fn zig_target(triple: &str) -> String {
    let arch = triple.split('-').next().unwrap_or("native");
    let os = if triple.contains("apple") {
        "macos"
    } else if triple.contains("windows") {
        "windows"
    } else {
        "linux"
    };
    let abi = ["musl", "gnu", "msvc"]
        .into_iter()
        .find(|abi| triple.ends_with(abi));
    abi.map_or(format!("{arch}-{os}"), |abi| format!("{arch}-{os}-{abi}"))
}
