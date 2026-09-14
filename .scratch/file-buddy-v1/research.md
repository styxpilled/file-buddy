# Research notes

Facts found while writing the spec. They are here so that a later session does
not find them again. Each one has a date, because some of them will go stale.

Found on 2026-09-14.

## GPUI cannot do three things you might assume it can

**No file on the clipboard.** `ClipboardEntry` has two variants, `String` and
`Image`. The Windows backend writes `CF_UNICODETEXT` and image formats only, and
it writes just the first entry. `CF_HDROP` appears on the read side only. To put
a real file on the clipboard you must call `SetClipboardData` yourself. Issue 05
uses `clipboard-win`.

**No drag out of the window.** `Div::on_drag`, `on_drag_move` and `on_drop` work
only inside the GPUI window. The drag state is a GPUI element drawn by GPUI.
There is no `DoDragDrop`, no `IDropSource` and no `IDataObject` source anywhere
in the crate. Receiving an external drop does work, through `IDropTarget` and
`ExternalPaths`. Dragging out would mean writing the COM side by hand. Cut from
v1.

**No text input.** GPUI ships no `TextInput`. It gives the plumbing
(`EntityInputHandler`, `ElementInputHandler`, `Window::handle_input`) and a
746-line hand-rolled example at `examples/input.rs`. This is one reason to take
gpui-component.

What GPUI does give for free: `App::reveal_path`, `App::open_with_system` and
`App::open_url`, all implemented on Windows. No extra crate needed.

## The gpui-pre situation

crates.io `gpui` is stalled at 0.2.2, released 2025-10-22, published by
zed-industries. Nothing newer.

`gpui-pre` is a **republished snapshot of Zed's main branch**, published by
huacnlee, who maintains gpui-component. Not by Zed. Its description says
"snapshot of zed@d89e9c2". A CI job publishes a new snapshot every Sunday at
18:00 Beijing time. No person reviews the delta. There is no changelog, and Zed
publishes none either.

gpui-component 0.6.x depends on `gpui-pre`, not on `gpui`. The two crate names
are different, so their types are different to the compiler. You cannot mix
them. The choice is all or nothing.

The library target of `gpui-pre` is still named `gpui`, so
`gpui = { package = "gpui-pre", version = "0.3" }` keeps every `use gpui::...`
path in the tree working.

This is why every version is pinned exactly. See issue 02.

## Porting gpui-video

The fork is `freesurfacemodules/gpui-video` at `1195fe7`, itself a fork of
`xhzrd/gpui-video`. Two recent commit messages record unresolved playback bugs
("audio sync is now good but video stops after 2 seconds"). Expect to fix
things.

1,751 lines total. **All GPUI coupling is in `src/element.rs`**, about 250 real
lines. `src/video.rs` is 1,219 lines of ffmpeg-next, cpal and crossbeam with
zero GPUI references.

The one confirmed API break from gpui 0.2.2 to gpui-pre 0.3.5:

    Window::paint_image(bounds, image_bounds, corner_radii, data, frame_index, grayscale)

It gained `image_bounds`. The crate passes 5 arguments. On Windows this is the
only paint path, because `surface()` and the zero-copy `CVPixelBuffer` route are
macOS only. Everything else the crate uses matched signature for signature: the
`Element` trait's five methods, `Window::use_state`, `RenderImage`,
`App::drop_image`, `request_animation_frame`.

Useful API it already has: `seek(Position, accurate)`, `set_looping`,
`set_muted`, `set_speed`, `current_frame_data()`. It has no thumbnail API, which
is why thumbnails come from the FFmpeg CLI instead.

The README documents a `take_frame_ready()` that does not exist in the source.

## Why rusqlite and not sqlx

GPUI has **no async runtime to share**. gpui-pre depends on `async-task` and
`futures` only. On Windows its dispatcher posts work to the WinRT thread pool
and the main thread through `PostMessageW`. There is no reactor for sqlx to hook
into, so any sqlx runtime starts its own threads next to GPUI's.

**Zed does not use sqlx for local storage.** The editor talks to
`libsqlite3-sys` directly through its own `sqlez` wrapper, synchronously, on the
GPUI background executor. sqlx appears only in Zed's server, against Postgres
with tokio. Zed's local situation is ours.

If sqlx were used anyway: 0.9 added `runtime-smol`, which drives its own
executor and needs no runtime context. `runtime-tokio` would panic when called
from `cx.background_spawn` without a bridge like Zed's `gpui_tokio` crate.

FTS5 is compiled in either way. `libsqlite3-sys`'s bundled build passes
`-DSQLITE_ENABLE_FTS5` unconditionally, and bundled means static linking, so no
`sqlite3.dll` to ship. The `trigram` tokenizer has been part of FTS5 core since
SQLite 3.34 and needs no flag, which matters only if typo tolerance is ever
added.

## FFmpeg

The vendored build is **ffmpeg 8.1.2-full_build from gyan.dev**, dated
2026-06-27, shared 64-bit, GPL v3. `ffmpeg.exe`, `ffprobe.exe` and `ffplay.exe`
are all present in `ffmpeg/bin`, next to the DLLs. `build.rs` copies the DLLs to
the target directory.

It is built with **`--enable-whisper`**, so transcription needs no new Rust
dependency, only a `ggml-*.bin` model file. It also has NVENC, NVDEC, CUDA,
D3D11, D3D12, DXVA2 and Vulkan.

**FFmpeg reads standard input.** In a loop that feeds it file names, it eats the
loop's input and silently corrupts a path. This happened during the OCR spike.
Pass `-nostdin` on every call.

## Windows OCR

Measured in issue 01, which has the numbers. Short version: it reads overlay
captions well, it almost never invents text, it costs about 20 ms per frame, and
the language packs installed on this machine are `en-GB`, `en-US` and `pl`.

The `windows` crate 0.62 has no blocking `get()` on `IAsyncOperation`. The
operations implement `IntoFuture`. The spike blocks with `pollster::block_on`.
Working call sequence is in `spike/ocr/src/main.rs`.

## Hardware this runs on

RTX 4070 SUPER with 12 GB, Ryzen 9 7950X with 16 cores and 32 threads, 64 GB of
RAM, Windows 10 Pro 19045. Model size is not a constraint. If the FFmpeg whisper
filter is too slow on CPU, a CUDA build of whisper.cpp is the fallback, but
measure before going there.
