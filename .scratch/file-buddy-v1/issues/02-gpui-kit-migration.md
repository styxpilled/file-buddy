# 02 - Migrate to gpui-kit and prove the video player

Status: ready-for-human

The video component is the part most likely to fight back, so it comes first.
The migration to `gpui-kit` touches the same file, so both happen together.

## Work

1. Replace `gpui = "0.2.2"` with the `gpui-kit` 0.6.1 umbrella. Pin exactly.
   The library target of `gpui-pre` is named `gpui`, so `use gpui::...` paths do
   not change.
2. Vendor `gpui-video` as project code: remove its `.git`, commit the source,
   and record the upstream revision (`1195fe7`, fork
   `freesurfacemodules/gpui-video`) in a README inside the crate.
3. Port `gpui-video/src/element.rs`. The known break is `Window::paint_image`,
   which gained an `image_bounds` parameter. The crate passes 5 arguments and
   needs 6. On Windows this is the only paint path, because `surface()` is
   macOS only. `src/video.rs` has no GPUI code and should not need changes.
4. Call `gpui_component::init(cx)` at start. Wrap the window in `Root`.
   Register the bundled Lucide assets as the `AssetSource`.
5. Prove the player in a small test application: seek, loop, mute, start and
   stop on hover, and several instances in one window with distinct element ids.

## Done when

Hover over a tile starts a looping muted clip, moving away stops it, and nothing
stalls after 2 seconds. The fork's own history records playback bugs of this
kind, so test it, do not assume it.

## Risk

The upstream API drifted for 11 months and has no changelog. Other breaks may
appear at compile time. They are expected and in scope.

## Comments

### 2026-09-14, agent

Work items 1 to 5 are done. `cargo check --all-targets` and `cargo clippy` are
clean for project code.

The migration itself was almost free. `Window::paint_image` was the only API
break in 11 months, exactly as predicted. `gpui-kit` 0.6.1 resolves `gpui-pre`
0.3.1, and `gpui-pre` is declared beside it in both manifests under the package
rename, so `use gpui::...` paths are unchanged. `Application::new()` is gone;
the entry point is `gpui_kit::application()`.

The test application is `src/main.rs`. It walks `test_videos/`, shows up to six
tiles with distinct element ids, plays on hover, rewinds on leave, and has
buttons for rewind, seek, mute and loop. A 250 ms tick redraws the per tile
status line, so a stalled decoder is visible as a frozen counter.

The player did not work. It froze a moment after playback started while the
audio kept running, which is the failure the issue warned about. Four separate
defects, all now fixed and covered by `gpui-video/tests/playback.rs`:

1. Decode throttling was `video_buffer_has_room || audio_ring_has_room` over a
   two second audio ring, so the decoder ran the whole clip at full speed.
2. A full frame buffer dropped its front, which is the frame about to show.
3. End of stream was treated as the end of playback, cutting every clip short
   by the buffer depth.
4. `Input::seek` takes AV_TIME_BASE microseconds, but the timestamp was
   converted to the video stream's time base, so a seek landed at a fraction of
   the target. The audio clock also ignored the seek offset.

A debug build is far too slow for frame conversion, so `Cargo.toml` sets
`opt-level = 3` for `gpui-video` and every dependency in the dev profile.

The FFmpeg DLL copy moved from the workspace `build.rs` to `gpui-video/build.rs`,
with the crate that links FFmpeg, and now also copies into `deps/` so tests run.

### Open

- The audio clock assumes two output channels. The resampler always makes
  stereo, so a device that is not stereo will not sync.
- Each tile opens its own CPAL output stream. Six streams on the default device
  works, but a wall of tiles in the real grid may not. Consider one shared
  stream, or no audio at all for grid previews.
- Delete `gpui-video/assets` (14 MB of upstream sample media), `gpui-video/examples`
  (upstream examples on the gpui-component 0.2 API) and the now unused workspace
  `build.rs`. They are gitignored and not built.

### 2026-09-14, agent, second round

Three reports from the first build: the video froze after an indeterminate time,
audio did not restart on re-hover, and some videos overflowed their borders.

The freeze was a fifth decoder defect, reproduced in six seconds by a soak test
once the frame buffer is capped at 30 frames the way the element caps it. The
audio clock is `samples_played`, so it stops the moment the ring runs dry, which
always happens at the end of a clip. The frames still buffered carry timestamps
the frozen clock never reaches, so they are never presented, the buffer never
drains, and the loop never fires. Wall time now carries the clock over the gap.
`position()` reports the one clock the decoder publishes, for both clocks.

