# 0022. Shift held at launch hands the file to another viewer

Status: Accepted

## Context

vgiew is registered as the default handler for images, so an ordinary
double-click in Explorer opens it, and a launch means a new window
([ADR 0007](0007-open-file-launches-in-new-windows.md)). That is the right
default — it is the whole point of the project — but the same file sometimes
wants a heavier tool instead: XnView MP for metadata, conversion or batch work.
Today that costs a right-click, "Open with", and picking from the menu, every
time.

The request is to make the choice at the moment of the double-click: hold a
modifier and the file goes straight to the other viewer, with vgiew not opening
a window at all.

The forces:

- **Explorer does not tell a launched process which modifiers were held.** No
  argument, no environment variable, no verb. The only way to know is to read
  the physical keyboard state as the process starts (`GetAsyncKeyState`), which
  makes the gesture inherently a race: the key must still be down when vgiew
  runs, a few tens of milliseconds after the click.
- **Alt is unusable.** Explorer claims Alt+double-click for "Properties" and
  never launches the handler at all. That leaves Shift and Ctrl; Shift is the
  more conventional "same action, different target" modifier.
- **XnView MP cannot be found by name.** Checked on the target machine: it
  registers no `App Paths\xnviewmp.exe` key (neither HKLM nor HKCU) and puts
  nothing on `PATH`, so `ShellExecute("xnviewmp.exe", …)` — the zero-config
  route — fails with "file not found". The path has to come from somewhere.

Alternatives considered:

- **A gesture inside the vgiew window** (a modified double-click on the image,
  or a hotkey, sending the current file onward). Rejected as a substitute: it
  does not answer the request, because vgiew still starts, builds a window and
  decodes the file before anything is delegated. It remains a reasonable
  *addition* later — "send the image I am looking at to the other viewer" is a
  different feature from "do not open this one here".
- **Hardcode `%ProgramFiles%\XnViewMP\xnviewmp.exe`.** Zero configuration and
  correct on this machine. Rejected: it writes a third-party product into our
  source, and breaks on a portable or relocated install.
- **Auto-detect by scanning the uninstall keys** for a `DisplayName` matching
  XnView and taking its `DisplayIcon`. Rejected: enumerating GUID subkeys and
  matching a product name by substring is the longest and most fragile option
  on offer, in service of what one string value settles.
- **`ShellExecuteEx` with the `openas` verb** — the system "Open with" dialog.
  Universal and needs no configuration, but it puts a dialog and an extra click
  inside a gesture whose whole point is going straight to the other viewer.
  Rejected.
- **A settings file** next to the executable or in `%APPDATA%`. Rejected for
  the reason [ADR 0005](0005-persist-window-position-and-size.md) gives: HKCU
  is already the one place this app keeps its settings, and this is a single
  string.

## Decision

We will check once at startup, before any window exists, whether Shift is
physically down, and if it is, hand the file to a configured external viewer
and exit.

1. **Trigger.** `GetAsyncKeyState(VK_SHIFT)`, bit `0x8000` (down right now),
   read immediately after the arguments are parsed — before the reuse-mode IPC
   hand-off and before the window is built, so a running vgiew cannot swallow a
   file meant for the other viewer, and nothing flashes on screen.
2. **Target.** `HKCU\Software\vgiew`, value `ExternalViewer` = the full path to
   an executable. Absent or empty means the feature is off, which is the
   default state: nothing is written at install time. The key is already
   deleted by `--unregister`, so uninstall still leaves nothing behind.
3. **Launch.** `std::process::Command` with the absolute image path as the sole
   argument, then return from `main`. Not `ShellExecuteW`: a full path to an
   .exe needs neither `App Paths` resolution nor association lookup, so the
   standard library is enough.
4. **Fallback.** Any failure — no value configured, or the process could not be
   started — falls through to opening the file in vgiew as usual. The gesture
   is never a no-op.
5. **No error reporting**, consistent with how the viewer handles the rest of
   its failures ([ADR 0012](0012-delete-to-recycle-bin-without-confirmation.md)):
   an unusable path simply means the image opens here.

## Consequences

- The user picks the viewer at double-click time, with no menu, and vgiew stays
  the fast default for the common case.
- **The key must be held until the other viewer starts, not merely during the
  click.** Releasing Shift immediately can beat vgiew to the keyboard read and
  open the image here instead. This cannot be fixed from our side — Explorer
  passes no modifier state — so it is a property of the gesture, not a bug to
  file.
- Shift+double-click also extends Explorer's selection to that file. Harmless,
  but it is why the selection sometimes looks odd afterwards.
- The feature needs one manual registry value before it does anything.
  `install.ps1` deliberately does not write it: an installer that silently
  points at another vendor's product would be a surprise, and the value is one
  line to set.
- Nothing is XnView-specific in the code — any executable that accepts a file
  path works, and a second machine can point it somewhere else entirely.
- A modified launch still starts a vgiew process; it just exits in a few
  milliseconds without creating a window. Explorer shows no window flash.
- Sending the *currently displayed* file onward is still not possible; only the
  launch is covered. Left out of scope on purpose (see the rejected in-window
  gesture above).
- Verified on the built release, driven by synthetic Shift held over the
  launch: with `ExternalViewer` set, vgiew exits with code 0 and no window
  while XnView MP opens the file; with Shift held and no value configured, the
  image opens in vgiew; without the modifier, the ordinary window opens and
  XnView MP is never started.
