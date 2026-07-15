# 0012. Delete to the Recycle Bin on Del, without a confirmation prompt

Status: Accepted

## Context

Culling a folder of images is a core viewer workflow: look, judge, drop,
move on. Until now the viewer could only browse, so discarding a shot meant
leaving for Explorer and losing your place.

Two questions had to be answered.

**Where does the file go?** A permanent `DeleteFile` is fast and dependency-
free, but an image viewer is exactly where a misjudged keypress is
unrecoverable and expensive. The shell's Recycle Bin is the undo users
already expect from Explorer.

**Does Del confirm first?** Alternatives considered:

- **Confirm every delete.** What XnView MP and IrfanView do by default, and
  it is the safe answer for a permanent delete. But it makes the modal the
  thing you interact with rather than the images, and culling degrades into
  Del-Enter-Del-Enter. Against a Recycle Bin the prompt guards a step that
  is already reversible.
- **No confirmation.** One keypress, one file gone, the next one on screen.
  The Recycle Bin is the safety net, and it is a better one than a prompt:
  it survives a wrong answer to the dialog too.
- **Undo inside the viewer.** Strictly nicer, but it means owning an undo
  stack and reconciling it with the folder watcher, for a case the shell
  already handles.

The remaining hazard is that `FOF_ALLOWUNDO` is best-effort: on a network
share, or with the bin disabled, the shell *permanently* deletes instead —
silently, at which point "no confirmation" would be indefensible.

## Decision

We will delete the current file to the Recycle Bin on Del via shell
`SHFileOperationW` with `FOF_ALLOWUNDO`, and pass `FOF_NOCONFIRMATION` so
the routine case is a single keypress — but also `FOF_WANTNUKEWARNING`, so
that when the shell cannot recycle and would destroy the file instead, the
user is still asked. Shell error UI is left enabled, so a failure (locked or
read-only file) explains itself instead of looking like a dead key.

Dropping the prompt means nothing throttles the key, so Del will ignore OS
auto-repeat (`KeyEvent::repeat`) and act on real presses only. The arrows
repeat because browsing is cheap to undo; a key that recycles a file at
30 Hz from one held finger is not.

After a successful delete we re-emit `UserEvent::FolderChanged`, reusing the
watcher's rebuild path (ADR 0010) rather than mutating the list in place:
it already remaps the cache by path and lands `current` on the next file, or
the previous one when the deleted file was last, or an empty viewport when
the folder is empty — which is exactly the required behavior. Re-emitting it
directly avoids waiting out the 200 ms debounce; the watcher's own event
arrives later and no-ops on an unchanged list.

## Consequences

- Culling is one keypress per image, and mistakes are recoverable from the
  Recycle Bin — including the ones a confirmation dialog would not have
  caught. Holding Del deletes one file, not a run of them.
- Deletion behavior is defined in exactly one place. A file removed from
  Explorer and a file removed with Del now converge on the same code path.
- Reusing the rebuild exposed a latent bug in it: it assumed the replacement
  image would arrive with a `Decoded` event and redraw the window, but a
  prefetched neighbor is already cached and emits no event. Del hits that
  case almost every time (prefetch is the point), so the rebuild now redraws
  when the file on screen changed. This also fixes deleting the on-screen
  file from Explorer, which left the stale image up.
- We give up an in-app undo. Restoring means Explorer's Recycle Bin, and the
  restored file reappears via the watcher rather than the viewer jumping
  back to it.
- `SHFileOperationW` is a legacy API superseded by `IFileOperation`, and it
  cannot take `\\?\` paths, so Del inherits the `MAX_PATH` limit. It was
  chosen over the COM interface because it is a single call with no
  apartment setup, matching how the rest of the Win32 surface here is bound.
