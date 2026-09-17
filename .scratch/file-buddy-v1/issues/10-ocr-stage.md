# 10 - OCR stage

Status: done

Unblocked. Issue 01 says **go**: `Windows.Media.Ocr` finds the text in 14 of 15
clips and almost never invents text. PaddleOCR is not needed.

## Work

- Add `ocr` as a stage in the job queue.
- **Sample 10 frames spread over the duration of the file**, that is
  `fps = 10 / duration`, limited to 4 fps for very short clips, and at most 10
  frames. Duration comes from the probe stage.
- Still images use the single image. GIFs are treated like video.
- Run `Windows.Media.Ocr` through `windows-rs` 0.62. The async calls return
  futures, not a blocking `get()`. See `spike/ocr/src/main.rs` for the working
  call sequence: `StorageFile::GetFileFromPathAsync` -> `OpenAsync` ->
  `BitmapDecoder::CreateAsync` -> `GetSoftwareBitmapAsync` -> `RecognizeAsync`.
- Read `OcrResult::Lines()` and keep the line structure. Do not use `Text()`.
- Filter each line: drop it if it has fewer than 3 letters, or if letters are
  less than half of the non-space characters. This removes number and chart
  noise. It removed 98 of 764 lines on the test set, nearly all junk.
- Remove duplicate lines per file, case-insensitively. The same caption appears
  in every frame that shows it.
- Write the result to the `ocr` FTS column, the lowest weight of the four.
- Every FFmpeg call needs `-nostdin`.

Keep the OCR engine behind one interface, so another engine can replace it later
without touching the pipeline.

## Do not

Do not use scene detection with a 0.3 threshold. It gave 1 or 2 frames for half
the test clips and lost the text in 6 of them. This was measured, see issue 01.

## Do not expect

Text that is warped over a physical object, or script fonts at low resolution,
will not be read. That is accepted.

## Comments

Done. The stage is `index/stages/ocr.rs`, the recognizer is behind `ocr::Engine`
with `ocr::windows::WindowsOcr` as the only implementation, and the frames come
from `ffmpeg/frames.rs`.

Measured over the 21 clips of issue 01, through the real pipeline and not the
spike:

- 14 of 15 clips with text give text. The miss is `back at it again at krispy
  kreme`, which issue 01 accepted.
- The 6 clips with no text give one junk line in total, `lifi`.
- Search finds a clip by its on-screen text alone: `insulindian freeloader`,
  `fergalicious`, `cornucopia`.

Two things worth knowing:

- OCR now runs before speech, not after it. It is minutes of work where speech
  is hours, so the window shows text results on the first run instead of waiting
  a night for them. `index::read_text` is the entry point, beside
  `index::transcribe`.
- A migration retries the OCR jobs that failed with "has no worker yet" and
  clears the files they marked failed. Without it, an existing database would
  never read its text and would show every video in Review.

A noise filter followed, on the user's request. `ocr::noise::is_noise` drops a
line that says nothing but the TikTok watermark:
the word mark, the handle under it, or both. A caption that names a handle or
the platform stays, because the line goes only when the watermark is all of it.

Measured over the 21 clips: 20 of 484 lines removed, and `fb search tiktok` now
finds nothing. The clean spellings all go. The mangled ones do not, because the
rule matches the word `tiktok` and not a likeness of it: `ikTok`, `TikTo`,
`RTikTO`, `Tok`, and the handles whose `@` the recognizer dropped, such as
`ladymisskay_` and `mangoslug`. Catching those needs a similarity measure, which
is a separate decision.

This reverses issue 01 on creator handles, which said to keep them.

The rule first ran in the stage, which was wrong: it deleted the lines before
they reached the `ocr` table, so changing it meant reading every file again. It
now runs in `db::fts::refresh`, where the index is built. The table keeps every
line the recognizer read, and `fb reindex` rebuilds the index from it in under a
second for 23 files.
