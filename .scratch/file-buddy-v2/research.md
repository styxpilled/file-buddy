# Research notes for v2

Facts found while writing the v2 spec, kept so a later session does not find
them again. Found on 2026-09-23. Claims marked (unverified) came from one source
or from memory and were not checked.

## The vendored FFmpeg

`ffmpeg 8.1.2-full_build` from gyan.dev has no DNN backend: no `dnn_classify`,
no `dnn_processing`, no libtorch, OpenVINO or TensorFlow. CLIP cannot run inside
FFmpeg.

It does have `chromaprint`, `signature`, `ebur128`, `silencedetect`,
`freezedetect`, `scdet`, `thumbnail_cuda`, `libvmaf`, CUDA and Vulkan.

The `whisper` filter options: `model`, `language`, `queue`, `use_gpu` (default
true), `gpu_device`, `destination`, `format`, `max_len`, `vad_model`,
`vad_threshold`, `vad_min_speech_duration`, `vad_min_silence_duration`.

## FFmpeg timings

23 clips from `test_videos/{text,no_text}`, 532.4 s of media. Clips run 3.8 s to
142 s, median about 14 s. 22 are H.264, 1 is VP9, from 270p to 1080x2340, all
with AAC audio. No stills or GIFs. The commands are the ones in
`src/ffmpeg/{probe,thumbnail,frames,whisper}.rs`. The median of 3 runs, except
whisper, which ran once per clip in two full passes.

| Command | Per clip, median | Total |
| --- | --- | --- |
| ffprobe | 0.023 s | 0.55 s |
| thumbnail, `thumbnail=n=100` | 0.09 to 0.13 s | 2.4 to 3.3 s |
| 10 frames to PNG, `fps` filter | 0.117 s | 3.9 s |
| whisper filter | 2.5 to 2.8 s | 61 to 67 s |
| `ffmpeg -version` | 0.017 s | |
| whisper on 1 s of silence, GPU (load only) | 2.26 s | |
| whisper on 1 s of silence, `use_gpu=0` | 1.27 s | |
| whisper on a 3.8 s clip, `use_gpu=0` | 7.8 s, against 2.3 to 2.5 s on GPU | |
| frames with `-hwaccel cuda` | 0.43 s | 13.1 s |
| frames with `-hwaccel d3d11va` | 0.48 s | 15.1 s |
| frames, 10 `-ss` inputs in one process | 0.28 s | 7.1 s |
| frames, 10 processes with `-ss` | 0.75 s | 17.9 s |
| frames to raw `rgb24` 224x224 on a pipe | 0.095 s | 3.3 s |
| `fps` filter to `null`, decode only | 0.090 s | 3.2 s |
| `thumbnail_cuda` and `scale_cuda` | 0.31 s | 7.2 s |
| audio to 16 kHz `f32le` on a pipe | 0.033 s | 0.86 s |

What this says:

- Whisper is about 90% of FFmpeg time, and about 85% of whisper time is model
  load and teardown.
- The FFmpeg command line builds the filter graph twice, with `-af` and with
  `-filter_complex`, so the model loads twice per call. One run: first load
  0.86 s, teardown 0.24 s, second load 0.84 s, exit 0.22 s.
- The filter runs on the GPU through Vulkan, not CUDA. `-v debug` shows
  `using Vulkan0 backend`, the RTX 4070 SUPER, with the fp16 model in its
  memory.
- Whisper time barely follows clip length: 2.3 s for 3.8 s of audio, 4.0 s for
  142 s.
- Hardware decode is 3 to 4 times slower on short clips, because of device set
  up and the copy back to memory.
- `-ss` seeking in one process wins only on the 142 s clip (0.34 s against
  0.47 s).
- Process start is about 17 ms, which does not matter.

The scripts and raw JSON were in the session scratchpad and are not kept.

## CLIP

### Runtimes

