# 0008. Keep zoom while browsing, and allow zoom below fit

Status: Accepted (supersedes parts of [0004](0004-open-scale-window-size-and-zoom-centering.md))

## Context

[ADR 0004](0004-open-scale-window-size-and-zoom-centering.md) set the on-open scale
to shrink-to-fit, floored zoom-out at that fit scale (`view_fit` — never smaller than
fit), and specified that browsing ←/→ fits each image into the window. Two
user-driven changes reopened this:

1. **Keep the zoom level when switching images with ←/→**, so inspecting a series at
   a chosen zoom (e.g. 100% pixel-for-pixel) does not require re-zooming every image.
2. **Allow zooming out below fit / below 100%.**

0004 explicitly rejected sub-fit zoom as leaving "a tiny floating image with no
purpose." The new information that justifies reopening it: an empirical study of
XnView MP 1.11.2 (`docs/research/xnview-mp-zoom-behavior.md`) — the same viewer 0004
surveyed — shows it *does* allow zoom-out to an absolute ~1% floor (not fit) and, once
a manual zoom is set, carries that **literal** zoom % verbatim across images. Its
model is a fit flag plus a literal carried scale.

For "keep zoom" the real fork is: preserve the literal scale factor vs a fit-relative
multiplier, and whether to preserve at all while merely at fit. Tying preservation to
the existing `fit_mode` flag and preserving the literal scale matches XnView and needs
no snap once the zoom floor is absolute.

Alternatives considered and rejected:

- **Keep the fit floor and snap a carried below-fit scale up to each image's fit.**
  Simpler and closer to 0004, but reintroduces a visible snap, diverges from XnView,
  and leaves sub-fit unreachable — which was separately requested.
- **Fit-relative carry** (a constant multiple of fit). Consistent framing across
  resolutions, but "same zoom" usually means 100% stays 100%; also unlike XnView.
- **Always carry, even from fit.** Would stop plain fit-browsing from refitting
  differently-sized images. Unwanted.

## Decision

We will adopt XnView MP's model:

1. **Absolute zoom-out floor.** Zoom-out is floored at an absolute `MIN_SCALE` (1%),
   not at `view_fit`. A below-fit / below-100% zoom is reachable; a smaller-than-window
   image is centered by the existing `clamp_center`. `0` still refits, `1` still sets
   100%, maximum zoom stays 64×.

2. **Carry the literal zoom and pan while browsing.** ←/→ keeps the existing
   `fit_mode` flag: while at fit, each image is refit to its own fit scale (unchanged
   from 0004); once the user has manually zoomed (wheel, or `1`), both the literal
   `scale` and the pan (`cx`/`cy`) are carried onto each newly shown image — kept as-is,
   then clamped to the new image's bounds (`clamp_center` centers any axis smaller than
   the window) — regardless of the new image's size and with no clamp-to-fit. So the
   same on-screen region stays put across a series of same-size images. This holds
   whether the next image is prefetched (shown instantly) or decoded on the fly (carried
   when it lands). XnView MP behaves the same way (verified: two 2000×2000 images show
   the identical panned region across navigation).

Shrink-to-fit remains the on-open / folder-open / `0`-key scale, and zoom-to-cursor
then clamp-and-center is unchanged. This **supersedes 0004's zoom-out floor** (part of
decision #3) and its **"browsing fits each image"** behavior (part of decision #1);
the rest of 0004's #1 and #3 stand.

## Consequences

- Browsing a set at a chosen zoom keeps that zoom, so inspecting detail across many
  images no longer re-zooms each time — the requested behavior, matching XnView.
- Zoom-out below fit is possible; carrying a small zoom onto a small image shows it as
  a tiny centered image (the case 0004 avoided) — accepted as the cost of a consistent
  literal-zoom model, with `0` as the one-key escape back to fit.
- No snap-to-fit is needed because the floor is absolute, so the carry is clean in both
  directions. Verified end-to-end on the built app across all six test sizes (fit
  browsing refits; a manual 100% and a sub-fit 64% each carried onto larger and smaller
  images, above and below their own fit) and pan is preserved (a 100% view panned to a
  corner shows the same corner on the next image).
- Bilinear minification below fit can alias on high-detail images — already true at fit
  for large images; sub-fit only extends the range. A better minifier (mip / area
  average) is out of scope.
- Divergences from XnView left as-is and *not* part of this decision: vgiew still wraps
  around at folder ends (XnView does not), the wheel step stays ×1.25 (XnView √2), and
  max zoom stays 64× (XnView 16×).
