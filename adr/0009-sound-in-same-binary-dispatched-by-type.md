# 0009. Play sound in the same binary, dispatched by file type

Status: Superseded by 0014

## Context

vgiew is adding sound playback (`concept.md`, section 6). The open question was
whether the sound player should live in the **same** `vgiew.exe` (dispatching by
file type) or in a **separate** project/binary. `concept.md` already sketches a
single exe that dispatches by type, but the trade-off was raised again and worth
recording with its reasoning so it is not re-litigated.

Forces:

- **Startup speed is priority #1** (ADR 0002, `concept.md` section 1). The worry
  was that linking an audio stack (`rodio` → `cpal`/`symphonia`) would slow the
  image path's cold start. In practice it does not: Rust runs no implicit static
  constructors, `cpal`/`symphonia` initialize lazily (no output device is opened
  until a stream is created), and demand paging keeps unexecuted audio code off
  the image path. The only cost is a slightly larger `.exe` and a trivial
  type-dispatch at launch; the whole audio init (device open, decode) is paid
  only when a sound is opened.
- **Shared infrastructure is ~80% of the work.** Both an image and a sound are
  "double-click a file in Explorer → instant single window, browse siblings in
  the folder." Window (`winit`), CPU present (`softbuffer`), the no-flash reveal
  (ADR 0003), install/associations (`--register`, `install.ps1`), natural-sort
  folder navigation, and the hand-drawn UI rasterizer are all shared. Only the
  decode-and-render (pixels) vs decode-and-play (samples) 20% differs.
- **Windows association economics** favor one artifact: one ProgID scheme, one
  install script, one binary to version and keep the double-click binding alive.

Alternatives considered: a separate binary/project, or a cargo workspace with a
shared `core` crate and two thin binaries. Both duplicate or externalize the
shared 80% for little gain, given audio adds no measurable startup cost.

## Decision

We will keep sound playback in the same `vgiew.exe`, dispatched by file type at
launch, with **audio initialized lazily** — only on the sound path. The image
path opens no audio device and pays nothing for linking `rodio` beyond binary
size. Audio lives behind a clean module boundary (`run_sound`) so it can be split
out later cheaply if the player ever grows a diverging, heavyweight UI.

## Consequences

- The image path's cold start is unchanged (lazy audio init; verify with
  `spikes/measure.ps1` if ever in doubt).
- No duplication of the window / render / install / navigation infrastructure.
- One binary, one association scheme, one release to ship — simplest for the
  Windows double-click flow.
- The `.exe` grows (rodio + symphonia PCM/RIFF + cpal). Acceptable; only WAV
  decoding is linked for now (`rodio` features `playback`, `wav`).
- If the sound player later diverges enough (playlists, EQ, visualizations) to
  stop sharing the image path, splitting the `run_sound` module into its own
  crate is a small refactor, not a rewrite. Revisit only then.
