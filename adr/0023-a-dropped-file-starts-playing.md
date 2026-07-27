# 0023. A dropped file starts playing, even when the previous one was paused

Status: Accepted (supersedes point 3 of
[0020](0020-playback-semantics-for-an-animated-file.md) for a dropped file)

## Context

[ADR 0020 point 3](0020-playback-semantics-for-an-animated-file.md) made the
pause carry across a browse step, "exactly as the zoom does under ADR 0008 and a
drop does under ADR 0018" — treating a `←`/`→` step and a drop as the same
event, because [ADR 0018](0018-dropped-image-lands-like-a-browse-step.md) had
just established that equivalence for the zoom.

In use, that combination reads as a broken viewer: pause one GIF, drag the next
one onto the window, and the new file sits motionless. ADR 0020 anticipated the
complaint in its consequences ("a carried pause can read as a broken viewer")
but weighed it only for browsing, where the pause was the whole point.

The new information the earlier decisions did not weigh is that **a drop and an
arrow key differ in intent**, even though both put a different image in the
window:

- `←`/`→` walk a list the viewer built. The user is comparing neighbours, and
  0020's "inspecting mood" argument holds — the pause is what makes frame-by-
  frame comparison across files possible at all.
- A drop is a file the user picked out and dragged in, usually to see what it
  does. Nothing about that gesture says "keep everything as it was".

The zoom is not a good analogy here either. A carried zoom is visible the
instant the image appears, so a user who did not want it can see it and press
`0`. A carried pause is *invisible*: a still first frame looks exactly like a
static image, and the only cue is the frame index in the title — which
fullscreen does not show at all (0020's own weakest point).

Alternatives considered:

- **Leave 0020 point 3 as it is.** One rule for every way of changing the file
  is easier to state. Rejected: the rule exists to serve an inspecting mood,
  and dragging in a different file ends that mood rather than continuing it.
- **Clear the pause on `←`/`→` too**, restoring a single rule in the other
  direction ("a new file always plays"). Rejected: it would break the real task
  0020 point 3 was written for — stepping two files to the same frame and
  comparing them — and that task has no other mechanism.
- **Clear the pause only when the dropped file comes from another folder.**
  Rejected: behaviour that depends on where a file happens to live cannot be
  predicted from the gesture.
- **Keep the pause but show it on screen** (an overlay instead of a behaviour
  change), so a carried pause stops looking like a defect. Rejected here: 0020
  point 7 rules out a playback widget, an overlay needs a compositing layer the
  CPU path does not have, and it is a separate decision that would not answer
  the question this one asks.

## Decision

We will clear `paused` in the `UserEvent::Open` handler, so a file that arrives
by drop plays from frame 0 regardless of the playback state it replaces.

1. **A drop clears the pause.** The rest of what a drop does is unchanged: the
   zoom and pan still carry as [0018](0018-dropped-image-lands-like-a-browse-step.md)
   specifies.
2. **`←`/`→` still carry the pause**, so 0020 point 3 stands for a browse step;
   only its extension to drops is withdrawn.
3. **The reuse-mode hand-off inherits this**, because it shares the handler — a
   path handed over by another process is likewise a file the user just reached
   for. That mode is compile-time off today (`REUSE_RUNNING_WINDOW_ON_FILE_OPEN`).
4. **Nothing is conditional on the file being animated.** `paused` means nothing
   to a still image, so clearing it unconditionally needs no format check.

## Consequences

- A GIF dragged onto the window is seen moving, which is what dragging it in
  was for.
- Comparing the same frame of two files by dropping them in turn now costs a
  `Space` per file. `←`/`→` remain the way to do that without re-pausing —
  which is also where the pause is most useful, since a browse list is what one
  compares along.
- A drop is no longer exactly a browse step: view state (zoom, pan) carries,
  playback state does not. The split is defensible — one is how the image is
  shown, the other is whether it is running — but it is a second rule to hold in
  mind, where 0018 had left one.
- Nothing changes for a still image, and no new state is tracked: the whole
  change is one assignment in the handler.
- Verified on the built release, driven over the IPC pipe because reuse mode is
  the only other way into this handler (the same route ADR 0018 was verified
  by): with `anim-basic.gif` paused at `frame 19/24`, `→` lands on
  `anim-disposal.gif` at `frame 1/12` — still paused — while handing over
  `anim.webp` shows it as `[320×320, 20 frames]`, i.e. playing, and `Space`
  then pauses it at `frame 9/20`.
