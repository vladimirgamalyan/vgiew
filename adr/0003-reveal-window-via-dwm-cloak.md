# 0003. Reveal the window via DWM cloak to avoid a startup flash

Status: Accepted

## Context

At startup the window briefly flashed white before the first frame appeared.
The previous mitigation (commit "Hide window until first frame") created the
window hidden (`with_visible(false)`), painted one dark frame with softbuffer,
then called `set_visible(true)`.

That does not work: softbuffer presents via GDI `BitBlt` into the window's DC,
and a present into a *hidden* window is discarded — it never populates the DWM
redirection surface that the compositor displays on `ShowWindow`. So on reveal
the window shows DWM's default (blank/white) surface for a frame before the next
`WM_PAINT` repaints it. winit's window class uses `hbrBackground: 0` (null
brush), so the white does not come from an erase brush; it is the uninitialized
redirection surface.

Alternatives considered:

- **Dark class background brush** (`SetClassLongPtrW` + `CreateSolidBrush`) —
  cheap, but on its own only recolors the flash to dark rather than eliminating it.
  Insufficient as the primary fix, so it is not chosen as such; it is layered on as
  a backstop to the cloak (see Decision).
- **Synchronous first decode before reveal** — orthogonal; it addresses "dark
  frame then image pops in", not the flash, and it conflicts with the
  "window shows immediately, decode in the background" architecture.
- **Transparent / layered window** — risky with our `0x00RRGGBB` pixel format
  (alpha byte 0 would read as fully transparent) and heavier machinery.

## Decision

We will reveal the window using DWM cloaking. The window is created hidden, then
cloaked (`DwmSetWindowAttribute(DWMWA_CLOAK, TRUE)`) and shown: it is composited
(its surface exists, so a GDI present lands in it) but not displayed. We paint the
first frame into that surface, then uncloak — the window appears already sized and
painted. This is the standard Win32 flash-free startup pattern.

As a backstop, we also set the window class background brush to the dark background
color (`SetClassLongPtrW(GCLP_HBRBACKGROUND, CreateSolidBrush(...))`), so any erase
that slips through fills dark instead of white even if cloaking is unavailable.

## Consequences

- The startup white flash is eliminated, and the earlier resize flash stays
  covered too (the show/resize dance happens while cloaked, i.e. off-screen).
- We keep the "window shows immediately, decode runs in the background" design:
  the revealed first frame is the dark background; the image appears when its
  background decode completes.
- We take on a small amount of Windows-only `unsafe` FFI to `dwmapi`
  (`DwmSetWindowAttribute`), `gdi32` (`CreateSolidBrush`), and `user32`
  (`SetClassLongPtrW`), and depend on the DWM compositor being active — always true
  on the Windows 10/11 targets. Non-Windows stubs keep the code compiling elsewhere.
  The backstop brush is a process-lifetime GDI object the OS reclaims on exit.
- The letterbox border around an aspect-fit image (the `BG` fill) is unchanged and
  intentional; it is not part of this decision.
