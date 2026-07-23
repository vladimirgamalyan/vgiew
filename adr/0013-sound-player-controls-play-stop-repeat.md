# 0013. Sound player controls: hand-drawn play/stop and repeat toggle

Status: Accepted

## Context

The sound path (ADR 0009) opened a bare window that played a file once with no
controls. `concept.md` §6 sketches a fuller player (play/pause, progress bar,
time, volume). The immediate need was smaller: a way to stop/restart playback
and to loop a file. This is the first interactive UI in the app — until now
both the image and sound windows were passive panes — so it also sets the
pattern for how widgets get drawn on the CPU/softbuffer path (ADR 0002), where
there is no widget toolkit.

Forces and choices:

- **How to draw widgets.** `concept.md` §8 anticipated a font rasterizer
  (`ab_glyph`/`fontdue`) for text. Two icon buttons need no text, so linking a
  font stack now would be premature. Instead the buttons are pure geometry
  (disc, triangle, square, annulus arc) rendered by a single supersampled
  fill primitive (`fill_aa`, SS×SS coverage) that anti-aliases any inside-test.
  The whole UI is repainted each frame; at this size it is negligible.
- **Play/Stop semantics (stop vs pause).** The button is *stop*, not pause: it
  ends playback and a later *play* restarts from the beginning. This maps
  cleanly onto rodio — `Player::stop()` empties the queue, and *play* re-opens
  the file and `append`s a fresh (lazy) decoder — and avoids depending on
  `try_seek` support for a rewind. A single `active` flag is the button's
  source of truth (stop square while playing, play triangle while stopped).
- **Repeat.** rodio has no "loop" on `Player`, and wrapping the source in an
  infinite repeat would make *stop* the only way out and complicate the queue.
  Instead the event loop polls `Player::empty()` (every 50 ms, only while
  `active`) and re-appends the file when a track ends naturally; a manual stop
  clears `active` first, so it is not treated as a natural end and does not
  loop. Toggling repeat never itself starts playback.
- **Repeat glyph.** Drawn as a circular arrow (annulus arc with a gap and an
  arrowhead) — the widely-read "repeat/refresh" symbol — filled accent with a
  white glyph when on, a muted disc with a gray glyph when off, so the toggle
  state is legible at a glance.
- **Layout in physical pixels.** Button centers/radius come from the live
  `inner_size()` via one shared `sound_layout`, used by both drawing and
  hit-testing so they cannot drift, and so the buttons scale with window DPI.

## Decision

We will give the sound window two hand-drawn round buttons — a play/stop button
and a repeat toggle — drawn with a supersampled shape rasterizer (no font/widget
library). Stop empties the rodio queue and play re-decodes from the start;
repeat is implemented by polling `Player::empty()` and re-appending the file on
natural end. Playback still starts automatically on open.

## Consequences

- Establishes the CPU-path widget approach (geometry + `fill_aa`) that later
  controls (progress bar, volume) can reuse; a text rasterizer is deferred
  until something actually needs glyphs.
- No new dependency; binary size and the image path's cold start are unchanged
  (audio and this UI are still only reached on the sound path).
- Stop-then-play re-opens and re-decodes the file rather than resuming in place;
  acceptable because the decoder is lazy (header-only on `append`) and this is a
  user-initiated action, not a hot path.
- Repeat loops through a ~50 ms poll, so there is a small gap between iterations
  rather than a gapless loop. Acceptable for the MVP; revisit if gapless
  looping is wanted.
- The player still lacks the progress bar, time readout, and volume from
  `concept.md` §6; those remain future work and can build on this scaffolding.
