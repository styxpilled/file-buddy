# 54 - Playback faults in the decoder

Status: needs-triage

The faults are in the vendored `gpui-video` decoder. They show in the side
pane and in the Studio, and they existed before v3.

## Sound and picture drift apart after much scrubbing

A long run of drags on the seek bar leaves the sound and the picture out of
step. The picture then plays very fast until it catches up with the sound, and
after that both are in step again.

A drag now pauses the clip until the release (fixed in the work on 36), so the
clock does not run during the drag. The fault still shows. Look at what a burst
of seeks that are not accurate leaves behind: `clock_base`, `samples_played`
and the audio ring are reset on each seek, but a seek that the decoder has not
read yet can let audio from before it into the ring. The fast playback is the
presenter dropping late frames until the video clock reaches the audio clock.

Check: drag back and forth on the bar of a clip with sound for some seconds,
then release. Picture and sound stay in step from the release, with no fast
playback.

## A seek shows the first frame of the clip first

A clip that opens at a moment, from a match, shows its first frame before the
frame at the moment. The fix in the work on 36 made the side pane wait until
the seek is done (`Video::seeking`) before it shows the clip. The first frame
still shows, but now the clip is paused on it. So `seeking` goes false before
the frame of the target is on screen, or the element paints the first frame
the decoder presents and not the one after the seek.

Look at where `seek_shown` clears the flag, and at which frame the element
paints when a seek is sent before the decoder presents anything.

Check: open a clip from a speech match some seconds in. The pane goes from the
old clip to the frame at the match, and it never shows the first frame.

## Some GIFs never show a frame

`C:\Users\styx\Downloads\broken-gif.gif` plays in no player of the window. The
timeline moves and the clock runs, but the stage stays empty, in the side pane
and in the Studio. The tile shows the thumbnail, so FFmpeg decodes it.

What is known:

- ffprobe: `gif`, 398x267, 25 fps, 95 frames, 3.8 s. The frames decode as
  `bgra`. Packet and frame timestamps start at 0 and step by 4 in a 1/100 time
  base, which looks normal.
- The log repeats `Buffer critically low (2 frames)! Rebuffering...` about
  every 3 seconds while it plays.
- `euler.gif` plays, so this is not every GIF.

Start with the frame path: the scaler from `bgra` to NV12, and whether the
frames reach the buffer with the timestamps the presenter expects.

Check: broken-gif.gif plays in the side pane, on a hovered tile and in the
Studio.

## Comments
