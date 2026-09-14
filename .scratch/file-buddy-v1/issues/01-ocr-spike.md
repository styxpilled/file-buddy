# 01 - OCR spike: Windows.Media.Ocr

Status: done

Result: **go**. `Windows.Media.Ocr` is good enough. Use it. Do not add
PaddleOCR.

The spike code is in `spike/ocr/`. It is throwaway, but keep it until issue 10
is done, because it is the fastest way to test a change to frame sampling.

## Method

21 clips from the user: 15 under `test_videos/text/`, 6 under
`test_videos/no_text/`. Frames extracted with the vendored FFmpeg, then read
with `Windows.Media.Ocr` through `windows-rs` 0.62.

Three sampling strategies were compared, because the first result was a
sampling failure, not an OCR failure.

| Sampling | Clips with text found | False text in `no_text` |
| --- | --- | --- |
| Scene change, threshold 0.3, max 10 | 9 / 15 | 2 lines in 32 frames |
| `fps=1`, max 10 | 14 / 15 | 1 line in 45 frames |
| **10 frames spread over the duration** | **14 / 15** | **1 line in 60 frames** |

## Findings

**Accuracy is good on overlay captions.** Full sentences come back clean:
"Meet the silky anteater, the smallest of the anteaters", "we are in a
recession / whether anyone wants to admit that or not", "Blud thinks we're
gonna be alive", and the whole 7-line caption of `prod moment`. Words are
sometimes damaged at low resolution, but enough correct words survive for
search, which is all this signal must do.

**Precision is excellent.** Across 60 frames of clips with no text, OCR
returned one junk fragment. It does not invent text. This was the user's main
doubt and it is answered.

**Scene detection at 0.3 is the wrong sampler.** It gave 1 or 2 frames for half
the clips, which is why 6 clips looked like OCR failures. Every one of those was
recovered by better sampling. See issue 10.

**The only true miss** is `back at it again at krispy kreme`: curved script on a
crumpled paper hat at 480x480, not an overlay caption. Dense sampling at 4 fps
did not help. Accept it. Incidental text on a physical object is out of scope.

**Speed is not a concern.** About 20 ms per frame, so about 200 ms per clip.
For a few thousand clips that is minutes of OCR. Frame extraction costs more
than the recognition.

## Noise, and what to do about it

- **Platform watermarks.** "TikTok" was the most frequent line in the whole set,
  23 times. bm25 lowers the weight of a term that appears in most documents, so
  this mostly solves itself. No stoplist for v1.
- **Creator handles** (`@mangoslug`, `@ladymisskay_`) come back reliably. Keep
  them. They are a real thing to search for.
- **Charts and numbers produce a flood.** One clip of a slide with float numbers
  gave 311 unique lines. A simple filter fixes it: drop a line with fewer than 3
  letters, or where letters are less than half of the non-space characters. On
  the full set it removed 98 of 764 lines, nearly all numeric junk.
- **The same caption repeats across frames.** Deduplicate per file before
  indexing, as the spec already says.

## Environment note

The machine has `en-GB`, `en-US` and `pl` OCR languages installed.
`TryCreateFromUserProfileLanguages` gives `en-US`.

## One bug found, relevant to the whole pipeline

FFmpeg reads stdin and ate the input of the shell loop that was feeding it file
names, which silently truncated one path. **Every FFmpeg call in the application
must pass `-nostdin`.**
