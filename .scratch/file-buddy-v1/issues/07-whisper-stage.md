# 07 - Whisper stage and backfill

Status: ready-for-agent

Depends on 03. It improves an application that already works.

## Work

- Add `whisper` as a stage in the job queue. Run it with the FFmpeg whisper
  filter, 1 or 2 files at a time. The vendored FFmpeg build has
  `--enable-whisper`.
- Model: `ggml-large-v3-turbo.bin`, multilingual. Content is 98% English, but
  the larger model resists hallucination on loud and musical audio, which is
  what this collection contains.
- The model file lives in `models/` next to the executable. It is gitignored.
  The user supplies it. The application gives a clear error if it is absent. It
  does not download it.
- Skip files longer than 5 minutes and files with no audio track.
- Filter the output. Drop segments with a high `no_speech_prob` or a low average
  logprob. Drop known hallucination phrases such as "Thanks for watching".
  Store a confidence value for each segment.
- Record "no speech found" so the file is never processed again.
- Write segments to `segments` and the joined text to the `speech` FTS column.
- Show progress in the interface. The first backfill is hours of work and must
  survive a restart.

## Measure

If the CPU path is too slow, or the text is too noisy, the alternative is a
whisper.cpp build with CUDA and `whisper-rs`. The machine has an RTX 4070 SUPER.
Do not start there. Measure first.
