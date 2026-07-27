# 0019. Animate GIF, APNG and WebP from a streaming ring buffer

Status: Proposed

## Context

`concept.md`'s non-goals list names **GIF animation** as deferred out of scope, and
`docs/FEATURES.md` advertises "GIF (first frame)". That non-goal was set while the MVP's
only priority was startup latency and before anything about animation had been costed. This
ADR reverses it deliberately; the reasoning below is what the original entry lacked.

**What the `image` crate already does for us.** The genuinely hard part of GIF is not
decoding, it is composition: frames are cumulative, a frame may cover only a sub-rectangle
of the canvas at an offset, and what remains of the previous frame depends on a per-frame
disposal method. `GifFrameIterator` implements all of it — it keeps a persistent canvas,
honours `Any`/`Keep`/`Background`/`Previous`, and yields a **full-canvas RGBA image** per
frame. `gif 0.14.2` is already in `Cargo.lock` via `image`'s `gif` feature, so there is no
new dependency and no binary growth: the code is already linked, we merely call more of it.

**One trait covers three formats.** `AnimationDecoder` is implemented by `GifDecoder`,
`ApngDecoder` and `WebPDecoder` alike — same `Frames` iterator, same `Delay`, same
full-canvas RGBA output. Deciding whether a given file is actually animated is a
header-level probe in each case: `PngDecoder::is_apng()` (presence of the `acTL` chunk) and
`WebPDecoder::has_animation()` are flags, and for GIF it is whether a second frame follows
the first. So the playback engine is a fixed cost and each additional format after it is a
detection branch, not a second implementation.

**The renderer needs nothing new.** An animation frame is a `DecodedImage`, so `draw_raster`
serves it unchanged, including the transparency checkerboard, the nearest/bilinear split and
the pixel grid of ADR 0011.

**The timer is nearly free.** `ControlFlow` defaults to `Wait` in winit 0.29 and vgiew never
calls `set_control_flow`, so the viewer is fully event-driven and idle at zero CPU today.
`WaitUntil(Instant)` turns that into a timer with no polling loop and no busy thread.

**The frame budget is already measured and sufficient.** `concept.md` reports ~285 FPS in a
typical window on the multithreaded CPU path, and ~55 FPS at 4K fullscreen where the
bottleneck is the GDI present (~10 ms) rather than resampling (~8 ms). Animation cadence is
10–25 fps. Rendering is not the constraint, at any window size.

**Memory is the constraint.** A frame costs `w*h*4` bytes and there are N of them:
500×500×100 = 100 MB, 800×600×200 = 384 MB, 1920×1080×200 = 1.7 GB. `evict` bounds the cache
to `{prev, current, next}`, which would multiply the peak by three.

Four memory strategies were considered:

- **Decode everything, unbounded.** Rejected: a gigabyte of RAM off a two-megabyte file is a
  routine scenario for a screencast GIF, not a pathological one.
- **Decode everything up to a cap, fall back to a static first frame above it.** Attractive,
  because the fallback is exactly today's behaviour — already written, already tested — and
  full residency gives free random access. Rejected because a long file then silently does
  not animate, and "it works on my files but not yours" is the hardest failure mode to
  explain in a viewer with no status line.
- **Decode on the fly, keeping only the current frame.** O(1) memory and any file animates,
  but the format forbids the cheap version: frames are cumulative, so there is no seeking,
  and every loop re-runs LZW over the whole file. That is constant CPU for as long as a
  window stays open, in a utility whose entire premise is being cheap.
- **A ring buffer of decoded frames under a byte budget.** Chosen.

The ring buffer has one property that also settles the size of the budget: **when a whole
file fits the budget, the buffer degenerates into "every frame resident" and loop wrap costs
nothing.** Only a file that exceeds the budget pays a decoder restart per loop. So the number
does not answer "how much memory do we allow" but "where does the line fall between a file
that loops for free and one that re-decodes every round".

