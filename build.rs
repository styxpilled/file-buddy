use std::path::{Path, PathBuf};
use std::{env, fs};

/// Copy the FFmpeg runtime DLLs next to the produced executable so it can be run
/// straight out of `target/<profile>/` without putting `ffmpeg/bin` on PATH.
fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let bin_dir = env::var("FFMPEG_BIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("ffmpeg").join("bin"));

    println!("cargo:rerun-if-env-changed=FFMPEG_BIN_DIR");
    println!("cargo:rerun-if-changed={}", bin_dir.display());

    if !bin_dir.is_dir() {
        println!(
            "cargo:warning=FFmpeg bin directory not found at {}; DLLs were not copied",
            bin_dir.display()
        );
        return;
    }

    // OUT_DIR is target/<profile>/build/<pkg>-<hash>/out — the profile dir is three levels up.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };

    for entry in fs::read_dir(&bin_dir).unwrap() {
        let src = entry.unwrap().path();
        if src.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)
            != Some("dll".to_string())
        {
            continue;
        }
        let dst = profile_dir.join(src.file_name().unwrap());
        if needs_copy(&src, &dst) {
            fs::copy(&src, &dst).unwrap_or_else(|e| {
                panic!("failed to copy {} to {}: {e}", src.display(), dst.display())
            });
        }
    }
}

fn needs_copy(src: &Path, dst: &Path) -> bool {
    let (Ok(src), Ok(dst)) = (fs::metadata(src), fs::metadata(dst)) else {
        return true;
    };
    match (src.modified(), dst.modified()) {
        (Ok(s), Ok(d)) => s > d || src.len() != dst.len(),
        _ => true,
    }
}
