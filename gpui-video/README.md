# gpui-video

A video player element for GPUI, built on FFmpeg for decoding and cpal for
sound. Project code for file-buddy, Windows only. Vendored from
[freesurfacemodules/gpui-video](https://github.com/freesurfacemodules/gpui-video)
and not kept in sync with it.

## Usage

```rust
// `uri` is a `url::Url`, such as `Url::from_file_path(path)`.
let video = Video::new_with_options(
    &uri,
    VideoOptions { looping: true, frame_buffer_capacity: 30, ..Default::default() },
)?;
video.set_muted(true);
video.set_paused(false);

// In `render`:
div().child(video(self.video.clone()).id("clip").size(px(640.), px(360.)))
```

## Known issues

- **Build:** needs the FFmpeg development files in `ffmpeg/` at the workspace
  root (`FFMPEG_DIR` in `.cargo/config.toml`). `build.rs` copies the DLLs from
  `ffmpeg/bin` next to the binaries. A missing directory gives only a build
  warning, then a failure at run time.
- **Debug builds:** decoding and frame conversion are too slow without
  optimization. Keep `opt-level = 3` for this package.
- **Memory:** each buffered frame is the full size of the clip. Cap
  `frame_buffer_capacity` for large clips and for many videos at once.
- **Looping:** a short clip hitches once per loop, because the buffers drain and
  refill.
- **Audio:** sync assumes a stereo output device.
- **Diagnosis:** `RUST_LOG=gpui_video=debug` logs packet, present, skip and drop
  counts and the stream leads once a second per decoder. Start there for any
  playback fault. `tests/playback.rs` covers the known ones.