The audio restart follows from the same defect: the clip was wedged at end of
stream, so a stop and rewind had nothing to rewind. `stop_and_rewind_restarts_playback`
covers the hover cycle.

The overflow was the test application, not the element. `w` and `h` size the
border box, so a 320 by 180 tile with a 2 px border has a 316 by 176 content box.
The video element now gets the content box.

`does_not_freeze_over_a_minute` is `#[ignore]`d because it takes a minute. Run it
with `cargo test -p gpui-video --test playback -- --ignored`.

### 2026-09-14, agent, third round

The wall time fallback from the second round was too eager. The audio device
hands over a block at a time, so the clock is still between callbacks, and the
fallback treated every one of those gaps as a stall: it raced ahead and snapped
back on each callback. That is the jittering timestamp, and the presenter
skipping and holding frames against a sawtooth clock is the choppiness. The
fallback now waits out a 120 ms grace period, and the clock cannot step
backwards. `the_clock_is_monotonic_and_steady` and
`frames_arrive_at_about_the_frame_rate` cover both.

Two further causes of choppiness, in the paint path rather than the decoder:

- The element converted NV12 to BGRA and uploaded a new texture on every window
  frame for every video on screen, paused ones included. It now keeps the
  converted frame until `Video::frame_seq` changes.
- Every element asked for the next animation frame unconditionally, so six
  paused thumbnails held the window at the display refresh rate forever. Only a
  playing video asks now.

For the stale audio on re-hover: a seek cleared the buffers on the calling
thread, but the decoder kept decoding until it read the command, so the front of
each buffer could hold pre-seek data. The decoder clears them again when it
handles the seek. If a short tail survives this, the remaining suspect is the
audio the OS already holds, which CPAL opens with `BufferSize::Default`.

### 2026-09-14, agent, fourth round

The slowdown reproduced headlessly once the diagnostic ran against a 60 fps clip
rather than the 25 fps one the tests defaulted to. Presented frames decayed from
62 a second to 19 over fourteen seconds, resetting on every loop, while the
clock stayed accurate. A once a second decoder report made the cause plain:
`video_lead` climbed from 500 ms to 1353 ms while `audio_lead` sat pinned at the
floor and frames were discarded at 42 a second.

Three faults compounded:

1. `ResampleContext::run` allocates its output frame with the input sample
   count. Every 44.1 kHz source into a 48 kHz device left about 9 % of each
   packet inside the resampler, and `flush` does not allocate, so draining it
   recovered nothing. The audio was steadily shorter than the video.
2. Short audio starved the ring that drives the clock, which forced decoding.
3. The decode pace was a frame count, and the frame buffer is a frame count, so
   30 frames is 1.2 s at 25 fps but 0.5 s at 60 fps. The video buffer filled
   first, and the forced audio decode pulled in video frames with nowhere to put
   them. They were discarded, and the picture became a sparse subset.

Decoding is now paced by how far each stream leads the clock, with the lead
scaled to fit the frame buffer. The 60 fps clip holds 60 presented frames a
second for 30 seconds in the application, and the decoder report shows both
leads locked at about 300 ms with no drops.

Also fixed: a thumbnail paused before its first frame was ever presented stayed
blank, because presenting only happens on the playing path.

`the_presented_frame_rate_holds` runs over four clips of different frame rates
and sample rates, because the default fixture alone never showed this.

### Still open

- Restarting a loop drains and refills both buffers, so a short clip hitches
  once per loop. Hover previews loop constantly, so this is worth fixing.
- Stale audio on re-hover was reported again after the third round. The
  resampler fix changes what the ring holds, so it needs re-testing before
  chasing further.

### 2026-09-14, agent, fifth round

Hovering a tile that had been hovered before sometimes played the whole clip at
once, then restarted normally, with the audio rushing after a while.

The stall fallback measures wall time, and the audio device consumes nothing
while paused, so an idle tile was indistinguishable from a ring that had run
dry. The clock carried the whole idle gap on the first frame after a resume:
measured at 796 ms of clock after 20 ms of playing. The presenter then passed
every frame at once on its way to catch up, and the decoder ran flat out because
every frame was behind that clock, which is the audio rush as well. The decoder
now restarts the stall timer when it resumes.

The first two tests written for this passed before the fix, because a clock that
leaps forward makes the decoder skip the frames it has passed, and a skip is not
a present. `repeated_hover_cycles_never_race` samples the clock throughout the
resume instead, and was confirmed to fail with the fix removed.
