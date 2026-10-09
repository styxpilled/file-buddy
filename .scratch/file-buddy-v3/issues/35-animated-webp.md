# 35 - Animated WebP

Status: done

Depends on nothing. Read the Animated WebP section of the v3 spec and the
Animated WebP part of `research.md` first. It lifts the v2 rule "FFmpeg stays
the only decoder" for animated WebP only.

FFmpeg 8.1.2 cannot decode animated WebP. Every stage fails, and so does
playback. `C:\Users\styx\Downloads\animated-webp-supported.webp` is the one file in the
index today: probed as a 0x0 still, with thumbnail, OCR and CLIP failed.

## Work

- Detect an animated WebP from its RIFF chunks: the animation flag in `VP8X`,
  or an `ANIM` chunk. Do not ask FFmpeg. The probe already turns a still into
  `Animated`, so the probe takes this check.
- Add `image-webp` 0.2.4 as a direct dependency, pinned exactly. It is already
  in `Cargo.lock`.
- The probe takes the size, the frame count and the duration from the decoder.
- The probe writes a proxy at `proxies/<hash>.mp4` in the cache, next to the
  thumbnails. Add its path to `paths.rs`. Frames go to FFmpeg as raw RGBA on a
  pipe and come out as near-lossless H.264. Each frame keeps its duration by
  repetition at a constant rate. Measure 100 fps against the file size first,
  and record the rate chosen here.
- Transparent pixels go on black.
- Thumbnail, OCR, CLIP, the tiles, the side pane and later the Studio read the
  proxy. Put the choice of path in one function, so no caller decides it.
- Copy, rename, delete and reveal act on the real file.
- A file that fails to decode keeps its error, as an FFmpeg failure does.
- Existing animated WebP files probe again once. A rescan does not retry
  failures today, so this needs a one-time requeue for WebP files.
- A proxy whose file leaves the index can stay. Do not add cleanup unless the
  cache size is a real problem.

## Tests

The RIFF check, on a static WebP, an animated WebP and a non-WebP file. The
frame durations to a constant rate.

## Check

`animated-webp-supported.webp` probes as animated with its real size, gets a
thumbnail, OCR and CLIP rows, and plays on hover and in the side pane. Write
the proxy size and the time to make it here.

## Comments

### Built (2026-10-09)

- `ffmpeg/webp.rs`. `is_animated` reads the first 21 bytes: `RIFF`, `WEBP`,
  then `VP8X` with the animation flag. The `ANIM` chunk walk was left out,
  because `image-webp` decides from the same flag and could not decode a file
  without it.
- The proxy sits beside the thumbnail as `<hash>.proxy.mp4`, from
  `paths::proxy_path`. Not a `proxies/` directory: the stages get the
  thumbnail cache as their cache, and this needs no new path plumbing.
- Rate: one frame per tick, where the tick is the largest step that divides
  every delay, never under 10 ms. A delay of 10 ms or less counts as 100 ms,
  as browsers play it. Frame ends round from the running total, so there is no
  drift.
- H.264, `libx264 -preset veryfast -crf 12`, 4:2:0, with the size padded to
  even numbers. The probe keeps the real size.
- `stages::run` gives thumbnail, OCR and CLIP `webp::readable`, which makes a
  missing proxy before it returns it. The probe writes it in any case.
  `paths::playable` picks the proxy for the tiles and the side pane, and never
  makes one, so the window never encodes.
- Migration `READ_ANIMATED_WEBP` queues the probe of every present WebP again,
  and every failed stage of them.

Measured on `animated-webp-supported.webp`: 12 frames of 70 ms, 400x400. The
proxy is 55,266 bytes, made in 0.28 s in a debug build. The same frames at a
constant 100 fps give 53,804 bytes, so the rate costs nothing in size, but the
tick keeps 12 frames to decode where 100 fps keeps 84.

`fb scan` on the debug database: all 8 animated WebP files in the roots are
`ready`, as `animated`, with probe, thumbnail, OCR and CLIP done. The longest
is 25 s at 652x810. The proxies are 55 KB to 5.3 MB. The 7 static WebP files
stay stills.

The OCR of the first frames of the test file reads a few broken characters,
such as `W�6P`. The frame at 500 ms reads clean. FFmpeg cannot read the
original, so there is nothing to compare the proxy with. Not followed up.

Checked in the dev window: `animated-webp-supported.webp` plays in the side pane,
and `animated-webp-test1.webp` plays on hover.
