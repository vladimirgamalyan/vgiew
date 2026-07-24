# vgiew

A fast, minimal image viewer for Windows, built for **instant startup**. Double-click
an image in Explorer and it's on screen in tens of milliseconds — no splash, no runtime,
no clutter.

> The name is a working title and may change.

## Status

- **Image viewer — working MVP.**

Sound playback used to live here too; it now has its own project, **vgplay**.

The whole design and the measurements behind the key decisions live in
[`concept.md`](concept.md).

## Why it's fast

- Native Rust, a single self-contained `.exe`, no runtime to spin up.
- The window and first frame appear immediately; decoding runs on a background thread.
- CPU rendering via `softbuffer` — no GPU context to initialize. On the target hardware
  this starts ~3–5× faster than a GPU-backed path (egui/wgpu); see `concept.md` for the
  full cold-start and interaction benchmarks.
- Multithreaded resampling (`rayon`) — smooth zoom/pan even on 24 MP images.

## Features

Fast startup and switching, fit-to-window, cursor-centered zoom and pan, crisp
resampling, folder browsing in natural sort order, copy to clipboard (`Ctrl+C`, as
both file and pixels), delete to the Recycle Bin, and fullscreen. Formats: JPG, PNG,
GIF (first frame), BMP, WEBP, detected by content.

See **[docs/FEATURES.md](docs/FEATURES.md)** for the full feature list and hotkey reference.

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
