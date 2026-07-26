# 0016. SVG via resvg, rasterizing the viewport rather than the image

Status: Accepted

## Context

Every format vgiew shows today is a raster the `image` crate decodes into a fixed
`DecodedImage { w, h, px }`, and the whole program is built on that: `draw` samples from
it by `cx/cy/scale`, the cache bounds itself by `w*h*4`, the title reports `[W×H]`, the
pixel grid marks the boundary between image pixels. SVG has no pixels — it has a document
and a coordinate space — so it cannot be added by enabling a feature.

**Renderer.** There is effectively one option. `ID2D1SvgDocument` (Direct2D 1.3) needs a
D2D device, i.e. the ~150 ms GPU-initialisation floor ADR 0002 rejected, and does not
support `<text>` at all. WinRT `Windows.Data.Svg` renders only through XAML
`SvgImageSource` and hands back no raster. The Shell thumbnail API works only if some
third party installed an SVG thumbnailer, and yields one fixed-size bitmap. That leaves
**resvg** (0.47): pure CPU via tiny-skia, no network code whatsoever, no scripting — a
natural fit for the Tier C path.

**How deeply to support it** was the real question, and three levels were considered:

- **Rasterize once at the document size**, then reuse the raster path unchanged (~30 lines
  of glue). Zooming in is then blurry — and zooming in is precisely why anyone opens a
  vector file. This would ship a feature that is worse than opening the file in a browser.
- **Rasterize the whole canvas at the current scale**, re-rendering on zoom. A 1000×1000
  drawing at 64× needs 64000² × 4 B = **16 GB**. Survivable only behind an arbitrary cap,
  beyond which the blur returns. A half-measure.
- **Rasterize only the visible viewport** at the current transform. Memory is `ww*wh*4`
  (~6 MB at 1600×1000) *at any zoom*, and cost tracks visible complexity rather than
  canvas area. `resvg::render(&tree, transform, &mut pixmap)` supports this directly.

Measurements settled it (full numbers in `concept.md`, spikes `svg_start_baseline` /
`svg_start_resvg` / `svg_start_trim`, scripts `measure_svg.ps1` / `measure_svg_cold.ps1`):

- **Linking resvg does not cost startup.** A normal launch moved 41.2 → 42.0 ms
  wall-clock — noise. Faulting in the extra 2.3 MB costs ~3 ms on NVMe, and the imported
  DLL set is byte-identical, so there is no new loader dependency. The one real penalty,
  +55…72 ms, is Defender scanning a newly written executable and is paid **once per
  build/install**: the same fresh copy runs 188 ms, then 49.8 ms, then ~51 ms.
- **A realistic SVG reaches first pixels faster than a photo**: 4.6 ms for an icon,
  10.8 ms for a gradient logo, 31.9 ms for a text-bearing file — against the 49 ms the
  existing raster path already spends decoding a 24 MP JPEG. A 2000-path illustration is
  about par at 59 ms. Only pathological files (60k paths → 610 ms) are slow.
- **Strip tiling gives 8–10×.** `resvg::render` is single-threaded, but a strip is just a
  shorter pixmap with a shifted transform, and rayon is already a dependency: 156 → 21 ms
  at 4× zoom on the illustration, which is what makes interactive zoom possible at all.
- **Cold system-font loading is ~1692 ms** (673 files, 420 MB in `C:\Windows\Fonts`);
  warm, 15–18 ms.

## Decision

We will support SVG (and SVGZ) with **resvg**, keeping the parsed `usvg::Tree` per file and
**rasterizing only the current viewport**:

1. **Dependency**: `resvg = "0.47"` with its **default features**, including
   `raster-images`. Trimming that feature looked attractive — it appears to duplicate the
   gif/webp/jpeg decoders `image` already provides — but three findings killed it. Its
   codec crates (`gif`, `png`, `image-webp`, `zune-jpeg`) are the *same versions* `image`
   0.25 uses and cargo unifies them (`cargo tree -d` reports no duplicates), so in a binary
   that already links `image` the feature costs 190 KB, not the 460 KB a codec-less spike
   suggested — about 7 ms of the one-time scan. `usvg::ImageKind`'s raster variants carry
   *undecoded* bytes and resvg decodes them itself behind that feature, so a custom
   `ImageHrefResolver` cannot substitute `image`: the resolver only classifies, and there is
   no variant that accepts decoded pixels. And with the feature off, resvg logs a warning
   and draws nothing, so an SVG with an embedded bitmap renders as a silent hole — exactly
   the failure mode this ADR calls out below. SVGZ needs no feature in 0.47.0: `flate2` is
   an unconditional dependency and `Tree::from_data` checks the `1f 8b` magic itself.
