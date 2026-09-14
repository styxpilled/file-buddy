use std::env;
use std::path::PathBuf;

/// Record where the vendored FFmpeg executables are, so a development build and
/// its tests find `ffmpeg.exe` and `ffprobe.exe` without a PATH entry. A shipped
/// build finds them next to the executable instead.
fn main() {
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
