# 18 - whisper-rs worker

Status: done

Depends on nothing in v2. Read the Worker processes and Speech sections of the
v2 spec first.

## Work

- Add `whisper-rs` 0.16.0 with the `vulkan` feature, pinned exactly. It needs
  CMake, LLVM (clang, `LIBCLANG_PATH`) and the Vulkan SDK. Ask the user to
  install any that are missing. Do not install them yourself.
- Add the worker process: `fb` started with a subcommand, long-lived, one per
  model stage. It loads its model once and takes files over a pipe. CLIP reuses
  this in 20, so keep the model code apart from the process code.
- If the worker stops, the application starts a new one. The job loses one of
  its two attempts.
- FFmpeg decodes the first audio track to 16 kHz mono `f32le` on a pipe. The
  worker never opens a media file.
- Keep the v1 rule filter as it is.
- Store the mean token log probability in `segments.confidence`, and store
  `no_speech_prob`. Drop nothing by threshold.
- A missing model still stops the stage before any job runs.
- Remove `src/ffmpeg/whisper.rs` and the filter path.

## Measure

Over the 23 clips in `test_videos`, against the filter: total time, time per
clip, and a diff of the transcripts. The filter took 61 to 67 s, and about
50 s of that was model load. Record the numbers here.

If Vulkan is slower than the filter, stop and tell the user before trying CUDA.

Do not re-transcribe the collection in this issue. That waits for 19.

## Comments

### Measurements, 2026-09-24

22 unique clips in `test_videos` (the "- Copy" file is a duplicate and is not
transcribed again). Debug build of `fb`. The dependencies and whisper.cpp are
optimized. Total is `fb scan` with every other stage done, so it is the whisper
stage alone.

| | Filter, concurrency 2 | Worker, concurrency 1 |
| --- | --- | --- |
| Total | 54.4 s, 51.9 s | 13.4 s (first run), 9.8 s, 10.0 s |
| Model load | about 2.2 s per clip | 1.6 s once, with process start |
| Per clip | 2.5 s | 0.17 to 1.62 s, mean 0.35 s (7.6 s for all 22) |

The per-clip times come from a script that drives `fb worker whisper` directly.

Vulkan is about 5 times faster than the filter. CUDA is not needed.

Transcript diff, with the same rule filter on both sides:

- 6 clips have no speech on both sides. Of the 16 with speech, 13 give the
  same words (one differs only by a comma).
- Segment starts move by up to 2 s in some clips (`im snacking`, `i remember
  the 35 sweet goodbyes`, `who in my family`). The worker decodes the whole
  file, not 30 s windows, so a segment boundary is not forced at each window
  edge.
- Longer sentences split into more segments in some clips. The words are the
  same.
- `my honest reaction` has small word changes: "looking only" became "looking
  lonely", "what I would" became "what I want".
- `walter the general` gains 5 segments of background voices that the filter
  did not give ("Fuck you!", "Hate your eyebrows!", "You can't keep getting away
  with it!" and others). Not checked by ear. Listen before the VAD work in 19.
- `we are in a recession` has small word changes ("leave him alone" became
  "leave them alone") and gains "Ugh." at 136.0 s, confidence -0.25.

Probabilities:

- `confidence` over 118 segments: -0.96 to -0.00001. The lowest are short
  exclamations ("Oh!" -0.96, "Oh." -0.65, "Nyeeeah!" -0.50).
- `no_speech_prob` is per decoding window, so the segments of one window share
  it. On these clips it is 1e-12 to 4e-9. No clip here is silence that the model
  filled with words, so its range on bad segments is still unknown.

Restart: the worker was killed with `taskkill` during a run. The job it held
failed with "the worker stopped (exit code: 1): no answer", used its second
attempt and finished. All 22 jobs ended done.

### Build notes

- whisper.cpp fails to build with CMake's Visual Studio generator: MSBuild's
  file tracker hits the 260 character path limit in the Vulkan shader build.
  `.cargo/config.toml` sets `CMAKE_GENERATOR = "Ninja"`. After a failed build
  with the other generator, delete `target/*/build/whisper-rs-sys-*/out/build`.
- A shipped build needs `fb.exe` next to `file-buddy.exe`. The window starts
  the worker from there. `FILE_BUDDY_FB` overrides the path, which the
  integration test uses.
- Whisper concurrency is 1. One worker process takes one file at a time.

