# 0002. Render on the CPU (softbuffer), not the GPU

Status: Accepted

## Context

The project's overriding goal is the fastest possible startup (see `concept.md`,
section 1). The graphics backend is the single biggest cost at startup and also
governs runtime smoothness. Three candidate paths were considered:

- **Tier A** — `eframe`/`egui` + wgpu (a full GUI toolkit, everything ready).
- **Tier B** — `winit` + a thin GPU blit (`pixels`/D3D11/Direct2D).
- **Tier C** — `winit` + `softbuffer`, a CPU pixel buffer with no GPU device.

The open worry was that CPU resampling would be too slow for smooth zoom/pan on
large images, forcing a GPU path despite its slower startup.

We settled this by measuring on the target machine (Ryzen 9 9950X3D, RTX 5070 Ti +
integrated Radeon, Windows 11); the spikes and scripts live in `spikes/`, full
numbers in `concept.md`:

- **Cold start:** Tier C ~46 ms wall-clock vs egui ~199 ms vs pixels ~537 ms.
- **CPU interaction** (24 MP → 1600×1000, bilinear): ~90 FPS single-threaded,
  ~650 FPS multithreaded — well above 60/144 Hz.
- **Tuned GPU** (recent wgpu 22, LowPower/integrated, various backends): best case
  ~150 ms, still 3–5× slower to start than the CPU path. GPU initialization has a
  hard floor (~150 ms here) that no tuning removed.

## Decision

We will render on the CPU using `winit` + `softbuffer` with multithreaded
resampling (`rayon`) — Tier C. Filtering is nearest-neighbor when zooming in
(scale ≥ 1) and bilinear when zooming out (scale < 1).

## Consequences

- Startup is the fastest of all options (~46 ms), directly serving the primary goal.
- Interaction is smooth on the CPU; multithreading leaves large headroom.
- We give up a batteries-included GUI: the sound player's widgets and text must be
  drawn by hand (the image viewer needs almost no UI). This is the main cost.
- A GPU path is justified only for one narrow case — 4K-fullscreen present, where the
  GDI blit becomes the bottleneck. That stays an option behind a measurement gate
  (see the deferred "CPU start → GPU in background" hybrid in `concept.md`), not part
  of the base design.
