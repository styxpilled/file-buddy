# 07 - Whisper stage and backfill

Status: ready-for-human

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

## Comments

### 2026-09-17, agent

The stage works with the FFmpeg filter. One decision is open: whether rules are
enough, or whisper-rs is necessary. Run the backfill over the real collection,
read the transcripts, then decide.

#### What the filter gives

- Start, end and text for each segment. No `no_speech_prob`, no logprob, no
  token probabilities. The probability filter and the confidence value in the
  Work list cannot be done with the filter. `segments.confidence` stays NULL.
- It writes quotes into the JSON text without escaping them, so the output is
  read by position, not as JSON.
- Windows of 30 seconds whatever `queue` is above that. Smaller queues cut
  sentences and hallucinate more (a Korean line from silence at `queue=3`).
- A short last window hallucinates: "Thank you." in the last 1.4 s of a 31 s
  file, "Bye." and "you" in the last 0.1 s of a 60 s file. The window edges move
  about 23 ms earlier per window.
- `use_gpu=true` works with the vendored build.

#### Rules

A segment is dropped when, after `[...]`, `(...)`, `*...*` and music notes are
removed, it holds no words, or holds only a known hallucination phrase
("thank you", "thanks for watching", "music" and others). A sentence that holds
one of these phrases is kept. Segments of a last window shorter than 2 s are
dropped. An end past the end of the file is clamped.

#### Measured over `test_videos` (21 clips)

- About 2 s per clip, most of it model load. 142 s of audio took 3.5 s.
- With 2 workers the 21 clips took 41 s, against about 50 s one at a time. The
  second worker gives 1.2x. A few thousand clips is about 1.5 hours.
- The text is clean. All dropped segments were noise: `*sad music*` x7,
  `*laughter*`, `*music*`, `*outro music*`, `*MUZIE*`, "Music.", and the three
  tail hallucinations. No real speech was dropped. Lyrics are kept as speech.
- Misheard words remain ("esoteric music tax"). Rules cannot catch those, and
  probabilities would not make them right.

#### Other behaviour

- A file with no audio track is skipped at the probe. A migration skips the
  whisper jobs that earlier probes queued for such files.
- "No speech found" is a `done` job with no segments. A finished job is never
  queued again.
- Without the model, `index::transcribe` stops before any job runs, so no file
  is marked failed for it. The header says `speech model missing` and the log
  has the full path.
- Transcription runs after probe and thumbnail, so the grid shows results before
  it starts. The header shows `speech N / M`, and new transcripts reach the
  results every 30 s. The queue is durable, so a restart continues the backfill.
  A job that a closed window interrupts loses one of its two attempts.
- Integration tests: a `flite` speech clip is transcribed once and found by
  "brown fox", and a video with no audio track skips whisper. The first needs
  the model in `models/`.

