use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    build_engine();
    tauri_build::build()
}

/// Builds the live wallpaper engine (`../engine`) for the target being compiled
/// and puts it where Tauri expects a sidecar: `binaries/wallflow-live-<triple>`.
/// Doing it here, not in a yarn script, means every Rust build — `tauri dev`,
/// `tauri build`, `cargo check`, CI — has a matching engine before
/// `tauri_build` checks `externalBin`.
fn build_engine() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let engine_dir = manifest_dir.join("../engine");
    let target = std::env::var("TARGET").unwrap();
    let host = std::env::var("HOST").unwrap();
    // Lets the end-to-end tests find the sidecar built for this target.
    println!("cargo:rustc-env=WALLFLOW_TARGET_TRIPLE={target}");

    for path in ["src", "Cargo.toml", "Cargo.lock"] {
        println!("cargo:rerun-if-changed={}", engine_dir.join(path).display());
    }

    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command
        .current_dir(&engine_dir)
        .args(["build", "--release", "--locked"])
        // Our own build's settings must not leak into the nested one.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET")
        // Under `cargo clippy` these would lint the engine with our flags; CI
        // checks the engine on its own.
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CLIPPY_ARGS");
    if target != host {
        command.args(["--target", &target]);
    }

    let status = command
        .status()
        .expect("could not run cargo to build the live wallpaper engine");
    assert!(
        status.success(),
        "building the live wallpaper engine failed"
    );

    let exe = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let built = if target != host {
        engine_dir.join("target").join(&target).join("release")
    } else {
        engine_dir.join("target/release")
    }
    .join(format!("wallflow-live{exe}"));

    let binaries = manifest_dir.join("binaries");
    std::fs::create_dir_all(&binaries).unwrap();
    copy_if_changed(
        &built,
        &binaries.join(format!("wallflow-live-{target}{exe}")),
    );
}

/// Leaves an unchanged sidecar alone so Tauri's watcher does not restart `dev`.
fn copy_if_changed(from: &Path, to: &Path) {
    let same = std::fs::read(from)
        .ok()
        .zip(std::fs::read(to).ok())
        .is_some_and(|(a, b)| a == b);
    if !same {
        std::fs::copy(from, to)
            .unwrap_or_else(|err| panic!("copying {} failed: {err}", from.display()));
    }
}
