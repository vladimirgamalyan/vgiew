# Architecture Decision Records

This folder records notable architecture and design decisions for
`vgiew`, together with the reasoning behind them. It exists so that a
decision — especially a debatable or previously-revisited one — gets made
once, on record, instead of being silently re-litigated in every session.

## When to write one

Add a new ADR for a decision that is:

- **Architecturally significant** — affects the rendering/graphics path,
  window and interaction behavior, file-type associations, or how a whole
  feature area works.
- **Debatable** — there was a real alternative, and someone (human or agent)
  could reasonably propose reopening it later.
- **Costly to reverse** — changing course later means touching multiple
  files, breaking the CLI contract, or redoing prior work.

Skip an ADR for routine bug fixes, refactors with no behavioral choice to
record, or anything already fully explained by `concept.md`.

## Before proposing a debatable change

Before proposing or re-proposing a decision that feels debatable, check
`adr/` first:

1. Search existing ADRs for the topic (filenames and titles).
2. If a relevant ADR exists and is `Accepted`, treat it as settled. Follow
   it. Only propose reopening it if you have new information the ADR did
   not consider — and say explicitly what that new information is.
3. If you do reopen it, do not edit the old ADR's decision in place. Write a
   new ADR that supersedes it (see below), so the history of *why* stays
   intact.

## File format

- Filename: `NNNN-short-kebab-title.md`, numbered sequentially
  (`0001-...`, `0002-...`).
- Use `template.md` as the starting point for a new record.
- Status is one of: `Proposed`, `Accepted`, `Rejected`, `Superseded by
  NNNN`. When a decision changes, add a new ADR and update the old one's
  status to `Superseded by NNNN` rather than rewriting it.

## Index

- [0001](0001-record-architecture-decisions.md) — Record architecture
  decisions as ADRs
- [0002](0002-render-on-cpu-softbuffer.md) — Render on the CPU (softbuffer),
  not the GPU
- [0003](0003-reveal-window-via-dwm-cloak.md) — Reveal the window via DWM cloak
  to avoid a startup flash
- [0004](0004-open-scale-window-size-and-zoom-centering.md) — Open scale, window
  size, and zoom centering (window sizing superseded by 0005; zoom-out floor and
  browse-refit superseded by 0008)
- [0005](0005-persist-window-position-and-size.md) — Persist and restore window
  position and size
- [0006](0006-single-instance-reuse-window.md) — Single instance: reuse the open
  window for a new file (superseded by 0007)
- [0007](0007-open-file-launches-in-new-windows.md) — Open file launches in new
  windows by default
- [0008](0008-keep-zoom-while-browsing-and-allow-sub-fit-zoom.md) — Keep zoom while
  browsing, and allow zoom below fit
- [0009](0009-sound-in-same-binary-dispatched-by-type.md) — Play sound in the same
  binary, dispatched by file type (superseded by 0014)
- [0010](0010-watch-folder-for-live-sibling-list.md) — Keep the sibling list live
  via a folder watcher
- [0011](0011-pixel-grid-at-high-zoom.md) — Draw a pixel grid at high zoom
- [0012](0012-delete-to-recycle-bin-without-confirmation.md) — Delete to the
  Recycle Bin on Del, without a confirmation prompt
- [0013](0013-sound-player-controls-play-stop-repeat.md) — Sound player controls:
  hand-drawn play/stop and repeat toggle (now lives in vgplay, see 0014)
- [0014](0014-split-sound-into-separate-vgplay-project.md) — Split sound playback
  into a separate vgplay project (supersedes 0009)
