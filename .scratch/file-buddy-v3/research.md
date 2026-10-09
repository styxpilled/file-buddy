# file-buddy v3 research

Measured on 2026-10-09 with the vendored FFmpeg, `8.1.2-full_build-www.gyan.dev`.

## Animated WebP

One file in the index is an animated WebP:
`C:\Users\styx\Downloads\HTZnD_HXkAARRI8.webp`, 139 KB. Its chunks are `VP8X`,
`ANIM`, then 38 `ANMF` frames. The probe recorded it as a 0x0 still. The
thumbnail, OCR and CLIP jobs all failed with "Nothing was written into output
file, because at least one of its streams received no packets". The 7 static
WebP files index without errors.

`ffmpeg -i` on the file prints `skipping unsupported chunk: ANIM` and `ANMF`,
then `image data not found`. The build lists `webp` as the only WebP decoder.
`libwebp` and `libwebp_anim` are encoders only.

A 2 s animated WebP that `libwebp_anim` wrote fails to decode the same way.
FFmpeg cannot read its own output.

`image-webp` 0.2.4 is already in `Cargo.lock`, through the dependency tree.
`image` 0.25.10 is a direct dependency, with only the `jpeg` feature.

## Encoders and filters

The GPU is an NVIDIA GeForce RTX 4070 SUPER, driver 617.14.

Each encoder wrote a 2 s, 640x360 `testsrc2` with a `drawtext` box shown from
0.5 s to 1.5 s:

| Encoder | Bytes |
| --- | --- |
| `h264_nvenc` | 463,246 |
| `av1_nvenc` | 439,863 |
| `libvpx-vp9` | 177,985 |
| `libwebp_anim` | 435,936 |
| `gif` | 602,525 |

All ran at default settings, so the sizes do not compare quality.

The build also has `aac`, `libopus`, `png` and `mjpeg` encoders, and the
`drawtext`, `palettegen`, `paletteuse`, `crop`, `setpts` and `atempo` filters.

`drawtext` takes a Windows font path only in quotes with the colon escaped:
`fontfile='C\:/Windows/Fonts/arialbd.ttf'`.

`arialbd.ttf`, `segoeuib.ttf` and `impact.ttf` are in `C:/Windows/Fonts`.

## File drop

`gpui-pre-windows` 0.3.1 registers an `IDropTarget` on each window
(`RegisterDragDrop` in `src/window.rs`). A drop from Explorer reaches the
elements as `ExternalPaths`. The application does not handle it yet: no code
in `src/` takes a drop, and `fb` takes no file arguments.
