# 0018. A dropped image lands like a browse step, keeping the zoom

Status: Accepted (supersedes the refit consequence of
[0015](0015-drop-image-opens-in-the-same-window.md))

## Context

[ADR 0015](0015-drop-image-opens-in-the-same-window.md) routes a dropped image
through the `UserEvent::Open` handler that was written for the ADR 0006 IPC
hand-off. That handler sets `fit_mode = true`, so a drop always refits the new
file — even when the user had zoomed in.

[ADR 0008](0008-keep-zoom-while-browsing-and-allow-sub-fit-zoom.md) settled the
opposite rule for ←/→ browsing: while at fit, each image is refit to its own fit
scale; once a manual zoom is set, the literal scale and pan carry onto the next
image. A user comparing images at 100% therefore keeps that zoom across arrows
but loses it across a drop, although both gestures do the same thing — show a
different image in this window. The inconsistency was reported as a defect, not
noticed as a design choice, which is the new information 0015 did not weigh.

Alternatives considered:

- **Keep the refit and call it deliberate.** A drop usually names a file from
  another folder, so it is arguably "open a new file" rather than "next image",
  and a carried 100% on an unrelated 24 MP photo shows a corner instead of the
  picture. Rejected: 0008 already accepted exactly that cost for browsing (a
  carried sub-fit zoom shows a tiny image), with `0` as the one-key escape back
  to fit. Two rules for the same question is worse than the rule being
  occasionally inconvenient.
- **Distinguish a drop from an IPC hand-off** — carry the zoom for drops, refit
  for a path handed over by another process. Needs either a flag on
  `UserEvent::Open` or a second handler, which is the duplication 0015 rejected;
  and reuse mode is compile-time off (`REUSE_RUNNING_WINDOW_ON_FILE_OPEN`), so
  the distinction would be dormant code serving a hypothetical difference.
  Rejected.

## Decision

We will leave `fit_mode` untouched in the `UserEvent::Open` handler, so a
dropped file lands exactly like a ←/→ step: refit when the window is at fit,
otherwise carry the literal zoom and pan onto it (clamped to the new image), as
0008 specifies. Everything else 0015 decided — the drop opens in this window,
through this handler, with non-images ignored — stands.

A file *launch* is unaffected: it still opens a new window, fit to it
([ADR 0007](0007-open-file-launches-in-new-windows.md)).

## Consequences

- A detail being inspected at a chosen zoom survives a drop, so files dragged in
  one after another are compared at the same magnification — the reason the rule
  exists for ←/→ in the first place.
- Dropping a much larger image while zoomed in shows a corner of it at that
  zoom, with no hint of what is off-screen; `0` refits. This is the trade 0008
  accepted, now paid on one more gesture.
- The IPC hand-off inherits the behavior, since it shares the handler. It is
  compile-time off today; were it re-enabled, a hand-off would keep the
  receiving window's zoom rather than refit.
- Still not matched to a browse step: the frame blanks for the moment between
  the drop and the new decode, because `Open` clears the caches and repaints,
  whereas a browse miss keeps the previous frame on screen. Left as is — the
  caches are keyed by index into the very list a drop replaces.
- Verified on the built app through the same handler, reached over the IPC pipe
  because reuse mode is the only other way in: with the window at fit on a
  400×300 image, a 6000×4000 file lands at its own 22% fit; with 100% set by `1`
  first, the same file lands at 100%.
