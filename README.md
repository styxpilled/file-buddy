# file-buddy

Media search app that indexes by filename, speech, OCR and CLIP and allows you
to clip/transcode.

Windows only, you'll have to gate some things behind OS flags, provide an
alternative OCR engine and have new sim scripts for an agent to test the app.

## --- SLOP LINE ---

Stack: Rust, `gpui-kit`, `rusqlite` with bundled SQLite and FTS5, FFmpeg for
probing, thumbnails and Whisper transcription, and `Windows.Media.Ocr` for
on-screen text. `gpui-video` is vendored as project code. All versions are
pinned exactly: gpui-pre is an unreviewed weekly snapshot of Zed's main branch.

## Tools

Install these before the first build:

| Tool                                    | Why                                                                                      |
| --------------------------------------- | ---------------------------------------------------------------------------------------- |
| Rust 1.88 or later, MSVC toolchain      | Edition 2024 and `let` chains                                                            |
| Visual Studio Build Tools, C++ workload | The MSVC linker and the C++ compiler for `whisper.cpp` and SQLite                        |
| LLVM (`clang`)                          | `bindgen` makes the FFmpeg bindings                                                      |
| CMake and Ninja, on `PATH`              | `whisper-rs` builds `whisper.cpp`. `.cargo/config.toml` sets `CMAKE_GENERATOR = "Ninja"` |
| Vulkan SDK                              | `whisper-rs` transcribes on the GPU through Vulkan                                       |
| Windows OCR language pack               | `Windows.Media.Ocr` reads on-screen text. English is the fallback                        |

CMake uses Ninja, not the Visual Studio generator, so it must find the MSVC
compiler (`cl.exe`) itself. If the `whisper-rs-sys` build cannot find a C or C++
compiler, or finds MinGW `gcc`, build from an "x64 Native Tools Command Prompt"
or a "Developer PowerShell for VS" (x64). These put `cl.exe` on `PATH`.

## Files that are not in the repository

These are gitignored. Put them at the repository root:

| Path                             | What                                                                                                                                                                                         |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ffmpeg/`                        | The gyan.dev **full shared** build of FFmpeg 8.1, with `bin/`, `include/` and `lib/`. `ffmpeg-next` is 8.1.0                                                                                 |
| `onnxruntime/bin/`               | `onnxruntime.dll` and `onnxruntime_providers_shared.dll` from ONNX Runtime 1.24 (DirectML build), and `DirectML.dll`                                                                         |
| `models/ggml-large-v3-turbo.bin` | The Whisper speech model, from `ggerganov/whisper.cpp` on Hugging Face                                                                                                                       |
| `models/mobileclip2-s2/`         | The image model, from `RuteNL/MobileCLIP2-S2-OpenCLIP-ONNX` on Hugging Face. It must hold `visual.onnx`, `text.onnx`, their `.onnx.data` files, `tokenizer.json` and `open_clip_config.json` |
| `test_videos/`                   | Optional. Clips for the `gpui-video` playback tests. Without them, the tests are skipped                                                                                                     |

Get the ONNX Runtime DLLs from NuGet. A `.nupkg` file is a ZIP archive:

- `onnxruntime.dll` and `onnxruntime_providers_shared.dll`: the package
  `Microsoft.ML.OnnxRuntime.DirectML`, version 1.24.x, in
  `runtimes/win-x64/native/`. The `ort` crate uses the `api-24` feature, so
  versions before 1.24 do not load.
- `DirectML.dll`: the package `Microsoft.AI.DirectML`, in `bin/x64-win/`.

The build copies the DLLs next to the executables: `gpui-video/build.rs` copies
the FFmpeg DLLs, and `build.rs` copies the ONNX Runtime DLLs. A debug build
finds `ffmpeg.exe`, `ffprobe.exe` and the models in the repository. For a build
you move to a different location, put `ffmpeg.exe`, `ffprobe.exe` and `models/`
next to `file-buddy.exe` and `fb.exe`.

## Variables to change on a new machine

Build time, in `.cargo/config.toml`:

| Variable             | Value in the repository        | Change it when                                   |
| -------------------- | ------------------------------ | ------------------------------------------------ |
| `LIBCLANG_PATH`      | `C:\Program Files\LLVM\bin`    | LLVM is in a different folder                    |
| `FFMPEG_DIR`         | `ffmpeg`, relative to the root | FFmpeg is not at `ffmpeg/` in the repository     |
| `FFMPEG_LIBS_DIR`    | `ffmpeg/lib`, relative         | The same                                         |
| `FFMPEG_INCLUDE_DIR` | `ffmpeg/include`, relative     | The same                                         |
| `CMAKE_GENERATOR`    | `Ninja`                        | Do not change. The Visual Studio generator fails |

Build time, in the environment:

| Variable         | What                                                                                                   |
| ---------------- | ------------------------------------------------------------------------------------------------------ |
| `VULKAN_SDK`     | The Vulkan SDK folder. The SDK installer sets it. Open a new terminal after the installation           |
| `FFMPEG_BIN_DIR` | Optional. The folder of the FFmpeg DLLs and executables, when it is not `ffmpeg/bin` in the repository |

Run time, in the environment. All are optional:

| Variable            | What                                                                              |
| ------------------- | --------------------------------------------------------------------------------- |
| `FILE_BUDDY_DATA`   | The folder for `file-buddy.db` and `thumbnails/`. Default: next to the executable |
| `FILE_BUDDY_FFMPEG` | The folder that holds `ffmpeg.exe` and `ffprobe.exe`                              |
| `FILE_BUDDY_FB`     | The path of `fb.exe`, which runs the speech and image workers                     |

In `file-buddy.toml`, change `roots` from the default `D:\clips` to your clip
folders.

## Build and run

```bash
cargo build
cargo run --bin file-buddy
```

The first run writes `file-buddy.toml` next to the executable. Set the folders
on the Settings page, or edit `roots` in the file. The Settings page saves the
file immediately. The file also holds `ignore` globs, `export_folder` for the
Studio, the search `weights`, the `paused` stages and the changed `keys`.

`fb` runs the index without a window. It uses the same `file-buddy.toml`,
database and thumbnails.

```bash
cargo run --bin fb -- scan              # walk the roots and run every stage
cargo run --bin fb -- search <words>    # print the ranked results
cargo run --bin fb -- visual <words>    # the 20 files that look most like the words
cargo run --bin fb -- like <path>...    # the files that look like these files
cargo run --bin fb -- reindex           # rebuild the search index
cargo run --bin fb -- status            # counts per state and per stage
```

`file-buddy` and `fb scan` start `fb worker whisper` and `fb worker clip` as
separate processes.

## Tests

`cargo test`
