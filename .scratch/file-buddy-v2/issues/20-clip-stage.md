# 20 - CLIP stage and storage

Status: ready-for-agent

Depends on 18, for the worker process. Read the CLIP section of the v2 spec
first. Replaces v1 issue 17.

Needs from the user: the MobileCLIP2-S2 ONNX files and tokenizer in `models/`,
and a pinned DirectML build of `onnxruntime.dll` for the repository. Ask which
exact versions before adding anything.

## Work

- Add `ort` 2.0.0-rc.13 with `load-dynamic` and the DirectML execution provider,
  and `tokenizers`, both pinned exactly. Vendor the DLL like `ffmpeg/bin`, and
  have `build.rs` copy it. The build must download nothing.
- Write the image and text encoders against `ort` directly. No wrapper crate.
- Add the `clip` stage: `probe -> thumbnail -> ocr -> clip -> whisper`. It
  samples frames the way `ffmpeg::frames` does. FFmpeg scales, crops and pipes
  raw `rgb24` at the size the model config asks for. A still gives its single
  image.
- The image encoder runs in a `clip` worker. The text encoder runs in the window
  process, on the CPU.
- Store one row per frame: hash, time of the frame, and the normalized vector as
  a BLOB. The rows go when the file goes, as segments do.
- The 5-minute limit applies.
- A missing model behaves like a missing whisper model.
- Show `clip N / M` in the header, as for speech.

## Measure

Time per file on GPU and on CPU, the total over `test_videos`, and the memory
the worker uses. Then run 20 topic queries over the real collection with a
plain cosine top 20, and show the user the results before 21 tunes anything.

## Comments
