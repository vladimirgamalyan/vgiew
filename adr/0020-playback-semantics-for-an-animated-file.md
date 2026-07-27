# 0020. Pause, frame step, `Ctrl+C` and the title for an animated file

Status: Accepted (point 3 superseded by 0023 for a dropped file)

## Context

ADR 0019 makes an animation something the viewer plays. Four questions then have no answer
inherited from any earlier decision: whether playback is controllable at all, what `Ctrl+C`
copies while frames are changing, what the title reports, and what carries across a browse
step.

Three precedents bear on this and none of them settles it:

- **ADR 0013** gave the sound player play/stop and repeat controls. That is weaker support
  than it looks: sound has nothing on screen, so without controls it cannot be inspected at
  all, whereas an image is already fully visible. It also no longer lives in this binary
  (ADR 0014).
- **ADR 0017 point 4** decided `Ctrl+C` on an SVG rasterizes at the document size rather than
  the current zoom, "so paste quality is independent of how the wheel happened to be turned".
  Read across, that argues for always copying the first frame, so a paste does not depend on
  *when* the key was pressed.
- **ADR 0008** and **ADR 0018** established that view state persists across a browse step or
  a drop rather than resetting: the zoom is carried, not discarded.

The `Ctrl+C` question turns out to depend on the controls question, which is why both are
decided here. Without a pause, the frame on screen at the moment of a keypress is genuinely
arbitrary — the user cannot aim, so ADR 0017's protective reasoning applies in full. With a
pause, the frame on screen is a deliberate selection, and there is no accident left to
protect against.

**Key availability.** `Space` is unbound today; `concept.md` once floated it as an alternative
for browsing, but the key handler only implements `←`/`→` for that. `←`/`→` are therefore
unavailable for stepping frames, as are `0`, `1`, `f`, `Enter`, `Esc`, `Delete` and `Ctrl+C`.

**A backward step was considered and rejected.** Stepping forward is what the decoder already
produces in order, so it is nearly free. Stepping backward from the trailing edge of the ring
buffer requires re-decoding from the start of the file, because frames are cumulative — an
unbounded stall triggered by a single keypress, and the worst latency behaviour anywhere in
the design, bought for the rarest interaction in it.

**Title updates are not free.** `set_title` on Windows is a synchronous message to the window
procedure. A live frame counter means dozens of those per second, for text that is unreadable
precisely because it is changing. A counter shown only while paused costs one call when
pausing and one per step.

## Decision

We will make playback controllable with two keys and let the paused frame be what the rest of
the viewer acts on:

1. **`Space` toggles pause.** On pause the timer is disarmed, so a paused animation costs
   exactly as much as a static image: nothing.
2. **`.` steps one frame forward**, and there is no backward step. Stepping past the last
   frame wraps, which is the same mechanism as an ordinary loop wrap (ADR 0019 point 5) and
   therefore introduces no new behaviour — including the re-decode stall when the animation
   did not fit the buffer.
3. **Pause carries across a browse step**, exactly as the zoom does under ADR 0008 and a drop
   does under ADR 0018. A user who stopped to inspect one frame is in an inspecting mood; the
   alternative — every new file starts playing — would fight that on every keypress.
4. **Returning to a file restarts it from frame 0.** This is not a preference but a
   consequence of ADR 0019 points 9 and 10: a neighbour holds only its first frame, and the
   decoder is torn down when a file stops being current, so there is no position left to
   resume from.
5. **`Ctrl+C` copies the frame on screen.** With `Space` and `.` available, that frame is
   something the user chose, so this makes the pair into a usable extract-a-frame tool. This
   knowingly diverges from ADR 0017 point 4: copying during playback is nondeterministic.
   Accepted, because the deterministic alternative would make pause useless for the one task
   it is most obviously wanted for, and the nondeterminism is confined to the case where the
   user did not bother to aim.
6. **The title always reports the frame count, and adds the current index while paused** —
   `[500×500, 48 frames]` playing, `[500×500, frame 12/48]` paused. The count is set at the
   points that already rebuild the title; the index changes only on pause and on step, so
   there is no per-frame `set_title`.
7. **No on-screen playback indicator.** ADR 0013 hand-drew controls for sound because a sound
   file puts nothing on screen; an image occupies the whole window and the state belongs in
   the title, where the zoom percentage already lives. Drawing a widget would mean a
   compositing layer the CPU path does not have.

## Consequences

- **`Space` becomes format-dependent**: it does nothing on a JPG. That is mild, but it also
  forecloses `Space` as a browse key, which `concept.md` had sketched and which some viewers
  use.
- **A carried pause can read as a broken viewer.** Someone who paused a GIF minutes ago,
  browsed through photos and arrived at another GIF finds it motionless. The title's frame
  index is the only cue.
- **In fullscreen there is no title bar, so the paused state is invisible** — exactly where a
  user is most likely to be inspecting a frame closely. This is the weakest point of decision
  6 and the most likely reason to revisit this ADR; an overlay is the obvious remedy and
  point 7 is what currently rules it out.
- **`Ctrl+C` is no longer deterministic for every format.** A user who copies without pausing
  gets whatever frame was showing, and two attempts can differ. The behaviour has to be
  documented per format in `docs/FEATURES.md`, where SVG already needs its own note.
- **Overshooting by one frame costs a full loop.** With no backward step, the only way back is
  round the whole animation — and on a file that exceeds the buffer, that round trip includes
  the re-decode stall.
- **Browsing away loses your position.** Stepping to an interesting frame, arrowing to the
  next file to compare, and arrowing back starts the animation over.
- **A paused animation still holds its ring buffer.** Pause stops the timer and the decoder,
  not the memory; up to 256 MB stays resident for as long as the file is current.
