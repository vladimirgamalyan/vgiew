# 0010. Keep the sibling list live via a folder watcher

Status: Accepted

## Context

The sibling list (`files`) was built once at startup (and on reuse-mode
`Open`). Files added to the folder after launch never appeared while browsing
with ←/→, and deleted files stayed in the list until a decode failed. The
requirement: added/removed files should be picked up, without hurting
browsing responsiveness.

Alternatives considered:

- **Rescan on every arrow key.** Simplest, but puts `read_dir` + a per-entry
  `is_file()` stat + sort on the hot navigation path. Warm NTFS caches make it
  a few ms, but cold caches, network shares, or folders with thousands of
  files turn keypresses laggy — the exact opposite of the project's
  responsiveness goal. Also does nothing for files deleted while idle.
- **Time-based lazy rescan (rescan if older than N seconds).** Still pays the
  scan on a keypress (just less often), and picks an arbitrary staleness
  window.
- **Rescan on window focus.** Free and covers the alt-tab-to-Explorer flow,
  but misses files produced by background processes (batch exports, camera
  imports) while the viewer stays focused.
- **Folder watcher (`ReadDirectoryChangesW`).** The list is refreshed *before*
  the user presses anything; keypresses never touch the filesystem. Costs a
  dependency (`notify`) and a background thread.

## Decision

We will watch the opened file's folder with the `notify` crate
(non-recursive), debounce events in a helper thread (coalesce a burst until
the folder has been quiet for 200 ms), and deliver a single
`UserEvent::FolderChanged` to the event loop, which rescans and rebuilds the
sibling list.

Because the cache, in-flight set, and failed set are keyed by index, and
indices shift when entries appear or vanish, the rebuild carries state over
**by path**: decoded images are remapped to their new indices, the current
position follows the current file's path (falling back to the clamped index
if it was deleted), and in-flight decodes are kept only where the index→path
mapping is unchanged. `failed` is cleared so transient failures (a file first
seen mid-copy) retry after the folder settles.

## Consequences

- Arrow-key navigation never scans the filesystem; the list is already fresh
  when the key is pressed. Added images show up, deleted ones drop out; if
  the on-screen image is deleted, the viewer falls back to its neighbor
  automatically.
- The quiet-period debounce doubles as write-completion detection: a file
  being copied in is rescanned only after the copy pauses/finishes, so we
  rarely decode half-written files — and `failed.clear()` on the next change
  recovers when we do.
- New dependency: `notify` (wraps `ReadDirectoryChangesW` on Windows; the
  watcher is created after window show, so startup is unaffected).
- A folder with a continuously-written file (e.g. an active download) can
  postpone rescans until it goes quiet for 200 ms. Accepted as an exotic case
  for image folders; a max-coalesce cap can be added if it ever bites.
- Watch failures degrade silently to the old static-list behavior.