| Budget | 500×500 held whole | 1080p held whole | 1080p runway at 10 fps |
|---|---|---|---|
| 64 MB | 64 frames | 7 frames | 0.7 s |
| 256 MB | 256 frames | 30 frames | 3 s |
| 1 GB | 1024 frames | 123 frames | 12 s |

A typical meme is 20–60 frames at 300–500 px, i.e. tens of megabytes — comfortably resident
at 256 MB. A 1080p screencast at 200 frames is 1.7 GB decoded and streams under any budget
worth having.

An adaptive budget (a fraction of physical RAM) was considered and rejected: it needs
`GlobalMemoryStatusEx`, and it makes behaviour differ from machine to machine, which turns
bug reports into irreproducible ones.

This budget is ours alone and does not interact with `image`'s own `Limits` (default
`max_alloc` 512 MB), which bounds the crate's single internal compositing canvas rather than
anything we store.

## Decision

We will show animated GIF, APNG and animated WebP as a **third `Frame` variant fed by a
background decoder through a ring buffer bounded at 256 MB**:

1. **One engine, three formats.** The playback machinery is written against
   `AnimationDecoder` and knows nothing about the container. GIF, APNG and animated WebP are
   supported in the same change, because the marginal cost per format after the first is a
   detection branch of roughly ten lines, and shipping animation for one of three would
   leave `docs/FEATURES.md` claiming support for formats whose animation is silently dropped.
2. **A file with one frame stays a `Raster`.** Nothing that is not animated pays for the
   animation path — no timer, no worker, no ring buffer. For APNG and WebP the probe is a
   header flag; for GIF it means taking the second frame and finding it absent, which costs
   one extra frame's decode on a static GIF and nothing on an animated one. `ApngDecoder` may
   report a leading *thumbnail* frame that is not part of the animation, which the probe must
   account for or a static APNG will look animated with one frame.
3. **Streaming, not blocking.** The first frame is shown as soon as it exists and the rest
   arrive while it is on screen. Waiting for a whole animation before the first present would
   put hundreds of milliseconds in front of the window for a long file, which is the one
   thing this project does not trade away. Streaming is not an independent choice here — a
   ring buffer means the decoder runs behind playback by construction.
4. **The ring buffer is denominated in bytes, not frames.** A single 4000×4000 frame is
   64 MB, so a frame count cannot bound anything: "hold 32 frames" is 2 GB on such a file.
   The decoder fills ahead of the playback position and stops when the budget is reached.
5. **Loop wrap re-opens the file** for an animation that did not fit the budget. There is no
   alternative within the format: frames are cumulative, so returning to frame 0 means
   decoding from the start. An animation that did fit has every frame resident and wraps
   with no work at all.
6. **Timing is wall-clock, not tick-counted.** The next deadline is computed from the
   animation's start instant plus accumulated delays, not by adding one delay per redraw.
   Otherwise every frame that overruns its delay — plausible at 4K fullscreen against a
   10 ms delay — permanently shifts the animation later, and a long GIF drifts visibly out
   of time. Frames whose deadline has already passed are skipped for *rendering*; they are
   still decoded, because the format gives no way to skip them.
7. **Frame delays are normalised on the browser convention.** `image` passes a GIF's delay
   through as centiseconds × 10 with no normalisation, so the very common `delay: 0` —
   authored to mean "as fast as possible" — arrives as 0 ms and would spin the timer as fast
   as the CPU allows. A delay of 10 ms or less becomes 100 ms, matching what every browser
   does, so such files play at the speed their authors actually observed.
8. **Decoder underrun holds the current frame** and lets the cadence sag. Dropping ahead is
   not an option the format offers, so this is a description of the only possible behaviour
   rather than a preference.
9. **The timer is armed only while an animation is current, visible and playing.** On
   `WindowEvent::Occluded(true)` — minimised or fully covered — the timer and the decoder
   stop, and playback resumes from the frame it stopped on. A quick-look utility left open
   behind other windows must not burn a core and a battery on frames nobody is watching.
