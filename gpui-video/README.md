# gpui-video (vendored)

Project code, not a dependency. It is adapted for file-buddy and is not kept in
sync with upstream.

| | |
| --- | --- |
| Upstream | https://github.com/freesurfacemodules/gpui-video |
| Revision | `1195fe7` ("Fix video-only decode throttling") |
| Vendored | 2026-09-14, for issue 02 |

Upstream is itself a fork of [xhzrd/gpui-video](https://github.com/xhzrd/gpui-video).

## Local changes

### Migration

- `gpui` is `gpui-pre` through `gpui-kit`, not crates.io `gpui` 0.2.
- `element.rs`: `Window::paint_image` gained an `image_bounds` parameter. The
  frame is already fitted, so `bounds` and `image_bounds` are the same rectangle.
- Removed upstream `.git`, `.cargo/config.toml`, `.github`, `renovate.json`, the
  sample media under `assets/` and the examples, which used the old
  `gpui-component` 0.2 API. The test application in `src/main.rs` replaces them.
  The build configuration comes from the workspace root.
- `build.rs` copies the FFmpeg DLLs next to the binaries, including `deps/` so
  the tests can run. It replaces the copy that was in the workspace `build.rs`.

### Playback fixes

Four defects made the picture freeze a moment after playback started while the
audio kept running. `tests/playback.rs` covers all four.

- Decode throttling was `video_buffer_has_room || audio_ring_has_room`. The
  audio ring holds two seconds, so the condition was almost always true and the
  decoder ran the whole clip at full speed. Both buffers are now a hard wall.
- A full frame buffer dropped its front, which is the frame about to be
  presented. It now drops the newest frame instead.
- End of stream was treated as the end of playback. The buffers must play out
  first, or every clip is cut short by the buffer depth.
- `Input::seek` takes AV_TIME_BASE microseconds, but the timestamp was converted
  to the video stream's time base, so a seek landed at a fraction of the target.
  The audio clock also ignored the seek offset, because it was `samples_played`
  alone and a seek zeroes that counter. It now counts from a `clock_base`.
- The audio clock froze whenever the ring ran dry, which always happens at the
  end of a clip. A frozen clock never reaches the timestamps of the frames still
  buffered, so they were never presented and the clip never looped: the picture
  stopped while the audio played out. Wall time now carries the clock over the
  gap. `position()` reports the one clock the decoder publishes.

- The wall time fallback above ran between every audio device callback, because
  the clock is simply still between blocks. It raced ahead and snapped back each
  time, which read as a jittering timestamp and made the presenter skip and hold
  frames. It now waits out `AUDIO_STALL_GRACE` and the clock is monotonic.
- A seek cleared the buffers on the calling thread, but the decoder kept going
  until it read the command, so the front of each buffer could hold pre-seek
  data. The decoder clears them again when it handles the seek.

### Performance

- `element.rs` converted NV12 to BGRA and uploaded a new texture on every window
  frame for every video on screen, paused ones included. It now keeps the
  converted frame until `Video::frame_seq` changes.
- Only a playing video asks for the next animation frame. A wall of paused
  thumbnails used to hold the window at the display refresh rate forever.

### The frame rate decay

Playback started at the clip's own frame rate and decayed over about ten
seconds, to a third or less, then reset on every loop. Three faults compounded:

- `ResampleContext::run` allocates its output frame with the INPUT sample count.
  Upsampling a 44.1 kHz source to a 48 kHz device therefore left about 9 % of
  every packet inside the resampler, and `flush` does not allocate at all, so
  draining it recovered nothing. The audio came out steadily shorter than the
  video. `decode_next_packet` now sizes the output frame for the output rate.
- The audio ring drives the clock, so a short ring forced continuous decoding.
- Decoding was paced by a frame count, and the frame buffer is a frame count, so
  its span depends on the frame rate: 30 frames is 1.2 s at 25 fps but 0.5 s at
  60 fps. The video buffer filled first, the forced audio decode pulled video
  frames in with no room to put them, and they were thrown away. The picture
  became a sparse subset of what had been decoded. Decoding is now paced by how
  far each stream leads the clock, and the lead is scaled to fit the buffer.

`RUST_LOG=gpui_video=debug` prints one line a second per decoder with packet,
present, skip and drop counts and both lead times. That is what made this
legible, and it is worth reaching for first on any playback fault.

### The race on re-hover

Hovering a tile that had been hovered before sometimes played the whole clip at
once, then restarted normally, with the audio rushing after a while.

The stall fallback above measures wall time, and the audio device consumes
nothing while paused, so an idle tile was indistinguishable from a ring that had
run dry. The clock carried the entire idle gap on the first frame after a
resume, the presenter passed every frame at once on its way to catch up, and the
decoder ran flat out because the frames were all behind that clock. The decoder
now restarts the stall timer when it resumes.

It is a narrow race: if the audio device consumes a block before the decoder
looks at the clock, the fallback never engages. `repeated_hover_cycles_never_race`
runs the cycle 25 times with a varying idle, and samples the clock throughout
the resume rather than counting presented frames, because a clock that leaps
forward makes the decoder *skip* the frames it has passed, and a skip is not a
present.

### Known remaining

- Restarting a loop drains and refills both buffers, so a short clip hitches
  once per loop. Seamless looping needs the decoder to seek back and carry on
  with a timestamp offset, rather than waiting for the buffers to empty.
- The audio clock assumes two output channels. The resampler always produces
  stereo, so a device that is not stereo will not sync.

The upstream README follows.

---

# gpui-video

A high-performance video player library for [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) applications, powered by FFmpeg and CPAL. This library provides efficient video playback with hardware-accelerated decoding, synchronized audio playback, and optimized frame presentation.

> **Note**: This is a fork of [gpui-video-player](https://github.com/cijiugechu/gpui-video-player) originally created by [@cijiugechu](https://github.com/cijiugechu). The original implementation used GStreamer for video decoding. This fork switches to FFmpeg and CPAL to reduce resource consumption and improve cross-platform compatibility. GStreamer, while powerful, can consume significant system resources and has complex dependency chains. FFmpeg provides a more lightweight solution with comparable functionality and better performance characteristics for most use cases.

![Video Player Demo](./assets/screenshot.png)

## Features

- 🎬 **FFmpeg-powered**: Robust video decoding supporting a wide range of formats and codecs
- 🎵 **CPAL audio**: Cross-platform audio playback with low latency
- 🚀 **Hardware acceleration**: Automatic hardware decoder selection when available
- 🎯 **Precise A/V sync**: Audio-clock based synchronization for frame-accurate playback
- 🔄 **Frame buffering**: Configurable buffer for smooth playback
- 🎨 **NV12 optimization**: Direct NV12 pixel format output for efficient rendering
- 🍎 **macOS Metal**: CVPixelBuffer rendering on macOS for zero-copy GPU upload
- 🔁 **Looping support**: Seamless video looping
- ⚡ **Multi-threaded**: Non-blocking decoder thread with FFmpeg frame threading
- 🎛️ **Full controls**: Play, pause, seek, speed, volume, and mute controls
- 💾 **Lightweight**: Way lower memory footprint compared to GStreamer-based solutions

> **Looking for the GStreamer version?** Check out the [original repository](https://github.com/cijiugechu/gpui-video-player) by [@cijiugechu](https://github.com/cijiugechu).

## Installation

### FFmpeg Dependencies

This library requires FFmpeg 4.0+ libraries to be installed on your system:

**macOS:**
```bash
brew install ffmpeg
```

**Ubuntu/Debian:**
```bash
sudo apt-get install libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libavfilter-dev
```

**Arch Linux:**
```bash
sudo pacman -S ffmpeg
```

**Windows:**
Download FFmpeg shared libraries from [ffmpeg.org](https://ffmpeg.org/download.html) and set the environment variables in `.cargo/config.toml`:
```toml
[env]
FFMPEG_DIR = "C:/path/to/ffmpeg"
FFMPEG_LIBS_DIR = "C:/path/to/ffmpeg/lib"
FFMPEG_INCLUDE_DIR = "C:/path/to/ffmpeg/include"
```

### Add to Your Project

```toml
[dependencies]
gpui-video = "0.1"
```

Or from GitHub:
```toml
[dependencies]
gpui-video = { git = "https://github.com/xhzrd/gpui-video" }
```

## Quick Start

```rust
use gpui::{App, Application, Context, Render, Window, WindowOptions, div, prelude::*};
use gpui_video::{Video, video};
use std::path::PathBuf;
use url::Url;

struct VideoPlayer {
    video: Video,
}

impl Render for VideoPlayer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                video(self.video.clone())
                    .id("main-video")
                    .buffer_capacity(30)
            )
    }
}

fn main() {
    env_logger::init();
    Application::new().run(|cx: &mut App| {
        let uri = Url::from_file_path(
            PathBuf::from("./video.mp4")
        ).expect("invalid file path");

        cx.open_window(
            WindowOptions {
                focus: true,
                ..Default::default()
            },
            |_, cx| {
                let video = Video::new(&uri).expect("failed to create video");
                cx.new(|_| VideoPlayer { video })
            },
        ).unwrap();
        cx.activate(true);
    });
}
```

## Usage Examples

### Basic Playback Control

```rust
use std::time::Duration;

// Play/Pause
video.set_paused(true);   // Pause
video.set_paused(false);  // Play

// Seeking
video.seek(Duration::from_secs(30), false)?;  // Seek to 30s
video.restart_stream()?;                       // Restart from beginning

// Volume Control
video.set_volume(0.75);   // 75% volume
video.set_muted(true);    // Mute audio

// Playback Speed
video.set_speed(1.5)?;    // 1.5x speed
video.set_speed(0.5)?;    // 0.5x slow motion

// Query State
let pos = video.position();
let dur = video.duration();
let paused = video.paused();
let has_audio = video.has_audio();
```

### Advanced Configuration

```rust
use gpui_video::{Video, VideoOptions};

let options = VideoOptions {
    frame_buffer_capacity: Some(60),  // Buffer 60 frames
    looping: Some(true),               // Enable looping
    speed: Some(1.0),                  // Normal speed
    prebuffer_frames: Some(10),        // Pre-buffer 10 frames
};

let video = Video::new_with_options(&uri, options)?;
```

### Looping Playback

```rust
// Enable looping during initialization
let video = Video::new_with_options(
    &uri,
    VideoOptions {
        looping: Some(true),
        ..VideoOptions::default()
    }
)?;

// Or enable at runtime
video.set_looping(true);
```

### Custom Display Size

```rust
// Set custom display dimensions
video.set_display_width(Some(1280));
video.set_display_height(Some(720));

// Or set both at once
video.set_display_size(Some(1920), Some(1080));

// Get effective display size
let (width, height) = video.display_size();
```

### Video Element Customization

```rust
use gpui_video::video;

let video_element = video(my_video)
    .id("custom-video")
    .size(px(800.0), px(600.0))  // Set element size
    .buffer_capacity(45);         // Configure buffer
```

## API Reference

### `Video`

The main video player struct:

**Playback Control:**
- `set_paused(bool)` - Pause or resume playback
- `paused() -> bool` - Check if paused
- `seek(Position, accurate: bool)` - Seek to position
- `set_speed(f64)` - Set playback speed
- `speed() -> f64` - Get current speed
- `restart_stream()` - Restart from beginning

**Audio Control:**
- `set_volume(f64)` - Set volume (0.0 to 1.0)
- `volume() -> f64` - Get current volume
- `set_muted(bool)` - Mute or unmute
- `muted() -> bool` - Check if muted
- `has_audio() -> bool` - Check if video has audio track

**Position & Timing:**
- `position() -> Duration` - Get current playback position
- `duration() -> Duration` - Get total duration
- `framerate() -> f64` - Get video framerate
- `eos() -> bool` - Check if end of stream

**Display:**
- `size() -> (i32, i32)` - Get video resolution
- `aspect_ratio() -> f32` - Get aspect ratio
- `display_size() -> (u32, u32)` - Get effective display size
- `set_display_size(Option<u32>, Option<u32>)` - Set display override
- `set_display_width(Option<u32>)` - Set width override
- `set_display_height(Option<u32>)` - Set height override

**Frame Buffer:**
- `set_frame_buffer_capacity(usize)` - Set buffer size (0 disables)
- `frame_buffer_capacity() -> usize` - Get buffer capacity
- `buffered_len() -> usize` - Get current buffer length
- `pop_buffered_frame()` - Pop oldest buffered frame
- `current_frame_data()` - Get current frame data
- `take_frame_ready() -> bool` - Check if new frame ready

**Looping:**
- `set_looping(bool)` - Enable or disable looping
- `looping() -> bool` - Check if looping enabled

### `VideoElement`

GPUI element for rendering video:

- `id(ElementId)` - Set element ID
- `size(Pixels, Pixels)` - Set both width and height
- `width(Pixels)` - Set width (height inferred from aspect ratio)
- `height(Pixels)` - Set height (width inferred from aspect ratio)
- `buffer_capacity(usize)` - Set frame buffer capacity

### `Position`

Time or frame-based positioning:

```rust
use gpui_video::Position;
use std::time::Duration;

let time_pos = Position::Time(Duration::from_secs(10));
let frame_pos = Position::Frame(300);
```

### `VideoOptions`

Configuration options for video initialization:

```rust
pub struct VideoOptions {
    pub frame_buffer_capacity: Option<usize>,  // Default: 30
    pub looping: Option<bool>,                 // Default: false
    pub speed: Option<f64>,                    // Default: 1.0
    pub prebuffer_frames: Option<usize>,       // Default: 5
}
```

## Performance Optimization

### Frame Buffering

The library uses a configurable frame buffer to balance memory usage and playback smoothness:

```rust
// High buffer for smooth playback (more memory)
video.set_frame_buffer_capacity(60);

// Low buffer for memory-constrained environments
video.set_frame_buffer_capacity(10);

// Disable buffering for minimal memory (may cause stuttering)
video.set_frame_buffer_capacity(0);
```

### Audio Synchronization

Audio playback uses CPAL with a ring buffer, providing:
- Low-latency audio output
- Precise audio clock for A/V synchronization
- Automatic sample rate conversion
- Zero-allocation audio callback

### Hardware Acceleration

FFmpeg automatically uses hardware decoders when available:
- **macOS**: VideoToolbox (H.264, HEVC)
- **Windows**: DXVA2, D3D11VA
- **Linux**: VAAPI, VDPAU

### Multi-threading

- Separate decoder thread prevents UI blocking
- FFmpeg frame threading for parallel decode
- Lock-free atomics for state management

## Examples

Run the included examples:

```bash
# Basic video player
cargo run --example video_player

# Player with transport controls
cargo run --example with_controls

# Looping playback demo
cargo run --example looping
```

## Platform-Specific Notes

### macOS
- Uses `CVPixelBuffer` for hardware-accelerated rendering when available
- Metal-backed IOSurface for zero-copy GPU upload
- Falls back to software rendering via GPUI sprite atlas

### Linux/Windows
- Uses optimized software rendering via GPUI sprite atlas
- YUV to RGB conversion using `yuvutils-rs` with SIMD optimizations
- Supports various FFmpeg backends (VAAPI, VDPAU, DXVA2, etc.)

## Troubleshooting

### FFmpeg Not Found

If you get FFmpeg linking errors:

1. Ensure FFmpeg is installed
2. Check library paths are correct
3. Set environment variables in `.cargo/config.toml`
4. Verify FFmpeg version is 4.0+

### Audio Issues

- Check default audio device is working
- Verify audio codec is supported by FFmpeg
- Try adjusting buffer size with `VideoOptions`

### Playback Stuttering

- Increase `frame_buffer_capacity` (default: 30)
- Increase `prebuffer_frames` (default: 5)
- Check CPU/GPU utilization
- Verify video codec has hardware acceleration

## Architecture

```
┌─────────────┐
│   Video     │ ← Public API (Clone-able handle)
└──────┬──────┘
       │
       ▼
┌─────────────────────────────────────────┐
│          Internal (Arc<RwLock>)          │
├─────────────────────────────────────────┤
│  • Decoder thread (FFmpeg)              │
│  • Frame buffer (VecDeque)              │
│  • Audio ring buffer (CPAL)             │
│  • Sync primitives (Atomics, Mutex)    │
│  • Command channel (crossbeam)          │
└─────────────────────────────────────────┘
       │                    │
       ▼                    ▼
┌─────────────┐      ┌──────────────┐
│   FFmpeg    │      │     CPAL     │
│  Decoding   │      │ Audio Output │
└─────────────┘      └──────────────┘
```

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

This project is a fork of the original [gpui-video-player](https://github.com/cijiugechu/gpui-video-player) by [@cijiugechu](https://github.com/cijiugechu), which was built on GStreamer. We're grateful for the foundational work that made this library possible.

**Why FFmpeg + CPAL?**

The switch from GStreamer to FFmpeg and CPAL was motivated by several factors:

- **Reduced Resource Usage**: FFmpeg typically uses less memory and CPU compared to GStreamer's pipeline architecture
- **Simpler Dependencies**: FFmpeg has a more straightforward dependency chain, making builds and deployment easier
- **Better Cross-Platform Support**: FFmpeg's dynamic linking model works more consistently across Windows, macOS, and Linux
- **Lighter Weight**: For video playback use cases, FFmpeg provides all necessary functionality without GStreamer's additional overhead
- **CPAL Audio**: Direct audio output via CPAL provides lower latency and simpler integration than GStreamer's audio pipeline

Both implementations have their merits - GStreamer excels at complex media pipelines, while FFmpeg is ideal for straightforward playback scenarios like this library.

**Additional Credits:**
- Original code structure inspired by [iced_video_player](https://github.com/jazzfool/iced_video_player) by [@jazzfool](https://github.com/jazzfool)
- Built with [FFmpeg](https://ffmpeg.org/) for video decoding
- Uses [CPAL](https://github.com/RustAudio/cpal) for cross-platform audio
- Powered by [GPUI](https://github.com/zed-industries/zed) for rendering

## Links

- **Repository**: https://github.com/xhzrd/gpui-video
- **Documentation**: https://docs.rs/gpui-video
- **Issues**: https://github.com/xhzrd/gpui-video/issues
