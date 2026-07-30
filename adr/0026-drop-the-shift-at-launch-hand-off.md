# 0026. Drop the Shift-at-launch hand-off; `X` is the only one

Status: Accepted

Supersedes [ADR 0022](0022-shift-at-launch-hands-the-file-to-another-viewer.md).

## Context

ADR 0022 made a `Shift`-held double-click in Explorer hand the file to
`ExternalViewer` without vgiew ever opening a window. ADR 0024 then added `X`,
which does the same for the image already on screen. Two gestures now reach the
same viewer through the same code, and the launch one is the weaker of the two on
every count that ADR 0022 itself recorded:

- **It is a race the user has to win.** Explorer passes no modifier state to the
  program it launches, so the key is read with `GetAsyncKeyState` a few tens of
  milliseconds after the click. Releasing `Shift` too early opens the image in
  vgiew instead — a coin flip written into the gesture, and something ADR 0022
  listed as unfixable rather than as a defect.
- **It asks for the decision at the wrong time.** Which tool a file wants is
  usually clear only once the image is visible. `X` costs one keystroke after
  looking, with no timing to get right, which is why it was asked for.
- **`Shift`+double-click also extends Explorer's selection**, so the gesture
  leaves the folder in a state the user did not ask for.

Alternatives considered:

- **Keep both.** They do not conflict, and the code is already written. Rejected:
  the launch path is a keyboard read plus a branch in front of every single
  startup, guarding a gesture that is unreliable by construction and now
  redundant. Two ways in also means two ways to document and two to explain when
  one of them silently loses its race.
- **Keep the launch gesture and move it to `Ctrl`.** Trades one unwinnable race
  for another; the modifier was never the problem.

## Decision

We will remove the startup modifier check and keep `X` as the only hand-off.

1. **Delete `shift_held()` and its call** in `main`, so nothing reads the keyboard
   before the window is built and the startup path loses a branch.
2. **Keep everything else**: the `ExternalViewer` value, its deletion by
   `--unregister`, `delegate_to_external_viewer` and the window activation of
   [ADR 0025](0025-wait-for-the-delegated-viewer-window-and-activate-it.md).
   `X` is unchanged.

## Consequences

- One gesture, no timing to get right, and the choice of viewer is made with the
  image in front of the user.
- **A double-click can no longer bypass vgiew.** Sending a file straight to the
  other viewer without opening it here now costs Explorer's "Open with", the same
  as before ADR 0022 — or `X` immediately after, which opens vgiew for the few
  hundred milliseconds it takes to hand the file on.
- `ExternalViewer` keeps its meaning and its name; a machine that already has it
  set needs no change, and only the way the value is reached is gone.
- ADR 0022's reasoning stays on record for why the target is a registry value at
  all — XnView MP registers no `App Paths` key and is not on `PATH` — which is
  still the reason `X` works the way it does.
- Verified on the built release: with `ExternalViewer` set and Shift held over
  the launch (synthetic, held across the whole startup), vgiew opens its own
  window and starts nothing else, while `X` in that window hands the image over
  as before.
