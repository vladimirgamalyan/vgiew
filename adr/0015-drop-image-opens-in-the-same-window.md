# 0015. A dropped image opens in the same window

Status: Accepted

## Context

vgiew accepts an image path on the command line, and ADR 0007 decided that
such a launch (a double-click in Explorer) gets its own window. Dragging an
image onto an already-open viewer is a different gesture: unlike a
double-click, it names the window it is aimed at, so the user has already said
where the image should appear.

winit 0.29 registers an OLE drop target for every window by default
(`drag_and_drop: true`), so the viewer already receives
`WindowEvent::DroppedFile(path)` — one event per file, with absolute paths —
without any builder flag or Win32 code of our own.

Alternatives considered:

- **Spawn a new window, exactly like a double-click.** Consistent with ADR
  0007, but it ignores the target the user pointed at, and it pays process
  startup for a viewer that is already running. Rejected.
- **Accept any dropped file and let the decoder reject it.** Simpler on paper,
  but a dropped `.txt` would clear the current frame, swap the sibling list to
  its folder, and leave the title stuck on a file that will never decode. The
  existing `is_image` extension filter avoids that for the price of one
  condition. Rejected.
- **Duplicate the open-a-new-file logic in the drop handler.** The folder
  list, watcher re-point, and cache reset already live in the
  `UserEvent::Open` handler written for the ADR 0006 IPC path. Duplicating it
  would give two places to keep in sync. Rejected in favor of forwarding the
  path through `UserEvent::Open`.

## Decision

We will open an image dropped on the window *in that same window*, by
forwarding the dropped path to the existing `UserEvent::Open` handler. Drops
that are not images by extension (`IMAGE_EXTS`) are ignored, leaving the
current frame untouched.

This does not change ADR 0007: a file *launch* still opens a new window. The
two gestures differ on purpose, because a drop identifies its target window
and a launch does not.

## Consequences

- Dropping an image replaces what the window shows, rebuilds the sibling list
  from the dropped file's folder, re-points the folder watcher, and refits the
  zoom — the same state swap a hand-off over the IPC pipe performs.
- The `UserEvent::Open` path is now live in the default configuration, so it
  is no longer dormant code kept only for a possible reversal of ADR 0007.
- Unsupported files and folders are ignored silently, with no error popup or
  flash. This matches the viewer's feedback-free style elsewhere (see ADR
  0012), and the price is that a rejected drop is indistinguishable from a
  drop the window never received.
- The filter is by extension, not content, so an image with the wrong
  extension is refused while a mislabeled non-image is accepted and then fails
  to decode like any other unreadable file. This is the same rule the sibling
  list already applies.
- Dropping several files at once delivers one event per file, so the last
  image in the batch wins after a few redundant folder scans and decodes. Not
  worth batching for a gesture whose natural use is a single image.