| Option | Native library | GPU on Windows | Notes |
| --- | --- | --- | --- |
| `ort` 2.0.0-rc.13 (ORT 1.28) | Build-time download from `cdn.pyke.io` by default, or `load-dynamic` with a DLL we supply | Windows builds include the DirectML EP. CUDA needs CUDA 13.2+ and cuDNN 9.23+ | Still a release candidate |
| `open_clip_inference` 0.4.2 | Wraps `ort` | Forwards `directml` and `cuda` | One maintainer. Defaults pull tokio and an HF download. `default-features = false` and `from_local_dir` avoid both |
| `candle` 0.11.0 | Pure Rust | CUDA needs nvcc at build, with many open Windows build failures (#2241, #3249). No DirectML | Has CLIP, SigLIP and MobileCLIP examples |
| `fastembed` 7.1.0 | `ort` download | `directml` only | Only CLIP ViT-B/32 and nomic-embed-vision for images |

DirectML is in maintenance mode. Microsoft adds no features and points new work
to Windows ML. It still runs on Windows 10 19045 (unverified for this build).

### Models

| Model | Dimensions | ImageNet zero-shot | ONNX size, fp32 |
| --- | --- | --- | --- |
| OpenAI ViT-B/32 | 512 | 63.2% (unverified) | about 600 MB (unverified) |
| MobileCLIP2-S2 | 512 (unverified) | 77.2% | 405 MB: 144 MB image, 254 MB text |
| MobileCLIP2-S3, S4 | 768 (unverified) | 80.7%, 79.4% | larger |
| SigLIP2 SO400M-384 | 1152 | 84.1% | 4.59 GB |

ONNX exports of MobileCLIP2 and SigLIP2 are at `huggingface.co/RuteNL`. CPU
times from the `open_clip_inference` README, hardware not stated, image and text:
S2 75 and 19 ms, S3 116 and 35 ms, SO400M 988 and 136 ms. MobileCLIP2 uses the
Apple AMLR licence, research use, which covers personal use.

### Vector search

Brute-force dot product and top 100 over 50,000 vectors, one thread on the
7950X, measured: 1.9 ms at 512 dimensions, 3.1 ms at 768, 5.4 ms at 1152. At
512 dimensions the vectors take 102 MB as f32, half that as f16.

`sqlite-vec` 0.1.9 is before v1 and warns of breaking changes. Not checked with
rusqlite 0.40 on MSVC. It gives nothing brute force does not at this size.

### Merging with bm25

- CLIP cosine scores are not calibrated. With ViT-B/32, matches score about 0.25
  to 0.35 and unrelated images about 0.15 to 0.22 (unverified). There is always
  a best match.
- Weighted reciprocal rank fusion needs no score normalization:
  `score = sum of w / (k + rank)`, with `k = 60` (Cormack 2009).
- Keep only vector hits with `sim >= max(floor, top - delta)`. Tune both on real
  queries.

### Search by sound

`Xenova/clap-htsat-unfused` has ONNX files: audio 118 MB (34 MB quantized), text
502 MB (127 MB quantized). It needs 48 kHz audio in 10 s windows and a mel front
end written in Rust. It knows general sounds such as a dog barking, and
probably not named humorous sounds (unverified). Not in v2.

## Whisper

### On ONNX Runtime

- `onnx-community/whisper-large-v3-turbo` has the encoder (fp32 2.55 GB, fp16
  1.27 GB, int8 645 MB) and `decoder_model`, `decoder_with_past_model` and
  `decoder_model_merged` (fp32 688 MB, fp16 344 MB).
- `onnx-community/whisper-large-v3-turbo_timestamped` exports cross attentions
  for word times. transformers.js #1357 reported broken word times, fixed by PR
  #1594.
- No maintained Rust crate runs whisper on `ort` with probabilities and word
  times. `transcribe-rs` runs whisper through whisper.cpp. The `sherpa-onnx`
  crate is 11 months old, and its issue #1240 reports whisper medium on DirectML
  crashing the GPU.
- Writing it ourselves is about 1,000 to 1,400 lines: log-mel front end, BPE
  decode, greedy loop with KV cache, timestamp rules, temperature fallback,
  30 s windows, DTW for word times.
- DirectML problems: the ORT fused whisper ops fail on it (Olive #1213, #1221),
  a shape that grows each step is slow (LLMs about 2x slower, ORT #15394), and
  there is the crash above. No published speed for whisper on DirectML.

Rejected. The one gain was a single runtime for both models.

### whisper-rs

- 0.16.0, released 2026-03-12. Features `cuda`, `vulkan`, `hipblas`, `metal`.
- Build needs MSVC, CMake and LLVM with `LIBCLANG_PATH`, plus the CUDA toolkit
  or the Vulkan SDK. A CUDA build must ship the cudart and cublas DLLs. A Vulkan
  build does not.
- Gives `WhisperSegment::no_speech_probability()`,
  `WhisperToken::token_probability()` and `token_data()` with `p`, `plog`,
  `t0`, `t1` and `t_dtw`.
- `DtwModelPreset::LargeV3Turbo` gives word times. DTW is off when flash
  attention is on.
- whisper.cpp has Silero VAD built in. `FullParams::enable_vad` has no effect
  in `whisper-rs` 0.16: whisper.cpp applies VAD only in `whisper_full`, and
  `whisper-rs` calls `whisper_full_with_state`. `WhisperVadContext` works on its
  own. Measured and rejected in issue 19.
- `ct2rs` 0.10.1 (CTranslate2) is faster on an RTX 4070 by one secondary source,
  about 12x against 8x real time, but its Windows GPU path is CUDA and cuDNN
  only, and it needs a converted model.

## Sources

- https://github.com/RuurdBijlsma/open-clip-rs
- https://ort.pyke.io/perf/execution-providers
- https://ort.pyke.io/misc/prebuilt-binaries
- https://github.com/pykeio/ort/blob/main/ort-sys/build/download/dist.tsv
- https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html
- https://github.com/Anush008/fastembed-rs
- https://github.com/huggingface/candle/issues/2241
- https://github.com/huggingface/candle/issues/3249
- https://alexgarcia.xyz/sqlite-vec/rust.html
- https://huggingface.co/RuteNL
- https://huggingface.co/Xenova/clap-htsat-unfused
- https://huggingface.co/onnx-community/whisper-large-v3-turbo
- https://huggingface.co/onnx-community/whisper-large-v3-turbo_timestamped
- https://github.com/huggingface/transformers.js/issues/1357
- https://github.com/cjpais/transcribe-rs
- https://github.com/k2-fsa/sherpa-onnx/issues/1240
- https://github.com/microsoft/Olive/issues/1213
- https://github.com/microsoft/Olive/issues/1221
- https://github.com/microsoft/onnxruntime/issues/15394
- https://docs.rs/whisper-rs/latest/whisper_rs/
- https://github.com/tazz4843/whisper-rs/blob/master/BUILDING.md
- https://crates.io/crates/ct2rs
