# 0025. Wait for the delegated viewer's window and activate it ourselves

Status: Accepted

Supersedes [ADR 0022](0022-shift-at-launch-hands-the-file-to-another-viewer.md)
point 3 (launch and return immediately) and the no-waiting consequence of
[ADR 0024](0024-x-hands-the-image-on-screen-to-the-external-viewer.md).

## Context

Both hand-off gestures — `Shift` at launch (ADR 0022) and `X` in the window
(ADR 0024) — start the viewer and return at once, passing it the right to come
forward with `AllowSetForegroundWindow(child.id())`. That call was added
precisely because the viewer opened behind Explorer, and it was verified only
indirectly: from a terminal the viewer came forward both before and after the
change, because the chain of foreground rights differs there.

The new information is that it does not work where it matters. Handing an image
over with `X`, on top of an Explorer window, the XnView MP window appears
**behind Explorer** — the exact defect the earlier change was supposed to fix.

Why it cannot work as written: the granted right is spent on the first
foreground attempt and is dropped once anything else is activated. vgiew exiting
*is* that activation — the system brings forward whatever was behind us, Explorer
or another window — and it happens long before the viewer is ready. Measured on
this machine, XnView MP puts up its window **200–230 ms** after launch (cold and
warm alike, a single visible top-level window, no splash), while vgiew is gone in
a few milliseconds. So the viewer reaches for a right that no longer exists.

What we do have is the right itself, for as long as we are alive: as the
foreground process, when `X` was pressed in our own window, and as a process the
foreground Explorer started, on a `Shift`-launch. Either one makes
`SetForegroundWindow` on someone else's window legal. It only has to be called
while the viewer's window exists — that is, ~200 ms later.

Alternatives considered:

- **Leave it.** The gesture works but lands the viewer behind another window,
  which the user sees every time. ADR 0024 traded this away for simplicity,
  before knowing it was a certainty rather than a risk.
- **Wait on a background thread**, so the event loop keeps running. Needs a new
  user event to exit by, a flag to ignore further keys while a hand-off is in
  flight, and the invisible state where vgiew is still showing an image it has
  already given away. At the measured 200 ms a blocking wait is not perceptible,
  and Windows does not mark a window unresponsive before 5 s, so the cap can stay
  well under that.
- **Hide our window first, then wait.** Hiding the foreground window activates
  the next one, which is the very loss of rights this ADR exists to avoid.
- **`SetWindowPos(HWND_TOP)` instead of activating.** Needs no rights, but raises
  the window without focus: the viewer would be visible while the keyboard still
  belonged to Explorer.
- **Find the window by executable rather than by the child's pid.** Only needed
  for a single-instance viewer, which hands the file to a running copy and exits.
  XnView MP is not one — a second launch was measured to make its own process and
  window — so enumerating processes to match an image path is machinery for a
  case we do not have.

## Decision

We will stay alive until the viewer has a visible window, make that window the
foreground one ourselves, and only then exit.

1. **Spawn and grant as before.** `AllowSetForegroundWindow(child.id())` stays:
   it costs nothing and still covers windows the viewer opens for itself later.
2. **Poll for the window.** `EnumWindows` every 25 ms for the first visible
   top-level window owned by the child's pid, then `SetForegroundWindow` on it.
3. **Give up early if there is nothing to activate** — the child exited without a
   window, which is what a single-instance viewer does after handing the file to
   its running copy.
4. **Cap the wait at 3 s**, about thirteen times the measured need, so a viewer
   that never shows a window cannot keep vgiew alive.
5. **One path for both gestures**, since both already share
   `delegate_to_external_viewer`.

## Consequences

- The viewer comes up in front, with the keyboard, in the case the user actually
  performs — over Explorer, and over the vgiew window that is about to close.
- **`X` now blocks the event loop for as long as the viewer takes** (~200 ms
  measured). The image stays on screen and no input is worth handling at that
  point, and the 3 s cap keeps it far below the 5 s that would earn a
  "Not responding" title.
- The gap ADR 0024 accepted — our window gone, the viewer's not yet up — is gone
  with it: the two now overlap instead.
- A **single-instance viewer** still lands wherever it lands. We detect that its
  process left without a window and stop, rather than pausing for the full cap,
  but bringing the surviving copy forward would mean finding a window by
  executable path, which point 5 of the alternatives above rules out until
  something needs it.
- A viewer slower than 3 s falls back to exactly the old behaviour, silently.
- Verified on the built release, keys posted to the window: with XnView MP as
  `ExternalViewer`, its window appeared at 205 ms and vgiew exited at 227 ms —
  i.e. vgiew now outlives the window it has to activate, where before it was gone
  first; with `ExternalViewer` pointed at a windowless executable
  (`rundll32.exe`), vgiew exited after 45 ms instead of sitting out the 3 s cap.
  **The activation itself is not machine-verifiable here:** the harness cannot
  take the foreground, since it belongs to the user's own window, and vgiew must
  be the foreground process for the call to be permitted — the same condition
  that makes the gesture work in real use. That part is confirmed by the user.
