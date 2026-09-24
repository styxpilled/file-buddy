# 20 - CLIP stage and storage

Status: done

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

### Versions, 2026-09-24

- `ort` 2.0.0-rc.13 with default features off: `std`, `load-dynamic`,
  `directml`, `api-24`. With its default `api-27`, rc.13 refuses any
  `onnxruntime.dll` older than 1.27. `api-24` accepts 1.24.
- `onnxruntime.dll` 1.24.4 from the NuGet package
  Microsoft.ML.OnnxRuntime.DirectML. It is the last DirectML build: ONNX
  Runtime went on to 1.30 without one.
- `DirectML.dll` 1.15.4 from Microsoft.AI.DirectML, the version 1.24.4 depends
  on. It is loaded by its full path first, because Windows 10 has an older one
  in System32.
- The three DLLs are in `onnxruntime/bin`, which git ignores like `ffmpeg/`.
  `build.rs` copies them next to the executables.
- `tokenizers` 0.23.2, default features off, with `fancy-regex`, so no C
  library is built.
- Model: `RuteNL/MobileCLIP2-S2-OpenCLIP-ONNX` at commit `5ee5cc15`, in
  `models/mobileclip2-s2`. 512 dimensions, 256 px input, shortest side scaled
  then cut to a square at the center, no mean or deviation (`/255` only), 77
  tokens padded with 0.

### Measurements, 2026-09-24

Image worker over the 22 clips of `test_videos`. The time is for one file,
FFmpeg decode included (10 frames, one batch):

| | GPU, DirectML | CPU |
| --- | --- | --- |
| Start and model load | 0.55 s | 0.40 s |
| Per file | mean 240 ms, 115 to 620 ms | mean 500 ms, 390 to 880 ms |
| Total, 22 files | 5.3 s | 11.0 s |
| Worker memory | 314 MB peak working set, 910 MB private | 526 MB peak, 722 MB private |

The whole stage through `fb scan` on `test_videos`: 6.3 s.

Real collection, on a copy of the database: 3,338 files done, 0 failed, 34
skipped as longer than five minutes. 13.4 minutes, 0.24 s per file. 31,514
vectors.

Text encoder: 17 to 19 ms per query on the CPU. `fb visual` then takes about
750 ms to read all 31,514 vectors from SQLite and score them in a debug build.
The window will load them once (issue 21).

### Topic queries

20 queries, plain cosine, each file by its best frame, top 20. The full lists
were shown to the user and are not kept here, because they name files from the
private collection.

- Concrete things work: "a cat" gives 20 cat clips. "a dog", "a bird", "a car",
  "snow" and "a video game" are mostly right.
- A query for a kind of shot ("a man talking to the camera", "a person
  laughing", "a meme with a caption") scores many files at 0.35 to 0.39, all
  close together. The list is plausible, but no score separates it from the
  rest.
- "food" has one clear hit at 0.298 and the rest at 0.22 to 0.26.
- Scores run from about 0.22 to 0.39. A good match for a concrete thing is 0.30
  to 0.33. So the cutoff in 21 cannot be one fixed floor. `top - delta` decides
  more than `floor` does.

The user read the topic results on 2026-09-24.
