# 33 - A smaller CLIP text encoder

Status: needs-triage

Depends on 32, which found this. Read the CLIP section of the v2 spec first.

The text encoder is the largest fixed cost of the window. It runs in the window
process on the CPU, and holds about 250 MB from start to close, in the heap and
in mapped files. `models/mobileclip2-s2/text.onnx.data` is 254 MB of fp32
weights: 63.4 M parameters, of which the token embedding is 49,408 × 512, about
101 MB. A smaller copy of the same model could cut this by half or more.

The image encoder and the stored vectors do not change. Only the query side
changes, so no file is embedded again.

## Found without testing

- `RuteNL/MobileCLIP2-S2-OpenCLIP-ONNX`, where our files come from, has only
  fp32.
- `kingfang008/mobileclip2-s2-int8` on Hugging Face is an int8 copy of the same
  base. It is not reviewed, and its text weights are 140 MB, so the token
  embedding is probably still fp32. Other MobileCLIP2 exports there are fp32,
  or of other sizes.
- Making our own copies from the fp32 files is better: the tools are
  Microsoft's, and the result can be made again. In a venv outside the
  repository:
  - int8: `onnxruntime.quantization.quantize_dynamic`, weights `QInt8`, with
    `Gather` in `op_types_to_quantize` so the token embedding is quantized too.
    The x86 CPU runs this with integer kernels, so the memory saving is real.
    Expected size: about 65 MB.
  - fp16: `onnxconverter_common.float16.convert_float_to_float16`, with
    `keep_io_types=True`. Not verified: the CPU execution provider has few fp16
    kernels on x86, so ONNX Runtime may insert casts and fold the weights back
    to fp32 at load. The file would then be half the size and the memory would
    not change. Measure it, do not assume it.
  - fp8: ONNX has fp8 types, but the CPU execution provider has no fp8 kernels
    for `MatMul`. Not available for this use.
- The quantization tool of the Python `onnxruntime` package may be newer than
  the vendored `onnxruntime.dll`. Check that the vendored DLL loads the result.

## Work

1. Make the int8 and fp16 copies beside the fp32 files, with new names, for
   example `text-int8.onnx` and `text-fp16.onnx`.
2. For each copy, measure against fp32:
   - quality: for about 30 queries, the overlap of the top 50 visual results,
     and the cosine similarity of the query vectors. `fb visual` gives the
     results without the window.
   - memory: private bytes and working set of the idle window, with
     `scripts/memory.ps1`.
   - time to embed one query, which the debounce of 150 ms must cover.
3. Keep the smallest copy whose results barely change. "Barely" is the user's
   call, from the numbers.
4. If one wins, `clip::text` loads it, and `paths::clip_model` checks for it.
   The user supplies the model files, so the README of the model directory
   says how the copy was made.

## Done when

The numbers for each copy are written here, and the window either loads the
chosen copy or the reason to keep fp32 is written here.

## Comments
