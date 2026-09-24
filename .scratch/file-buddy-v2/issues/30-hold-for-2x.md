# 30 - Hold for 2x speed

Status: ready-for-agent

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
