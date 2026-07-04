# vgiew — concept

> A fast, simple Windows 11 utility: opens an image or plays a sound on double-click
> in Explorer. One window, instant startup, minimal clutter.

*The name is a working title (after the project folder) and can be changed.*

---

## 1. Goal and principles

Replace the heavyweight built-in apps ("Photos", "Media Player") with a lightweight
utility that opens instantly and does exactly one thing: show the file.

### ⭐ Primary goal — the fastest possible response

**This is priority #1, and every other decision is subordinate to it.** The time
between the double-click and the image appearing on screen (or the sound starting)
must be as short as possible — ideally imperceptible. If a feature gets in the way
of startup or response speed, it is trimmed or deferred.

Targets (a direction, not hard limits):

- Cold start to first frame — **tens of milliseconds**, an "instant" feel.
- Browsing `←/→` to a neighboring file — **no delay**, the frame is already ready.
- Reaction to zoom/pan/seek — **within the same frame**, no stalls.

How this is achieved:

- **Native Rust, release + LTO**, a single self-contained `.exe` with no runtime and
  no heavy dependencies — there is nothing slow to initialize.
- **Show before ready:** the window and the first frame appear as early as possible;
  heavy work (full decode, metadata) happens after the window is up.
- **Decoding does not block the UI** — it runs on a background thread, the UI stays responsive.
- **Prefetch neighbors** (previous/next file) in the background → instant browsing.
- **Laziness:** nothing extra at startup — only what needs to be shown right now.
- Performance is measured and kept in check at every stage.

### Other principles

- **Simple** — no extra UI, no "startup" settings.
- **Single binary** — a self-contained `vgiew.exe` with no installer and no runtime.
- **One window** — opening a file = one window, neighboring files browsed with arrows.

---

## 2. What it does

A single `vgiew.exe` decides what to show based on the type of the passed file:

- **Image** → a viewer window with fit, zoom, pan, fullscreen.
- **Sound** → a compact player window with playback and seeking.

The type is determined by extension (and by file signature when needed).

---

## 3. Supported formats (MVP)

| Category | Formats |
|----------|---------|
| Images   | JPG, PNG, GIF (first frame), BMP, WEBP |
| Sound    | WAV, MP3, FLAC, OGG |

Future (not in MVP): AVIF/HEIC/JXL for images; M4A/AAC, Opus for sound.

---

## 4. Installation and double-click binding

- The utility takes a file path as a command-line argument.
- With no argument, an empty window opens.

### Intermediate releases (implemented)

A scheme for frequent dev builds on the work machine that keeps the double-click binding:

- **`install.ps1`** — build release → copy into `%LOCALAPPDATA%\Programs\vgiew`
  (per-user, **no admin rights and no UAC per release**) → register.
  The `-InstallDir` parameter allows installing into `Program Files` (needs admin).
  The script first kills running `vgiew` (otherwise the `.exe` is locked); the install
  path is **stable** — later releases simply overwrite the file.
- **`vgiew --register` / `--unregister`** — the `.exe` itself writes associations into
  `HKCU` (it knows its own path via `current_exe()`): ProgID `vgiew.image`, an entry in
  `.<ext>\OpenWithProgids` (candidate, the default is not hijacked), and `Applications\vgiew.exe`
  with `SupportedTypes`. `SHChangeNotify` notifies the shell.
- **Windows 11 does not allow silently setting the default** (UserChoice protection).
  So the user does a **one-time** right-click → "Open with → Always → vgiew".
  After that it's just `install.ps1`; the association holds via the stable path.
- Built with `windows_subsystem = "windows"` (no console flash on double-click);
  CLI subcommands (`--register`/`--dump`/…) attach to the parent console via `AttachConsole`.
- **`uninstall.ps1`** — removes associations and deletes the folder.

---

## 5. Image viewer

Behavior:

- The image **fits** into the window preserving aspect ratio.
- **Zoom** with the mouse wheel (toward the point under the cursor), **pan** by dragging.
- **Fullscreen** via `F` / `Enter` / double-click on the image.
- **1:1** and **fit** scales — via hotkeys.
- **Filtering by scale:** when zooming in (scale ≥ 1) — nearest-neighbor
  (crisp pixel edges, no blur, like other viewers); when zooming out (scale < 1) —
  bilinear (smoothing, no aliasing).
