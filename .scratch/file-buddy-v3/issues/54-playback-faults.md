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

### 2026-10-09

**Some GIFs never show a frame: fixed.** broken-gif.gif is 267 px high. An
odd height has one more NV12 chroma row than half the height, and
`pack_nv12` dropped it. The frame came out 199 bytes short, and the element
then painted a zeroed, transparent frame. `nv12_chroma` now gives the chroma
size to the packer and to the element. Checked in the side pane, on a hovered
tile and in the Studio.

**Sound and picture drift apart: one cause fixed.** The audio callback played
and counted the sound while the decoder buffered after a seek, a loop or a
rebuffer. So the clock was ahead of the first frame, and the picture raced to
it. The sound now waits while `buffering` holds. The callback also counts the
samples under the ring lock, and a seek zeroes the count under the same lock,
so no sound from before a seek is counted. In a release build, the clock led
the first frame after a seek by 26 to 113 ms before. Now it is within one
frame and one audio block. Test:
`a_seek_while_playing_starts_sound_and_picture_together`. A drag in the sim
build did not show the fault before or after, so this needs a check by ear.

**A seek shows the first frame first: not reproduced.** The decoder shows the
target first when the seek comes at once after the open, and also on a
preloaded clip (22 clips). In the sim build, with autoplay off and on and with
a query typed key by key, the pane painted the frame at the match first every
time. Need the clip and query that show it.

### 2026-10-09, later

The user confirms the GIF and drift fixes.

**A drag's hold wins over pause and play.** While a drag on the bar or on a
Studio handle holds the clip, space, a click on the clip, autoplay and a 2x
hold change only what the user chose (`Player::held`, `Studio::held`). The
release seeks, then applies that choice.

**Scrubbing is frame-accurate.** Keyframe seeks showed only the keyframes.
Most clips here have 2 to 7, so a short clip showed only frame 0. A drag now
sends accurate seeks, one at a time, and keeps only the latest place while one
decodes (`SeekBar::pending`). An accurate seek costs 16 to 220 ms over
`test_videos` (worst 329 ms, 1080x1920 with 3 s between keyframes). So a slow
clip updates less often but never falls behind the pointer, and no length limit
is needed.

**The end of a clip came early.** `monkey opening portal.gif` (10 fps, 4.9 s)
showed its last 4 frames at once and ended at 4.48 s. The presenter showed
early frames at once when 3 or fewer were buffered, which happens only at the
end of the stream. And the end did not wait for the last frame's own time.
Frames now carry their duration from the packet (1 / frame rate when it has
none), and `drained` and `ended` wait until the clock passes the end of the
frame on screen. The GIF now ends at 4.9 s. Test:
`the_last_frames_play_for_their_time`.
