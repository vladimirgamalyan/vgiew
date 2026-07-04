# 0005. Persist and restore window position and size

Status: Accepted

Supersedes decision #2 (window sizing) of
[0004](0004-open-scale-window-size-and-zoom-centering.md). Decisions #1
(open scale = shrink-to-fit) and #3 (zoom-to-cursor then clamp-and-center)
of 0004 remain in force.

## Context

ADR 0004 sized the window to the image on open (shrink-to-fit size, capped at
~90% of the primary monitor work area) and left window *placement* to the OS.

Users expect an image viewer to reopen where they last left it — same
position, same size. Two forces made this the right time to add that:

- With per-run persistence, "size the window to the image" only ever applies
  on the very first run; every later run restores the remembered window. A
  window that is image-sized once and then remembered is the odd combination —
  the remembered size wins from then on regardless.
- The saved geometry must survive a changed environment: a corrupt/absent
  value, a monitor that was removed, or a lower resolution than when it was
  saved. Restoring a window onto a monitor that no longer exists, or larger
  than the current screen, would put it off-screen or oversized.

Open questions were: where to store the geometry, how to validate it, and what
to do on the first run or when the saved value is unusable.

Alternatives considered:

- **Keep 0004's image-sizing as the first-run fallback** (persistence layered
  on top). Rejected: it keeps a header pre-read and the `SPI_GETWORKAREA`
  sizing path alive only for the first-ever run, and mixes two different
  "how big should the window be" rules. The simpler, more predictable
  behavior is one rule — restore, else OS default.
- **Store geometry in a file** (`%APPDATA%`/`%LOCALAPPDATA%`). Rejected: the
  app already writes HKCU via `winreg` for file associations; the registry is
  the consistent, dependency-free store, and a single string value is trivial
  to validate.
- **Clamp a fully-off-screen saved window back onto the primary monitor**
  instead of falling back to the OS default. Rejected: a saved position that
  lands on no current monitor means the layout changed enough that the
  remembered spot is meaningless; treating it as "no usable data" and letting
  Windows place the window is clearer. A window that still lands on a monitor
  but overflows it *is* fitted (not discarded) — that is the resolution-change
  case the request calls out.

## Decision

We will persist and restore the window geometry, and drop image-based window
sizing:

1. **Store.** `HKCU\Software\vgiew`, value `WindowGeometry` = `"x,y,w,h"`:
   the outer position `x,y` (screen pixels) and the inner/client size `w,h`
   (physical pixels). Written once on `Event::LoopExiting`, from the last
   known *windowed* (non-fullscreen) geometry, which is tracked live via
   `WindowEvent::Moved`/`Resized` (updates ignored while fullscreen).
   `--unregister` deletes this key so uninstall leaves nothing behind.

2. **Restore + validate.** On launch, read and parse the value. Any malformed
   input (missing/extra field, non-integer, non-positive size) is treated as
   absent (corruption check). If it parses and its rect still lands on an
   existing monitor (`MonitorFromRect` + `MONITOR_DEFAULTTONULL`), the window
   is created at that position and client size, then `fit_window_to_screen`
   shrinks and nudges the *outer* window (frame included, now that its real
   frame is known) to fit the monitor work area — handling a saved window that
   no longer fits a smaller resolution.

3. **Fallback = OS default.** First run, corrupt data, or a saved position on
   a monitor that no longer exists → set neither position nor size on the
   window; Windows assigns the default (winit's default client size and a
   `CW_USEDEFAULT` cascade position). This replaces 0004's "size the window to
   the image".

4. **Fullscreen is not persisted.** Only the windowed geometry is remembered;
   the app always starts windowed.

## Consequences

- The window reopens where the user left it, at the size they left it, and is
  guaranteed to fit fully on screen after a resolution or monitor-layout
  change. On the first run it opens at the OS default.
- The image-sizing path from 0004 is removed: `initial_window_size`, the
  `SPI_GETWORKAREA` `work_area` helper, and the `read_dimensions` header
  pre-read are gone. We give up the "snug window sized to this image" feel on
  the first run in exchange for standard remember-my-window behavior (which is
  what applies on every subsequent run anyway). The background-decode / instant
  window design is unchanged — nothing here reads pixels before showing.
- One Windows-only settings value is written under `HKCU\Software\vgiew` and
  removed on `--unregister`. Multi-monitor and resolution changes are handled
  with `MonitorFromRect` + `GetMonitorInfoW` (with non-Windows stubs).
- Decisions #1 and #3 of 0004 (shrink-to-fit open scale; clamp-and-center on
  zoom/pan) are untouched and still apply.