- **Transparency** is drawn as a checkerboard. **GIF** shows its first frame only.
- Window background — neutral dark.
- Window title: file name, pixel size, current scale.

Implementation details:

- Large images are scaled for display (no loading of giant 1:1 buffers into the GPU).
- Neighboring files can be prefetched in the background for instant browsing.

---

## 6. Sound player

A compact window with:

- File name.
- A **play/pause** button.
- A **progress bar** with seeking (click/drag).
- Current time / total duration.
- A **volume** control.

Behavior:

- Playback starts automatically on open.
- `Space` — play/pause; `←/→` in the player switch to a neighboring **audio** file
  (seeking within a track is a separate key, see the table).

*Future (optional):* visualization — a waveform or spectrum.

---

## 7. Folder navigation

- The arrows `←/→` browse files of **the same type only** as the opened one:
  opened an image → browse images, opened a sound → sounds.
- Ordering is **natural sort** by name, like in Explorer
  (`file2` before `file10`), case-insensitive.
- The neighbor list is built from the opened file's folder at launch time.

---

## 8. Technology stack

Language — **Rust**. Stack for the chosen CPU path (Tier C, see the measurements below):

| Task | Crate |
|------|-------|
| Window and events | `winit` |
| Pixel buffer output | `softbuffer` |
| Scaling/resampling | custom, multithreaded (`std::thread` / `rayon`) |
| Player UI (widgets, text) | custom drawing + `ab_glyph` / `fontdue` rasterizer |
| Image decoding | `image` (JPG/PNG/GIF/BMP/WEBP) |
| Audio playback | `rodio` |
| Audio decoding | `symphonia` (via `rodio`) |
| Natural sort | e.g. `natord` / custom |

`eframe`/`egui` was considered as the "batteries-included" option, but measurements
showed a startup ~3–5× slower than the CPU path (GPU context bring-up) — rejected for
the primary "instant startup" goal. The price of softbuffer is hand-drawing the player
UI, but the amount is small.

Build: `--release`, LTO, `panic = "abort"`, `strip`, a single self-contained `.exe`
for x86_64 Windows.

### Choosing the graphics path (the main speed fork)

The graphics backend is the biggest startup cost and the main factor in runtime
smoothness. **Important: fast startup ≠ fast response.** The lightest-on-startup path
(CPU pixel output) turns out slow on interaction: resampling a large image on the CPU
during zoom/pan drops FPS, whereas the GPU does it for free (a texture + bilinear sampling).

Three tiers to choose from:

| Tier | What | Startup | Response (zoom/pan) | Development |
|------|------|---------|---------------------|-------------|
| A | `eframe`/`egui` + wgpu | slower (GPU init) | excellent | easy, all ready |
| **B** | `winit` + thin GPU blit (`pixels`/D3D11/Direct2D) | **medium** | **excellent** | medium (hand-drawn widgets) |
| C | Win32/`winit` + `softbuffer` (CPU) | fastest | worse on large images | lots of manual code |

**Approach — "measure first":** build a tiny spike and measure the real cold start of
each tier on the target Windows 11 before committing. The single selection criterion:
minimal *perceived* latency at both startup and runtime.

#### Cold-start measurements (2026-07-04)

Hardware: Ryzen 9 9950X3D, RTX 5070 Ti + integrated Radeon, Windows 11.
`release` profile (LTO), 14 runs, cold run dropped. Spikes in `spikes/` (`measure.ps1`).

| Tier | Graphics | first-frame (internal) | wall-clock (≈click→screen) |
|------|----------|------------------------|----------------------------|
| **C — softbuffer** | CPU / GDI | **~32 ms** | **~46 ms** |
| A — eframe/egui | OpenGL (glow) | ~118 ms | ~199 ms |
| B — pixels | wgpu 0.17 (DX/Vulkan) | ~409 ms | ~537 ms |

Conclusions:

- **The CPU path (Tier C) starts practically instantly** — ~4× faster than egui and
  ~12× faster than pixels. The earlier prediction "the optimum is Tier B" was **not confirmed**.
