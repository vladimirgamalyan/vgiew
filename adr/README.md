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
  size, and zoom centering (window sizing superseded by 0005)
- [0005](0005-persist-window-position-and-size.md) — Persist and restore window
  position and size
