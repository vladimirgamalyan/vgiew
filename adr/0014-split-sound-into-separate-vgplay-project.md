# 0014. Split sound playback into a separate vgplay project

Status: Accepted

Supersedes: 0009 (play sound in the same binary, dispatched by file type)

## Context

ADR 0009 decided to keep sound playback inside `vgiew.exe`, dispatched by
file type at launch, because both paths shared ~80% of the infrastructure
(window, CPU present, no-flash reveal, install/associations, folder
navigation) and audio added no measurable startup cost. That reasoning still
holds for *startup cost*, but the project owner has decided to make the sound
player a product of its own rather than a mode of the image viewer.

Forces that changed the balance:

- **Product identity.** `vgiew` should be, and be described as, a pure image
  viewer. Bundling a sound player under an image-viewer name, icon, and
  ProgID blurs what the app is. Two focused apps read more clearly than one
  that dispatches by type.
- **The shared surface turned out small.** In practice only a handful of
  platform helpers are truly common — the DWM-cloak reveal (ADR 0003), the
  class-background backstop, console attach, icon loading, `absolutize`, and
  the assoc-changed notify. Folder navigation, geometry persistence, the
  folder watcher, and Recycle-Bin delete are image-only; the hand-drawn
  button UI (ADR 0013) is sound-only. Duplicating the few shared helpers into
  a second project is cheaper than ADR 0009 assumed the split would be.
- **Independent evolution.** The sound player is expected to grow its own UI
  (progress bar, time, volume — `concept.md` §6). Keeping it separate lets it
  evolve without touching the image path, and lets `vgiew` drop the `rodio`
  (→ `cpal`/`symphonia`) link entirely, shrinking its binary.

Alternatives considered: a cargo workspace in this repo with a shared `core`
crate (one repo, two binaries), or a second binary target in the same crate.
Both were rejected in favor of two fully independent repositories, so each
app has its own name, icon, associations, install script, and release cadence,
and `vgiew`'s dependency tree carries no audio stack at all.

## Decision

We will split sound playback out of `vgiew` into a separate, standalone
project **vgplay** with its own repository, binary, icon, `--register`/
`--unregister` (ProgID `vgplay.sound` for `wav`/`mp3`/`flac`/`ogg`), and
install/uninstall scripts. `vgiew` becomes a pure image viewer: the
`SOUND_EXTS`/`is_sound` dispatch, the `run_sound` path, the hand-drawn player
UI, and the `rodio` dependency are removed from it.

## Consequences

- `vgiew` no longer plays sound and no longer links `rodio`; its binary is
  smaller and its identity is a single-purpose image viewer.
- The few shared platform helpers are duplicated into `vgplay` rather than
  factored into a shared crate. Acceptable: they are small and stable, and two
  independent repos avoid a workspace's coupling. If they start to diverge or
  drift, revisit extracting a shared crate.
- Sound-file associations move to `vgplay` (its own ProgID and install flow);
  `vgiew --register` registers images only, as before.
- ADR 0013 (hand-drawn play/stop and repeat controls) now describes behavior
  that lives in `vgplay`. It remains valid as the history of *why* those
  controls are drawn the way they are; vgplay carries that decision forward.
- Two artifacts to build, version, and keep associated instead of one — the
  cost ADR 0009 wanted to avoid. Accepted deliberately in exchange for two
  focused products.