- **pixels/wgpu is the slowest (~0.5 s)** — likely wakes the discrete RTX and/or the
  heavy old `wgpu 0.17`. Potentially tunable (LowPower adapter, recent wgpu), but as-is
  it's the worst option at startup.
- The GPU tiers also pay at process load (driver DLLs): the wall−internal gap is
  80–130 ms vs 14 ms for CPU.

Caveats: **startup only** was measured; interaction (zoom/pan/GIF on the CPU) is not yet
measured; Tier A records the time in the first `update`, slightly before the actual present.

#### CPU interaction measurements (2026-07-04)

A synthetic 24 MP image (6000×4000) → a 1600×1000 window, bilinear resampling,
120 frames with panning (`interactive_cpu`).

| Scene | 1 thread | 32 threads |
|-------|----------|------------|
| fit (~0.25×) | ~11 ms (~90 FPS) | ~1.5 ms (~650 FPS) |
| 100% | ~11 ms | ~1.5 ms |
| 300% | ~11 ms | ~1.5 ms |
| present (GDI, 1600×1000) | ~2 ms | — |

Conclusions:

- **CPU resampling is more than enough.** Even single-threaded ~90 FPS (above 60 Hz);
  multithreading (trivial on 32 cores) — ~650 FPS, headroom for 144 Hz+.
- Per-frame cost barely depends on scale — it's bound by the number of output window
  pixels. The earlier fear "the CPU drops FPS on large images" was **not confirmed**.
- Limits: 4K fullscreen (×5 pixels) needs multithreading (ST ~55 ms, MT ~8 ms);
  the GDI present gets heavier at 4K — the one place where a GPU present would really help.
  Naive bilinear aliases on heavy downscale → a high-quality fit is done as a one-time
  downscale to a working resolution. Decoding a real JPEG is a separate one-time cost
  in the background.

#### Tuned GPU check (2026-07-04, wgpu 22)

We tested the hypothesis "pixels is slow because of the discrete RTX and old wgpu" on
recent `wgpu 22` with different adapters/backends (`gpu_tuned`, `measure_gpu.ps1`):

| Config | Chosen adapter | first-frame |
|--------|----------------|-------------|
| GL | NVIDIA RTX (GL) | ~150 ms |
| DX12 / high | RTX (discrete) | ~235 ms |
| Vulkan / low | Radeon (integrated) | ~272 ms |
| DX12 / low | Radeon (integrated) | ~332 ms |
| auto | Radeon / Vulkan | ~483 ms |

Conclusions:

- The hypothesis was only partly confirmed: recent wgpu is notably faster than old pixels
  (~150 vs ~409 ms), but **preferring the integrated Radeon did NOT speed things up** — its
  driver initializes slower (270–480 ms) than the discrete RTX on DX12 (235 ms).
- The best GPU config (GL, ~150 ms) is still **~3–5× slower than the CPU path** (~32–46 ms).
  GPU initialization cannot be pushed below ~150 ms on this hardware by any means.
- The GPU device/context creation floor is fundamental — **no GPU tuning catches up to the
  CPU on startup.**

#### Graphics path verdict

**The CPU path (Tier C) wins on both fronts:** startup ~46 ms (3–5× faster than any GPU
config, including tuned wgpu 22, and 12× faster than old pixels) and interaction >60 FPS
even single-threaded, 144 Hz+ with MT. The only real price is **hand-drawing the player UI**
(widgets + text rasterizer); the image viewer needs almost no UI. A GPU path is justified
only for 4K fullscreen present.

**Decision (confirmed by measurements):** the base graphics path is **Tier C (softbuffer +
multithreaded CPU resampling)**. Keep the GPU in mind only as an option for 4K, if we hit
present cost.

#### "CPU start → GPU in background" hybrid: considered and deferred

The idea: show the first frame instantly on the CPU (~46 ms), bring up the GPU in the
background in parallel (~150–450 ms) and switch rendering to it for fast interaction.
Deferred, because:

- **The problem it solves barely exists.** The CPU path (MT) gives ~285 FPS in a typical
  window. Jank is possible only at 4K fullscreen (~55 FPS), and there the bottleneck is
  **present (GDI ~10 ms), not resampling** (~8 ms). So the GPU is needed for output, not scaling.
