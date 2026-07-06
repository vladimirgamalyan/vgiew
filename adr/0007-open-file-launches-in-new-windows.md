# 0007. Open file launches in new windows by default

Status: Accepted

## Context

ADR 0006 made Windows file launches single-instance: a second
`vgiew.exe "<path>"` connected to the running viewer over a named pipe, handed
it the path, and exited. That preserved one active window but made a normal
double-click on another image replace the currently viewed image.

The current product preference is the opposite: each file launch should create
its own viewer window. This better supports side-by-side comparison and avoids
surprising replacement of an image the user intentionally left open.

The named-pipe IPC from ADR 0006 is already implemented and may be useful
again if the preferred behavior changes back. Removing it would save a small
amount of dormant code but would make that reversal more expensive.

## Decision

We will open file launches in new windows by default.

The reuse-window IPC code remains in the codebase, but both call sites are
gated by `REUSE_RUNNING_WINDOW_ON_FILE_OPEN`, which is currently `false`.
Re-enabling the previous behavior requires changing that constant to `true`;
the named-pipe server and client path are otherwise kept intact.

ADR 0006 is superseded by this decision.

## Consequences

- Double-clicking another image while vgiew is already running opens a second
  viewer window.
- Side-by-side image comparison works naturally.
- No background named-pipe server thread is started in the default mode.
- The old single-instance behavior remains available for a future reversal,
  but the dormant IPC code still needs to compile and be maintained.
