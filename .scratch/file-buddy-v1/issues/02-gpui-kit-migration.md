# 02 - Migrate to gpui-kit and prove the video player

Status: ready-for-agent

The video component is the part most likely to fight back, so it comes first.
The migration to `gpui-kit` touches the same file, so both happen together.

## Work

1. Replace `gpui = "0.2.2"` with the `gpui-kit` 0.6.1 umbrella. Pin exactly.
   The library target of `gpui-pre` is named `gpui`, so `use gpui::...` paths do
   not change.
2. Vendor `gpui-video` as project code: remove its `.git`, commit the source,
   and record the upstream revision (`1195fe7`, fork
   `freesurfacemodules/gpui-video`) in a README inside the crate.
3. Port `gpui-video/src/element.rs`. The known break is `Window::paint_image`,
   which gained an `image_bounds` parameter. The crate passes 5 arguments and
   needs 6. On Windows this is the only paint path, because `surface()` is
   macOS only. `src/video.rs` has no GPUI code and should not need changes.
4. Call `gpui_component::init(cx)` at start. Wrap the window in `Root`.
   Register the bundled Lucide assets as the `AssetSource`.
5. Prove the player in a small test application: seek, loop, mute, start and
   stop on hover, and several instances in one window with distinct element ids.

## Done when

Hover over a tile starts a looping muted clip, moving away stops it, and nothing
stalls after 2 seconds. The fork's own history records playback bugs of this
kind, so test it, do not assume it.

## Risk

The upstream API drifted for 11 months and has no changelog. Other breaks may
appear at compile time. They are expected and in scope.
