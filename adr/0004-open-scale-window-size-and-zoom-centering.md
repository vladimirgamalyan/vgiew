# 0004. Open scale, window size, and zoom centering

Status: Accepted

## Context

Two related view-behavior questions were open, and the code got both wrong in
ways users noticed:

1. **On-open scale and window size.** The window always opened at a fixed
   `1280×800`, independent of the image, and the initial scale was a plain
   `fit_scale = min(ww/iw, wh/ih)` with no upper bound — so small images were
   enlarged past 100% and looked blurry.

2. **Zoom and centering.** Wheel zoom was correctly anchored to the cursor, but
   nothing bounded the result: panning could drag the image fully off-screen,
   and after zooming in off-center then zooming back out the image stayed wherever
   the cursor-anchored math left it instead of re-centering. Zoom-out was floored
   at `0.01`, allowing the image to shrink to a dot.

A survey of popular viewers (IrfanView, XnView, ImageGlass, nomacs, qView,
Gwenview, FastStone, macOS Preview, Chrome/Firefox) plus the canvas pan/zoom
literature produced a strong consensus:

- The sane default initial scale is **shrink-to-fit** (downscale large images,
  show small ones at 100%, never enlarge). This is the default in ImageGlass,
  Preview, Chrome, and Firefox, and a named option in the rest.
- The window is either fixed or **sized to the image, capped at the screen work
  area**. Sizing to the image (Preview / IrfanView style) gives the more premium
  feel for a per-file viewer.
- The correct fix for off-center zoom-out is **not** a special "re-center" path.
  It is: cursor-anchored zoom, then a **per-axis clamp that collapses to
  centering** on any axis where the image is smaller than the window, applied
  after *every* zoom and pan. Gwenview (`adjustImageOffset` + `setScrollPos`) and
  XnView's hybrid anchor are the same idea; centering falls out of the clamp.

Alternatives considered and rejected:

- **Fixed default window + always fit into it** (Windows Photos / qView style):
  simpler (no header read), but the window never adapts to the image. Rejected
  for the on-open experience.
- **Full fit that also enlarges small images** (legacy Windows Photo Viewer):
  the most-complained-about default (blurry small images). Rejected.
- **A separate "re-center on zoom out" branch**: redundant and prone to visible
  snaps once the clamp already centers. Rejected in favor of one clamp function.
- **Allowing zoom-out below fit**: leaves a tiny floating image with no purpose.
  Rejected; minimum zoom is floored at the shrink-to-fit scale.

## Decision

We will:

1. **Open scale = shrink-to-fit.** `view_fit = min(fit_scale, 1.0)` is the scale
   used on open, on decode, on window resize while in fit mode, and for the `0`
   (reset) key. Large images are downscaled to fit; small images show at 100%.

2. **Size the window to the image.** Before creating the window we read only the
   header dimensions (`read_dimensions`, no full decode — this keeps the "instant
   window, background decode" design) and size the client area to the shrink-to-fit
   image size, capped at ~90% of the desktop work area (`SPI_GETWORKAREA`, primary
   monitor) and floored at `480×360` so tiny images don't get a pinhole window.
   When there is no file or the header can't be read, fall back to `1280×800`.
   While browsing with ←/→ the window is *not* resized; each image is fit into it.

3. **Zoom to cursor, then clamp-and-center.** After every wheel zoom and every
   drag we run one `clamp_center` function: per axis independently, if the image
   is smaller than the window that axis is locked to center, otherwise the pan is
   clamped so neither edge pulls inside the window. Minimum zoom is floored at
   `view_fit` (never smaller than fit); maximum stays `64×`.

## Consequences

- The reported bug is fixed: zooming out re-centers automatically, and the image
  can no longer be dragged off-screen — both as a side effect of the single
  `clamp_center`, with no separate re-centering code and no visible snap.
- Small images open crisp at 100% in a snug window; large images open downscaled
  in a window that fits the screen. This matches the mainstream "good" default.
- We take on a cheap synchronous header read before window creation and one more
  Windows-only FFI call (`user32!SystemParametersInfoW` for the work area, with a
  non-Windows stub). The full decode stays on the background thread, so the window
  still appears immediately.
- Window **placement** is left to the OS (winit default), sized against the
  *primary* monitor's work area. Opening on the monitor under the cursor and using
  that monitor's work area is a possible future refinement, deliberately deferred
  to keep this change small.
- `fit_scale` (geometric, un-clamped) is retained for the headless `--dump`
  pipeline check, whose contract is unchanged.
