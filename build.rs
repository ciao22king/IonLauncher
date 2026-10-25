//! Embeds the Windows icon and version metadata into the executable.
//!
//! Rust has no built-in way to attach an `.rc`, so the resource script is
//! compiled with `windres` and handed to the linker as an extra object. This
//! is what gives the `.exe` its icon in Explorer and the taskbar, and the
//! version details shown in the file's Properties dialog.
//!
//! Nothing happens on non-Windows targets, so Linux and macOS builds are
//! unaffected.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/ionlauncher.rc");
    println!("cargo:rerun-if-changed=assets/ionlauncher.ico");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }

    // windres is named after the target architecture, not the host.
    let prefix = match target.split('-').next().unwrap_or_default() {
        "x86_64" => "x86_64-w64-mingw32",
        "i686" => "i686-w64-mingw32",
        "aarch64" => "aarch64-w64-mingw32",
        other => {
            println!("cargo:warning=no windres prefix known for {other}; icon will be omitted");
            return;
        }
    };
    let windres = format!("{prefix}-windres");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap_or_default()).join("ionlauncher.res");

    let result = Command::new(&windres)
        .current_dir(manifest.join("assets"))
        .arg("ionlauncher.rc")
        .args(["-O", "coff"])
        .arg("-o")
        .arg(&out)
        .status();

    match result {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-bins={}", out.display());
        }
        // A missing or failing windres should not break the build: the binary
        // is still perfectly usable, it just loses its icon.
        Ok(status) => {
            println!("cargo:warning={windres} failed with {status}; icon will be omitted")
        }
        Err(err) => println!("cargo:warning=could not run {windres}: {err}; icon will be omitted"),
    }
}