2. **Render path**: render into a viewport-sized pixmap at the current view transform,
   split into horizontal strips across rayon. Tiling is applied unconditionally — its
   overhead is ~0.3 ms, irrelevant against a frame budget, and a threshold would be one
   more tuned constant and one more behaviour to explain.
3. **Stale frame as placeholder**: keep the last raster together with the transform it was
   produced at. When the view still matches, blit it; when it does not, resample it through
   the *existing* sampler while a fresh raster is computed on a background thread. Cheap
   files complete within a frame; heavy ones degrade to blurry-then-crisp, like a browser.
4. **Cancellation**: rasterization requests carry a generation token, and results for a
   superseded view are dropped. Without this a wheel gesture on a heavy file queues
   hundreds of milliseconds of dead work.
5. **No special mode for heavy files.** The placeholder path already degrades gracefully;
   a second "rasterize once, then treat as raster" mode would add a tuned threshold and a
   second behaviour for one format.
6. **Font loading is gated** on a byte scan of the source for `<text` / `<tspan`. Most
   SVGs — icons, logos, chart exports — carry no text and must not pay for the font
   database. When they do, it runs on the background decode thread with the window already
   on screen.
7. **Detection stays by content**, as for every other format: skip BOM and leading
   whitespace, accept an XML declaration, comment or DOCTYPE, then require `<svg`; gzip
   magic `1f 8b` marks a candidate SVGZ, which usvg then decompresses and validates itself.
   `svg`/`svgz` join `IMAGE_EXTS`, so SVGs appear in the folder browse list and are accepted
   as drops.
8. **The pixel grid is suppressed for SVG**, and the placeholder is always filtered
   bilinearly. Both existing behaviours are defined in terms of image pixels, which a
   re-rasterized vector does not have; drawing a grid would mark the boundaries of a
   resolution that is an artefact of the current zoom.
9. **Prefetching a neighbour parses the tree *and* rasterizes it** at the transform that
   neighbour would actually be shown at, so that arrowing onto it is as instant as it is for
   a raster. That transform is known before switching: a carried zoom (ADR 0008) is already
   in hand, and the fit case is computable from the parsed size. Parsing alone would not be
   enough — parsing is the cheap half (0.1 ms for an icon), while rasterizing is what costs
   (up to 488 ms), so a parse-only prefetch would leave exactly the stall it exists to
   remove.

## Consequences

- The primary goal survives: SVG costs nothing on a normal launch, one ~60 ms launch per
  install, and less time to first pixels than a photo for realistic files.
- `DecodedImage` stops being the only thing the viewer can show. The cache holds either a
  raster or a tree-plus-last-raster, and eviction accounts for both — trees are small, and
  three viewport rasters are ~18 MB.
- The event loop gains cancellation of superseded background work, which it did not need
  before. This is the main complexity cost of the decision, and it is what levels 0 and 1
  would have avoided.
- Rendering is not browser-equivalent, and this will draw complaints. resvg does not
  implement SMIL or CSS animation, scripting, or `<foreignObject>`, and covers filters only
  partly. Exports from Figma, Illustrator and Inkscape render well; animated or
  HTML-embedding web assets do not. A valid file whose unsupported filter renders to
  nothing looks like a viewer bug and will not be caught by the `failed` path, because it
  parsed successfully.
- Text renders with *installed* fonts. A missing family substitutes silently and the
  layout stops matching the author's intent. Unfixable in any viewer that does not embed
  fonts.
- The first text-bearing SVG after a boot pays ~1.7 s of font scanning on the background
  thread. The window is up and responsive throughout, but the image appears late that once.
- resvg has no network code and executes no scripts, so a hostile SVG cannot phone home.
  It can reference local files through `xlink:href`; with no exfiltration path this is
  cosmetic for a local viewer, and a custom resolver can refuse it if that changes.
- A hostile or merely enormous file can occupy a decode thread for seconds. That is already
  true of large JPEGs, and the window-first architecture absorbs it — but with SVG the file
  size is no longer a proxy for the cost.
- `vgiew --dump <in> <out.png> [W H]` needs no new contract: its optional `W H` is exactly
  the raster size a vector document requires, so the headless verification path extends to
  SVG for free.
