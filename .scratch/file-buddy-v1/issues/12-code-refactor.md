# 12 - Refactoring and cleaning up

Status: done

The code written in issues 02-04 is not very idiomatic. Analyze the existing code, particularly in gpui-video and refactor it.

## Work

- Refactor existing code
  - Treat gpui-video with extra care. The source code is filled with new variable declarations and bloated Arc clones. Consider using the [share-rc](https://crates.io/crates/share-rc) crate if the excessive use of cloning Arcs is neemed optimal.
  - The construction of the Arcs is also messy.
- Remove legacy MacOS-only code

## share-rc

Before

```rust
let _some_a = self.some_a.clone();
let _some_b = self.some_b.clone();
let _some_c = self.some_c.clone();
set_ffi_callback(move || {
    do_something(_some_a, _some_b, _some_c)
});
```

After

```rust
set_ffi_callback(share!(|| {
    do_something(
        take!(self.some_a.clone()),
        take!(self.some_b.clone()),
        take!(self.some_c.clone()),
    )
}));
```

## Comments

Done. `share-rc` was not needed: the Arc clones were a symptom, not the problem.

- One `Arc<Shared>` replaces 17 separate Arcs. `Shared` holds the frame, both
  buffers, both clocks and the flags, so the decoder thread and the audio
  callback each take a single clone.
- `decoder_loop` had 24 parameters. It is now `Worker::run`, and the loop body
  is split into `handle_commands`, `restart_loop`, `master_clock`,
  `present_due`, `pace_decoding` and `decode_one`.
- The audio decoder, its resampler, its stream index and its time base are one
  `AudioTrack`. `has_audio` is `Option::is_some` on it.
- `Video` is `Arc<Internal>`, not `Arc<RwLock<Internal>>`. Every field behind
  that lock was an atomic or its own mutex, so the `read()`/`write()` split said
  nothing. `Drop` moved to `Internal`, which removed the `strong_count == 1` and
  `try_write` dance.
- `VideoOptions` fields are plain values, not `Option`s that are unwrapped to a
  default two lines later.
- Named the magic numbers in the presenter: `BUFFER_FLOOR`, `MAX_SKIP`,
  `PRESENT_TOLERANCE`, `SKIP_THRESHOLD`.
- Pulled out `pack_nv12` and `to_duration`; both were open-coded twice.
- Removed the macOS `CVPixelBuffer` paint path, the `core-video` and
  `core-foundation` dependencies, and the unused `Error::Duration` and
  `Error::Lock`.
- Rewrote the crate docs, which described a cross-platform crate with an
  `examples/` directory. The main doctest did not compile.

No behaviour change. `cargo test -p gpui-video` passes, as do the ignored
`the_presented_frame_rate_holds`, `does_not_freeze_over_a_minute` and
`repeated_hover_cycles_never_race`.
