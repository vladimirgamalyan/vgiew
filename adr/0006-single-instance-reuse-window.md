# 0006. Single instance: reuse the open window for a new file

Status: Accepted

## Context

vgiew is registered as a per-extension file handler, so opening an image
launches `vgiew.exe "<path>"`. Until now every launch created its own window.
Double-clicking a second image while a viewer was already open left the user
with two windows, when the expected behavior is for the already-open viewer to
switch to the new image (the way Windows Photos or most single-instance
viewers behave).

We need a second launch to (a) detect that a viewer is already running, (b)
hand it the file path, and (c) bring that window to the front — then exit
without creating a window of its own.

Two sub-problems: how the second process talks to the first, and how the first
process safely adopts a file from a *different* folder while background decode
threads for the old folder may still be in flight.

Alternatives considered for the IPC channel:

- **`WM_COPYDATA` to the existing window.** The canonical Win32 way, but the
  receiver must handle the message in its window procedure. winit owns the
  `WndProc`; intercepting it means subclassing (`SetWindowLongPtr` /
  `SetWindowSubclass`), which is fragile against winit internals. Rejected.
- **Named mutex for election + a separate transport.** A mutex cleanly elects
  the primary and `Local\` scopes it per-session, but it only answers "is one
  running?" — we still need a transport for the path, so it adds a primitive
  without removing one. The pipe already answers both "is one running?" (open
  succeeds) and "here is the path" (write). Rejected as redundant.
- **A localhost TCP socket.** Works, but opens a listening port (firewall
  prompts, port collisions) for what is a local, same-session handoff.
  Rejected.

Alternatives considered for stale in-flight decodes after a folder swap:

- **A generation counter** threaded through `ensure_decode`/`prefetch`/
  `spawn_decode` and both result events. Correct, but churns several function
  signatures. Rejected in favor of the lighter path-echo below.

## Decision

We will make vgiew single-instance per desktop session on Windows, using a
named pipe, and guard cached decode results by path.

1. **Transport = named pipe, per session.** The primary instance runs a
   blocking named-pipe server on a background thread at
   `\\.\pipe\vgiew-<session-id>` (session id from
   `ProcessIdToSessionId`, so separate sessions on one machine stay
   independent). A later launch opens that pipe as a client
   (`std::fs::OpenOptions::write`) and writes the file path.

2. **Detect by connecting.** If a launch has a file argument, it tries to open
   the pipe. Success ⇒ a viewer is running: write the path and exit. Failure
   (no server) ⇒ this process is the primary; it starts the server and opens a
   window as before. There is no separate election primitive.

3. **Path bytes are sent losslessly.** The client sends
   `OsStr::as_encoded_bytes`; the server reconstructs with
   `OsStr::from_encoded_bytes_unchecked` (sound because both ends are the same
   binary). A relative CLI path is made absolute first (`absolutize`, no
   filesystem touch, no `\\?\` prefix) so it resolves against the launcher's
   cwd, not the primary's.

4. **Server hands the path to the event loop** as a new `UserEvent::Open(path)`
   — reusing the existing `EventLoopProxy` channel already used by decode
   threads. The handler rebuilds the folder's sibling list, resets
   `current`, clears the decode cache/inflight/failed sets, kicks off the new
   decode, and raises the window (`set_minimized(false)` + `focus_window`).
   The launching process calls `AllowSetForegroundWindow(ASFW_ANY)` before
   sending so that foreground raise is permitted.

5. **Guard stale results by path, not a generation counter.** Both
   `UserEvent::Decoded` and `UserEvent::Failed` now carry the source `path`.
   On receipt we drop the result unless `files[idx] == path`. After an `Open`
   swaps folders, a late decode from the previous folder — whose index now
   points at a different file — fails the check and is discarded, so it can
   never be shown or cached under the wrong image. In normal same-folder
   browsing the check always holds, so behavior is unchanged.

## Consequences

- Opening an image while a viewer is already running reuses that window and
  brings it to the front, instead of spawning a second window. This is the
  common flow (vgiew is a file handler), so it is the default from now on.
- A running viewer keeps one detached pipe-server thread for the process
  lifetime, blocked in `ConnectNamedPipe`; the OS reclaims it on exit, like
  the existing detached decode threads.
- The feature is Windows-only. The IPC helpers are `#[cfg(windows)]` and the
  call sites are gated; `UserEvent::Open` and the path guard are
  cross-platform and simply never fire elsewhere. On another OS, or if the
  pipe cannot be opened, a launch falls back to opening its own window — the
  prior behavior.
- Deliberately *not* single-instance for a launch with **no** file argument: a
  bare `vgiew` still opens its own window. This keeps the change scoped to the
  file-open flow and avoids a focus-only handshake; the trade-off is that two
  no-argument launches yield two windows (and two servers), after which
  forwarding is nondeterministic. This is an unusual manual action for a file
  handler and is accepted.
- Cold-start race: two file launches within the sub-second window before the
  first server is listening could both become primary (two windows). For a
  double-click-a-second-image flow the first instance is already running, so
  this is not the real path; worst case degrades to today's behavior.
