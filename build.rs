use std::env;
use std::path::PathBuf;

/// Record where the vendored FFmpeg executables are, so a development build and
/// its tests find `ffmpeg.exe` and `ffprobe.exe` without a PATH entry. A shipped
/// build finds them next to the executable instead.
///
/// The same for the vendored ONNX Runtime DLLs, which are also copied next to
/// the executables, so the build output runs as it will ship.
fn main() {
    icon();
    onnxruntime();
    println!("cargo:rerun-if-env-changed=FFMPEG_BIN_DIR");
    let bin_dir = env::var("FFMPEG_BIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
                .join("ffmpeg")
                .join("bin")
        });
    println!(
        "cargo:rustc-env=FILE_BUDDY_FFMPEG_BIN={}",
        bin_dir.display()
    );
}

/// gpui takes the window icon from icon resource 1 of the executable.
fn icon() {
    println!("cargo:rerun-if-changed=icon/file-buddy.rc");
    println!("cargo:rerun-if-changed=icon/file-buddy.ico");
    embed_resource::compile("icon/file-buddy.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}

fn onnxruntime() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let bin_dir = manifest.join("onnxruntime").join("bin");
    println!("cargo:rerun-if-changed={}", bin_dir.display());
    println!(
        "cargo:rustc-env=FILE_BUDDY_ONNXRUNTIME_BIN={}",
        bin_dir.display()
    );

    // OUT_DIR is target/<profile>/build/<package>/out.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    for dll in [
        "onnxruntime.dll",
        "onnxruntime_providers_shared.dll",
        "DirectML.dll",
    ] {
        let from = bin_dir.join(dll);
        let to = profile_dir.join(dll);
        if !from.is_file() {
            println!("cargo:warning={} is missing", from.display());
        } else if std::fs::metadata(&to).map(|m| m.len()).ok()
            != std::fs::metadata(&from).map(|m| m.len()).ok()
            && let Err(e) = std::fs::copy(&from, &to)
        {
            println!("cargo:warning=copy {dll}: {e}");
        }
    }
}