- **The cost is doubling the graphics:** two backends (softbuffer + wgpu) and handing the
  HWND between them on the fly (risk of a flicker at the swap). This is exactly the complexity
  Tier C was chosen to avoid.
- **Most of the work is wasted:** a quick-look utility (open→glance→close) — the GPU often
  won't finish coming up; needlessly waking the discrete card and draining battery.

**Gate:** revisit only if a measurement on the real app shows that 4K fullscreen with active
zoom is genuinely janky. And even then — add not a full second renderer, but an **optional
fast GPU present** behind autodetection/a flag.

### Why Rust, not C++

We considered switching to C++ for speed. Verdict: **the gain is negligible, we stay on Rust.**

Startup time consists of OS process load, window/graphics initialization, and file decoding.
None of these is sped up by changing the language:

- **Process load** — identical: both produce a native PE with static linking.
- **Runtime** — near-zero for both (no VM/JIT/GC, unlike .NET/Electron).
- **Graphics initialization** — the biggest startup cost, but it depends on the
  *framework and backend*, not the language.
- **Decoding** — the same SIMD code under the hood (libjpeg-turbo, libpng, etc.); Rust
  binds to it just as easily.

Machine-code quality is comparable for Rust (LLVM) and MSVC — the difference is within noise.
We have no hot loop where micro-optimizations would accumulate: we're bound by I/O, syscalls,
and decoder time — which are the same.

Meanwhile Rust offers advantages specifically for this task:

- **Memory safety without a GC** — the utility parses arbitrary (including malformed) files,
  and image/audio decoders are a classic source of crashes and vulnerabilities.
- **Ecosystem** (`image`, `rodio`, `symphonia`, `eframe`) plugs in with a single line;
  in C++ the same capabilities mean wrangling vcpkg/CMake and manual builds.
- **Faster to a working, fast MVP**, and the real speed wins are in the startup
  architecture (see section 1), not the language.

---

## 9. Hotkeys (summary)

| Key | Images | Sound |
|-----|--------|-------|
| `←` / `→` | previous / next file | previous / next track |
| `Esc` | exit fullscreen / close | close |
| `F` / `Enter` | fullscreen on/off | — |
| Mouse wheel | zoom | volume |
| Drag | pan | seek (drag on the bar) |
| `Space` | next file | play / pause |
| `1` / `0` | 1:1 scale / fit | — |

*The layout is preliminary; to be refined during implementation.*

---

## 10. Non-goals

To stay "fast and simple", the utility does **not**:

- Editing, rotate-with-save, format conversion.
- A gallery/thumbnail grid, cataloguing, tags.
- Playlists, an equalizer, network streaming.
- A settings "kitchen sink", themes, plugins.
- GIF animation and drag-and-drop into the window (deferred as out of scope).

---

## 11. Development stages

1. **Image MVP** — in progress. Done and verified: window (winit+softbuffer),
   open by argument, background decode (window shows immediately), format detection
   by content, fit + letterbox, multithreaded resampling (rayon), natural sort,
   title with name/size/scale.
   browsing `←/→`/`Space` with neighbor prefetch (instant on big photos),
   parallel RGBA pack.
   Implemented but not yet verified live: zoom-to-cursor, pan, fullscreen
   (`F`/`Enter`/`Esc`), `0`/`1` (fit/100%).
   There is a headless `--dump <in> <out.png>` mode for render checks.
2. **Sound MVP:** type detection, player window, play/pause, progress, volume,
   browsing through sounds.
3. **Polish:** transparency checkerboard (next). ✔ Done: neighbor prefetch,
   parallel RGBA pack, natural sort. (GIF animation and drag-and-drop → non-goals.)
4. **Optional:** reusing a single window on a repeated double-click
   (single-instance via a named pipe), extra formats, sound visualization.
   ✔ `--register`/`--unregister` + `install.ps1`/`uninstall.ps1` — already done
   (intermediate releases, see section 4).

---

## 12. Open questions

- Final name of the utility and the `.exe`.
- Do we need a delete command (`Del` → Recycle Bin) right from the window?
- Reusing the single window on a repeated double-click — do it now or defer to stage 4
  (needs single-instance + IPC)?
- The exact hotkey layout.
