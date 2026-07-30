# 0024. X hands the image on screen to the external viewer

Status: Accepted (the immediate-exit consequence superseded by 0025: we wait for
the viewer's window and activate it)

## Context

[ADR 0022](0022-shift-at-launch-hands-the-file-to-another-viewer.md) put the
choice of viewer at the moment of the double-click: hold `Shift` and the file
goes to `ExternalViewer` instead of opening here. It explicitly left the
in-window half out of scope, and named it as a reasonable addition later —
"send the image I am looking at to the other viewer" is a different feature from
"do not open this one here". That is what is being asked for now: the decision to
reach for XnView MP is usually made *after* seeing the image, and today it costs
closing vgiew and finding the file again in Explorer.

The forces:

- **The key has to be read by position, not by character.** The requirement is
  that it works whatever the active layout is. Under the Russian layout the same
  physical key produces `ч`, so matching the logical character would leave the
  gesture dead exactly where the user asked for it to work. `Ctrl+C` already has
  this problem and already solves it by matching `PhysicalKey::Code`.
- **The action is consequential** — it starts another program and closes this
  window — so it should be hard to hit by accident, and it must not fire twice.
- **`X` is otherwise unused**, both as a bare key and under `Ctrl`.

Alternatives considered:

- **Match the logical key** (`Key::Character("x")`). One line shorter and wrong:
  it fails on every non-Latin layout.
- **Match the logical key and also accept `ч`.** Fixes Russian and nothing else;
  the physical match covers every layout at once and needs no list.
- **Keep the window open after handing the file on.** Rejected: the request is
  to hand the *work* over, and two viewers showing the same image is the state
  the gesture exists to leave. It would also read differently from the
  `Shift`-launch, where vgiew is never seen at all.
- **Put it on `Ctrl+X`** (or on a modified click). `Ctrl+X` is conventionally
  "cut" — an operation this viewer could plausibly grow, given it already
  implements `Ctrl+C` — so it stays free. A bare `X` is also what was asked for.
- **A second registry value**, so the key and the launch could point at different
  programs. Speculative: nothing asked for two targets, and one setting is the
  whole configuration surface of the feature today.

## Decision

We will hand the image currently on screen to the same configured viewer on a
bare `X`, and close vgiew once it has started.

1. **Trigger.** `key.physical_key == PhysicalKey::Code(KeyCode::KeyX)` with no
   modifiers held and `key.repeat` false. Position, not character, so the layout
   is irrelevant; bare press, so a future `Ctrl+X` cannot collide; no repeat, so
   a held key cannot spawn a second viewer while the first is starting.
2. **Target.** The same `HKCU\Software\vgiew` value `ExternalViewer` and the same
   `delegate_to_external_viewer` used by the `Shift`-launch, including its
   `AllowSetForegroundWindow` call so the viewer comes forward rather than opening
   behind whatever is left on screen. One setting covers both gestures.
3. **Close only on success.** No value configured, or the process could not be
   started, means the key does nothing and the window stays. Closing on a
   hand-off that never happened would throw the image away.
4. **No error reporting**, as everywhere else in this viewer
   ([ADR 0012](0012-delete-to-recycle-bin-without-confirmation.md)): an unusable
   path simply means the key appears to do nothing.

## Consequences

- The gesture no longer has to be decided before the image is visible, and the
  `Shift`-launch race described in ADR 0022 — key released too early, image opens
  here — now has a one-key recovery instead of costing a round trip through
  Explorer.
- **`X` is spent.** No other single-key function can claim it, and a user who
  wanted `X` for something else has to reopen this decision.
- Only the path is handed over. Zoom, pan, the paused frame of an animation —
  none of it carries; the other viewer opens the file from scratch. Carrying view
  state is not possible through a command line and was not asked for.
- vgiew disappears immediately while the other viewer takes a second or more to
  show a window, so there is a visible gap with neither on screen. Waiting for
  the other window before closing ours would mean polling another process, which
  is far more machinery than the gap is worth.
- With no `ExternalViewer` configured — the default state — the key does nothing
  at all, and there is no message saying why. Same trade as ADR 0022: the
  feature needs its one registry value before it exists.
- Verified on the built release, with keys posted to the window (this harness
  cannot take the foreground) and XnView MP as `ExternalViewer`: with the
  **Russian layout active** in vgiew's thread, `X` opened `a.png` in XnView MP
  and vgiew exited with code 0; with `Ctrl` genuinely held down, the same key
  left vgiew running and started nothing, and a bare `X` immediately afterwards
  handed the file off as before.
