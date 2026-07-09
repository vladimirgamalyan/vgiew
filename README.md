# vgiew

A fast, minimal image viewer for Windows, built for **instant startup**. Double-click
an image in Explorer and it's on screen in tens of milliseconds — no splash, no runtime,
no clutter. A sound player is planned (see [`concept.md`](concept.md)).

> The name is a working title and may change.

## Status

- **Image viewer — working MVP.**
- Sound player — early: plays WAV files (opens a window, plays once; no controls yet).

The whole design and the measurements behind the key decisions live in
[`concept.md`](concept.md).

## Why it's fast

- Native Rust, a single self-contained `.exe`, no runtime to spin up.
- The window and first frame appear immediately; decoding runs on a background thread.
- CPU rendering via `softbuffer` — no GPU context to initialize. On the target hardware
  this starts ~3–5× faster than a GPU-backed path (egui/wgpu); see `concept.md` for the
  full cold-start and interaction benchmarks.
- Multithreaded resampling (`rayon`) — smooth zoom/pan even on 24 MP images.

## Features (images)

- Fit-to-window with letterboxing; window title shows name, pixel size, and zoom.
- Zoom to the point under the cursor; drag to pan; fullscreen.
- **Crisp zoom:** nearest-neighbor when zooming in (sharp pixel edges), bilinear when
  zooming out (no aliasing).
- Browse neighboring images in the same folder, natural sort order (`file2` before `file10`).
- Formats: JPG, PNG, GIF (first frame), BMP, WEBP. Format is detected by content, not
  by extension.
- Background/transparency composited over a neutral dark background.

## Hotkeys

| Key | Action |
|-----|--------|
| `←` / `→` | previous / next image |
| Mouse wheel | zoom to cursor |
| Left-drag | pan |
| `F` / `Enter` | toggle fullscreen |
| `Esc` | exit fullscreen / close |
| `0` | fit to window |
| `1` | 100% (1:1) |

## Build

Requires the Rust toolchain (`stable-x86_64-pc-windows-msvc`) and the MSVC C++ build
tools (Visual Studio Build Tools).

```powershell
cargo build --release
# run:
target\release\vgiew.exe path\to\image.png
```

## Install and bind to double-click

`install.ps1` builds a release, installs into a stable per-user path
(`%LOCALAPPDATA%\Programs\vgiew`, no admin required), and registers vgiew as a handler:

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

Then, one time, set it as the default: right-click an image → **Open with → Choose
another app → vgiew → Always** (Windows 11 requires this manual confirmation and does
not allow setting a default silently). Because the install path is stable, every later
release is just `install.ps1` again — the association keeps working.

To install into `Program Files` instead (needs an elevated terminal):

```powershell
powershell -File install.ps1 -InstallDir "C:\Program Files\vgiew"
```

Remove everything with `uninstall.ps1`.

## Development

- `vgiew --dump <in> <out.png> [W H]` — headless render of one frame, for verifying the
  decode/fit/resample pipeline without a window.
- `vgiew --help` — CLI help.
- `spikes/` — throwaway benchmarks used to choose the graphics path (cold start,
  interactive CPU cost, tuned GPU). Run with `spikes/measure.ps1` and `spikes/measure_gpu.ps1`.

## Requirements

Windows 10/11, x64.

## License

MIT — see [LICENSE](LICENSE).
