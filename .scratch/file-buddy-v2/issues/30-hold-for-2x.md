# 30 - Hold for 2x speed

Status: done

Depends on 29, for click to pause. Read the Preview controls section of the v2
spec first. Only the side pane.

## Work

- First check what `gpui-video` does with the sound at 2x. `set_speed` moves the
  clock. Find out whether the audio follows it cleanly. Record it here.
- Space and a mouse press on the clip each do two things:
  - A press shorter than about 250 ms is a tap. It toggles pause on release.
  - A longer press plays at 2x while held. On release, the clip goes back to its
    state from before: paused stays paused, and playing goes on at 1x.
- The speed is always 2x. Keep the sound if it follows the speed cleanly.
  Otherwise mute it during the hold, and restore it on release.
- GPUI actions fire on key down only, so the release needs a key-up listener.
  Key repeat must not start the hold again or toggle pause.
- `ctrl-space` (autoplay) does not change.
- Unit tests: tap against hold, and the state after a hold, both from paused and
  from playing.

## Comments

### Sound at 2x (2026-09-25)

The sound does not follow the speed, and before this change a clip with sound
did not speed up at all. With an audio track, the master clock is the sample
count of the audio device (`master_clock` in `gpui-video/src/video.rs`), and
`speed` scales only the wall clock that a silent clip uses. The device plays at
its own rate, so the video followed the sound at 1x.

The first version muted the sound during the hold. The second keeps it:

- `gpui-video/src/stretch.rs` stretches the sound to the speed at the same
  pitch, with WSOLA. Windows are 20 ms with a 10 ms hop. Each window goes
  where it best continues the last one, within 5 ms of its nominal place.
  Browsers use the same method for `playbackRate`.
- It runs in the audio callback. The ring keeps 1x sound, and at 2x the
  callback takes twice as many samples for each block it plays. The clock
  counts the 1x samples used, so it stays right at any speed, and the video
  follows it as before.
- A speed change acts on the next block, with no seek. Back at 1x, the sound
  the stretcher took from the ring and did not play is played first. A clear
  of the ring (a seek or a loop) empties the stretcher.
- Unit tests: 2x halves the length and keeps the pitch of a sine, no jump
  between samples, and the unplayed sound comes back.

### Done

- `Player` holds the press: its source (key or mouse) and whether it became a
  hold. A second press while one is down is ignored, which covers key repeat.
- Space fires `TogglePaused` on key down. A key-up listener on the root ends
  the press when the key is one of the keys bound to `toggle_paused`, so
  custom keys work.
- The clip takes a left press and a release, over it or outside it.
- A new clip in the pane drops the press. The window losing focus ends a hold
  with no toggle, because no release comes.
- Unit tests in `ui::player`: tap, hold from paused and from playing, key
  repeat, and a release from the other input.
- `scripts/drive.ps1` has `kdown` and `kup` for held keys.

Checked in the window with a 9.6 s clip with sound: taps play and pause. A
2 s space hold from paused, with key repeats, moved the clip from 0:01 to 0:05,
and the clip stayed paused afterwards. A 1.5 s mouse hold from playing moved
it from 0:01 to 0:04 and went on at 1x. The same checks pass with the stretched sound:
a 2 s hold from paused moved it from 0:00 to 0:03 and stayed paused, and a
1.5 s mouse hold from playing moved it from 0:05 to 0:09.
