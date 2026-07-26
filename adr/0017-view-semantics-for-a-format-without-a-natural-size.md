# 0017. Open scale, `[W×H]` and `Ctrl+C` for a format with no natural pixel size

Status: Accepted

## Context

ADR 0016 adds SVG. ADR 0004 fixed the on-open scale as **shrink-to-fit**: downscale
anything larger than the window, but never enlarge past 100% (`view_fit`'s `.min(1.0)`).
That rule was chosen after surveying a dozen viewers, and its purpose is specific — an
enlarged raster is blurry, so enlarging past 1:1 is a disservice.

A vector has no 1:1. Worse, many SVGs declare no size at all: `width="100%"` with only a
`viewBox` is the normal shape of a web asset. So three questions have no answer inherited
from ADR 0004: what scale to open at, what `[W×H]` and `1` (100%) mean, and what resolution
`Ctrl+C` should put on the clipboard.

`usvg` resolves a size for every document, but does **not** report where it came from.
Measured on the full attribute matrix (`test-images/svgsize`, via the `svg_start_resvg`
spike):

| Root attributes | `tree.size()` |
|---|---|
| `width="400" height="200"` (with or without `viewBox`) | 400×200 |
| `width="100%" height="100%" viewBox="0 0 300 150"` | 300×150 |
| `width="50%" height="50%" viewBox="0 0 300 150"` | 150×75 |
| `width="400" height="100%" viewBox="0 0 300 150"` | 400×150 |
| `viewBox="0 0 300 150"`, no width/height | 300×150 |
| `width="210mm" height="297mm"` | 794×1123 (96 dpi) |
| `width="72pt" height="144pt"` | 96×192 |
| nothing, no `viewBox` | bounding box of the content, measured from the origin |
| nothing, no `viewBox`, no content | 100×100 (`default_size`) |

Two facts follow. First, a declared size and a `viewBox` fallback are indistinguishable
from `size()` alone, so distinguishing them requires reading the root `<svg>` element's
`width`/`height` ourselves. Second, for a document with neither size nor `viewBox`, usvg
reports the content's bounding-box extent rather than the browsers' 300×150 default — which
for a viewer is the better answer, since it shows all the content instead of clipping it.

Alternatives considered for the open scale:

- **Apply `min(1.0)` to vectors too**, unchanged from ADR 0004. Perfectly consistent, and
  it keeps the browse-time zoom carry (ADR 0008) coherent across formats — but a 24×24 icon
  in a 1280×800 window is a stamp in the middle of an empty screen, and the user has to
  zoom by hand every time. Rejected as the sole rule.
- **Always fit a vector to the window**, dropping `min(1.0)` because there is no blur to
  protect against. Attractive and principled, but it overrides an explicit instruction: a
  file that says `width="24"` has stated the size it wants to be seen at.
- **Rasterize to a fixed large size** and treat the result as an ordinary image. Predictable
  but it discards the author's intent for every file.

## Decision

We will resolve view semantics for SVG by **honouring a declared size and fitting only when
none was declared** — the rule Chrome and Firefox apply when an `.svg` is opened as a
document:

1. **Open scale.** If the root `<svg>` declares *both* `width` and `height` as non-percentage
   lengths, the document has a natural size: `min(1.0)` applies exactly as for a raster.
   Otherwise there is no natural size, and the document is fitted to the window, enlarging
   past 100% if needed. A mixed case (one axis declared, one a percentage) counts as *not*
   declared — one rule beats a per-axis rule nobody can predict.
2. **Detection** is a bounded scan of the root `<svg>` tag for `width`/`height`, not a second
   parse: re-parsing a 10 MB file to read two attributes would cost 120 ms.
3. **`[W×H]` and `100%`** are the size usvg resolves, whichever branch produced it. So `1`
   means one SVG user unit per physical pixel, and physical units resolve at 96 dpi
   (`210mm` → 794 px) — the same convention browsers use.
4. **`Ctrl+C` rasterizes at the document size**, not at the current zoom. It is the number
   the title already shows and the number `1` already means, and it keeps paste quality
   independent of how the wheel happened to be turned. The alternative — copy what is on
   screen — would make an identical file paste differently depending on zoom, and would
   need an upper bound to avoid gigabyte bitmaps at 64×.

## Consequences

- An icon that declares `width="24"` opens as a 24×24 stamp. This is deliberate: the file
  asked for it, and `0` fits it to the window in one keystroke. It is also the one part of
  this decision most likely to be questioned later, so the reasoning is recorded here
  rather than left to be rediscovered.
- A web asset with `width="100%"` fills the window, which is what a browser does and what
  the file's author expected.
- One format now has two open-scale behaviours. That is a real cost in explainability, and
  it is the price of matching author intent instead of picking one rule for both cases.
- The browse-time zoom carry of ADR 0008 needs care for vectors: carrying a photo's 0.25×
  onto a 24×24 icon would render it 6 px wide. Files that open at fit already re-fit, so the
  no-natural-size branch is covered; the declared-size branch inherits the raster rule and
  the same surprise a small PNG already produces.
- `Ctrl+C` on a small icon puts a small bitmap on the clipboard. Predictable, but a user
  who wanted a large paste has to export elsewhere — we are a viewer, not a converter.
- `[W×H]` for a file with neither size nor `viewBox` reports the content's bounding box, so
  the number can look arbitrary next to the same file opened in a browser. Accepted: it
  shows all the content, and clipping it to 300×150 to match a browser would hide artwork.
