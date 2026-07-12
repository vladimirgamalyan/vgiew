# 0011. Draw a pixel grid at high zoom

Status: Accepted

## Context

When zoomed in far enough that each image pixel spans many screen pixels,
a run of identical-colored pixels renders as one flat block: you cannot tell
where one pixel ends and the next begins, or count them. That matters for the
close pixel inspection that high zoom exists for (checking a gradient, reading
exact pixel boundaries, spotting a stray off-by-one-color pixel).

The render path is CPU/softbuffer with nearest-neighbor sampling at `scale >= 1`
(ADR 0002), so we already touch every screen pixel in `draw` — a grid is a
per-pixel tweak, not a new pass.

Design questions and the forces on each:

- **When to show it.** A grid over pixels only a few screen-px wide would cover
  a large fraction of each pixel and read as noise during ordinary zooming. It
  is only useful once pixels are clearly blocky. So gate it on a zoom threshold
  rather than showing it always.
- **Automatic vs. a toggle.** The request was "show a grid at high zoom." A
  scale-gated automatic grid satisfies that with no new UI, no key to discover,
  and no state to persist. A toggle is speculative until asked for.
- **Line color.** A fixed gray line is the obvious choice but fails on exactly
  the case the grid is *for*: a field of identical mid-gray pixels, where a gray
  line vanishes. The line must contrast with whatever pixel it sits on.
- **Where the line lives.** The grid must move and scale *with the image* (it is
  an image-space overlay, anchored to pixel boundaries), not a fixed screen-space
  lattice — otherwise it would not line up with pixel edges as you pan/zoom.

## Decision

We will draw a 1px pixel grid inside `draw`, automatically, when
`scale >= GRID_MIN_SCALE` (8×, so each pixel is ≥8 screen px). No toggle.

- **Boundary detection is in image space.** A screen pixel is a grid line when
  it maps to a different image pixel than its left neighbor (vertical line) or
  its upper neighbor (horizontal line) — i.e. `floor(sx)`/`floor(sy)` changed.
  This is exact for any (including fractional) scale and pans naturally with the
  image. The column test is row-independent, so it is precomputed once per frame.
- **The line color is content-adaptive** (`grid_tint`): blend the underlying
  composited pixel halfway toward the opposite luminance extreme (toward black
  on a light pixel, toward white on a dark one). The line always contrasts, even
  across a run of one color, and stays a tint of the pixel rather than a jarring
  fixed hue.
- **The grid is confined to the image**, drawn only where the sample is in
  bounds — never over the surrounding background or the fit margins. Left/top
  image edges get a border line as a side effect of the "differs from neighbor"
  rule; the right/bottom edges sit against background and get none. Accepted as
  a negligible, standard asymmetry.

## Consequences

- Same-colored adjacent pixels become individually visible once you are zoomed
  in enough to be inspecting them, with no interaction and nothing to enable.
- Cost is paid only when the grid is active: below 8× the extra work is skipped
  entirely, and even when active it is a floor/compare per pixel plus a cheap
  tint on the thin set of boundary pixels — negligible against the existing
  per-pixel sampling, and it stays inside the same rayon-parallel loop.
- The 8× threshold and 50%-toward-extreme contrast are fixed constants tuned by
  eye. If either proves wrong they are one-line changes; a user-facing toggle or
  configurable spacing can supersede this ADR if a real need appears.
- No new dependency, no new window state, no persistence.
