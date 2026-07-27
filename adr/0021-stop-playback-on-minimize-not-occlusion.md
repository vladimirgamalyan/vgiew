# 0021. Stop playback on minimize, because Windows never reports occlusion

Status: Accepted

Supersedes point 9 of [0019](0019-animate-via-streaming-ring-buffer.md).

## Context

ADR 0019 point 9 says playback and the decoder stop "on `WindowEvent::Occluded(true)` —
minimised or fully covered". Implementing it showed the event does not exist on the platform
this viewer targets: `winit 0.29`'s Windows backend contains no reference to `Occluded` at
all, so the handler would have been dead code and the animation would have kept running
behind whatever was covering it. The event is real on macOS, iOS and Web; Windows is simply
not among the backends that emit it.

What Windows does offer is `IsIconic`, which `winit` exposes as `Window::is_minimized()`.
That answers "is this window minimised" and nothing else. There is no supported way to ask
"is this window fully covered by another one": `DwmGetWindowAttribute(DWMWA_CLOAKED)` reports
cloaking rather than occlusion, and the several ways of inferring coverage — walking the
z-order and intersecting rectangles, or polling a `PrintWindow` capture — are either wrong in
the presence of transparency and multiple monitors, or cost more than the frames they would
save.

Three approaches were weighed:

- **Keep the `Occluded` handler as written.** Rejected: it compiles, it reads as if the
  behaviour exists, and it never fires — the worst of the three, because the next reader has
  no way to tell.
- **Infer coverage from the window z-order.** Rejected: unreliable for exactly the cases
  worth catching, and it would run per frame.
- **Ask `is_minimized()` each time round the loop.** Chosen.

Polling is acceptable here only because the loop is already awake: the question is asked in
`AboutToWait`, which for an animation runs at the frame rate and for anything else runs when
an event arrives. `IsIconic` is a field read on the window struct in the kernel — cheaper than
the timer arithmetic it guards.

## Decision

1. **Playback and the decoder stop while the window is minimised, and not otherwise.** A
   window that is merely covered by another one keeps animating. This is narrower than
   ADR 0019 point 9 promised, and it is as far as the platform allows.
2. **Being hidden is polled, not awaited.** `window.is_minimized()` is read in `AboutToWait`
   alongside the pause flag; there is no `Occluded` handler and no cached flag fed by events.
3. **Pause and minimize share one path.** Both stop the clock, and both re-base it on the
   frame on screen when it starts again, so neither races through the frames nobody watched.
   Keeping them separate would have meant two ways to say the same thing.

## Consequences

- **A visible-but-covered window still costs CPU.** The frames are still decoded, resampled
  and presented for a window nobody can see. Measured on a 400×400 GIF the cost collapses on
  its own — Windows clips the present, leaving only the resample — but it is not zero, and on
  an animation that exceeds the ring buffer the decoder keeps running at full tilt.
- **Minimizing is verified to cost exactly nothing**, streaming animations included: the
  timer stops, playback stops advancing, and the decoder blocks on a buffer nobody is
  draining.
- **The check runs on every pass of the event loop**, so a future change that makes the loop
  busy for some other reason inherits a per-iteration syscall that was costed against a loop
  which sleeps.
- **This ADR becomes wrong the moment `winit` emits `Occluded` on Windows.** The upstream
  event is the better mechanism and the code should move back to it; that is a reason to
  re-read this record, not to re-derive the answer.
