# 10 - OCR stage

Status: ready-for-agent

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