10. **Prefetching a neighbour decodes its first frame only.** The neighbour enters the cache
    as an ordinary static image and the decoder plus ring buffer spin up only when the file
    becomes current. This deliberately differs from ADR 0016 point 9, which rasterizes a
    neighbouring vector in full: there, the expensive half was rasterization and a
    parse-only prefetch would have left exactly the stall it existed to remove. Here the
    first frame *is* what appears on screen at the moment of the switch, so it covers the
    stall completely, and full prefetch of two neighbours would either triple the memory
    budget or cut each buffer to a third of it — to speculate on files the user may never
    reach.
11. **The renderer is untouched, pixel grid included.** Unlike a re-rasterized vector
    (ADR 0016 point 8), an animation frame has real image pixels, so the grid of ADR 0011 and
    the nearest-neighbour step at high zoom mean exactly what they always meant.
12. **Detection stays by content** and `IMAGE_EXTS` is unchanged — `gif`, `png` and `webp`
    are already listed, so browse order, drops and registration need no adjustment.
13. **`vgiew --dump` keeps rendering the first frame.** It is a headless check of the
    decode-and-draw pipeline; adding a frame index to its contract would serve no verification
    the animated path needs.

## Consequences

- **An oversized animation stutters at the seam between loops.** A 1080p screencast that does
  not fit 256 MB must re-open and re-decode from frame 0 at the end of every round, which
  will be visible. This is inherent to the format, it is the price of not silently refusing
  to animate long files, and it is recorded here so it is recognised as a known compromise
  rather than filed as a bug.
- **Four separate code paths change `current`** — the `Decoded`, `FolderChanged` and `Open`
  handlers and the arrow keys — and every one of them must now reset the animation clock and
  tear down the decoder. Missing one leaves the previous file's animation running under a new
  image. This is the main correctness risk of the decision.
- **`load_frame` has to be restructured**, and it sits on the hot startup path. Today it tries
  `load_rgba` for everything and only offers a rejected file to the SVG parser, deliberately
  so that "the common case keeps exactly the I/O it always had". Probing for animation means
  constructing decoders ourselves rather than letting `ImageReader::decode()` do it
  internally. The probes themselves are header-level and add no I/O, but time-to-first-pixel
  for an ordinary JPG or PNG must be measured before and after, not assumed unchanged.
- **`DecodedImage` was already not the only thing the viewer can show** (ADR 0016); now the
  cache holds a raster, a vector document, or an animation whose size varies over its
  lifetime as the buffer fills. `evict`'s accounting stops being a simple `w*h*4`.
- **Each stored frame costs one extra conversion pass.** `image` yields RGBA8 bytes and we
  store `0xAARRGGBB` u32, so `pack_rgba` runs per frame. It is already rayon-parallel, and
  the stored footprint is `w*h*4` either way, but the transient allocation is doubled at the
  moment of conversion.
- **The test surface grows and the samples do not exist.** `test-images/` holds no animated
  file of any kind, and `cargo test` currently covers only pure functions. Verification needs
  deliberately awkward inputs: `delay: 0`, a single-frame GIF, disposal `Previous`, a frame
  smaller than the canvas at an offset, an APNG with a leading thumbnail frame, and a file
  large enough to exceed the budget and exercise the wrap path.
- **A hostile file can now occupy a decoder thread indefinitely** rather than for one decode.
  ADR 0016 already accepted that file size stopped being a proxy for cost; an animation
  extends that from a bounded decode to an unbounded loop. The byte budget bounds the memory
  but not the CPU, and only closing the window or browsing away stops it.
- **The window is no longer idle while open.** Every prior version of vgiew consumed nothing
  between events. Occlusion gating limits this to a window the user can actually see, but a
  visible animated GIF now costs CPU continuously, which is a change in the character of the
  program and not only in its feature list.
- **`concept.md`'s non-goal entry and the "GIF (first frame)" wording in `docs/FEATURES.md`,
  `README.md` and `concept.md` become wrong** and must be updated when the implementation
  lands, not before — until then they correctly describe shipped behaviour.
