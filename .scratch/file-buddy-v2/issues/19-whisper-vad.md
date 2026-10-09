# 19 - Whisper VAD and the full re-transcription

Status: done

Depends on 18. Needs a Silero VAD model in ggml form (`ggml-silero-*.bin`) in
`models/`. The user supplies it. `models/` holds only the whisper model today.

## Work

- Turn on whisper.cpp's Silero VAD through `whisper-rs`. Check first that the
  0.16 API exposes it.
- Measure with and without VAD over `test_videos`, then over the real
  collection. For each file where the kept segments differ, list what changed.
- Adopt VAD only if it drops no real speech that the rules keep. Lyrics count as
  speech. The rule filter stays either way.
- Record the result here and let the user decide.
- After the decision, transcribe every file again once. Skip any file with an
  edited transcript. Issue 25 adds edits, so until it is done there are none to
  skip.

## Comments

### VAD measurement on `test_videos`, 2026-09-24

The 0.16 API has the VAD flag (`FullParams::enable_vad`), but whisper.cpp
applies it only in `whisper_full`, on the context's own state. `whisper-rs`
creates a context without a state and calls `whisper_full_with_state`, so the
flag has no effect. The worker therefore runs VAD itself with
`WhisperVadContext`. It joins the speech stretches the same way whisper.cpp
does (0.1 s overlap, 0.1 s of silence between two stretches) and maps the times
back to the file. That code was removed after the decision.

Default VAD parameters (threshold 0.5, minimum speech 250 ms, minimum silence
100 ms, 30 ms padding). The rule filter runs on both sides. The table compares
the words of each of the 22 clips.

| | Off | Silero v5.1.2 | Silero v6.2.0 |
| --- | --- | --- | --- |
| Whisper stage | 9.8 s | 12.5 s | 12.3 s |
| Clips with the same words as off | | 12 | 12 |
| Words lost | | 103 | 113 |
| Words gained | | 21 | 23 |

Real speech that VAD drops (both versions):

- `my honest reaction`: the whole song, 51 words to 0.
- `sopranos tony`: the sung part, "Ashamed you seemed an honest man, and all
  the fears you hold up there will turn to whisper in your ear".
- `walter the general`: the background voices, "Fuck you! Hate your
  eyebrows!". v6 also drops "You can't keep getting away with it!"
- `i remember the 35 sweet goodbyes`: v5 keeps 10 of 15 words. v6 drops the
  whole clip and gives "cool" instead.
- `nl umamusume`: "Nyeeeah!".

What VAD adds: v6 gives new text in two clips that had none, "ha ha ha ha ha"
(`blud thinks`) and "all three all three all three" (`prod moment`). Both
versions also change words in the speech they keep ("Canadian Geese Diane"
became "keys dying" in v6, "Marco" became "Marcoux").

Result: VAD drops real speech that the rules keep, lyrics most of all. By the
rule in this issue it is not adopted. The real collection was not measured,
because it cannot change this result.

### Decision, 2026-09-24

VAD is rejected. The user decided it, and the code for it is removed. The
worker transcribes the whole file, as in issue 18.

Left to do: transcribe every file again once. The user starts it later. At the
time of the decision, 2,951 files need it: 820 done with the FFmpeg filter and
2,131 pending.


2026-10-09: Closed. The full re-transcription is done.
