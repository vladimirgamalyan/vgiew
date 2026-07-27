# vgiew — Features

A reference of what the viewer does. The guiding goal is **instant startup and instant
switching**: everything below is built so that opening and browsing images stays fast.

## Viewing

- **Instant display.** The window and first frame appear immediately; decoding runs on a
  background thread, so there is no splash and no wait on double-click.
- **Fit to window** with letterboxing over a neutral dark background.
- **Transparency** is composited over that background.
- **Crisp zoom:** nearest-neighbor when zooming in (sharp pixel edges), bilinear when
  zooming out (no aliasing). A vector document is instead re-rendered for the current zoom,
  so it has no pixels to sharpen — see [SVG](#svg).
- **Pixel grid at high zoom:** past a threshold zoom, a 1px grid marks the boundary between
  image pixels. The line tint adapts to the underlying color so it stays visible. Not drawn
  for vector documents, which have no image pixels.
- **Informative title:** `vgiew — name [W×H] size zoom%`, and `(loading…)` while a frame
  is still decoding. An animation adds its length — see [Animation](#animation).

## Navigation

- **`←` / `→`** move to the previous / next image in the same folder.
- **Natural sort order** (`file2` before `file10`), case-insensitive.
- **Zoom and pan are kept while browsing** — only images opened at fit re-fit; otherwise the
  current zoom carries onto the next image.
- **Neighbors are prefetched**, so switching is instant once they are decoded.
- **Live folder watching:** adding, removing, or renaming files in the folder updates the
  list automatically, without reopening.

## Zoom and pan

- **Mouse wheel** zooms to the point under the cursor.
- **Left-drag** pans.
- **`0`** fits the image to the window; **`1`** shows it at 100% (1:1).
- Zoom-out goes below fit, down to 1%.

## Copy to clipboard (`Ctrl+C`)

Pressing **`Ctrl+C`** puts the current image on the clipboard in **two formats at once**, so
it pastes correctly wherever you go:

- **As a file (`CF_HDROP`)** — exactly what Explorer's `Ctrl+C` produces. Paste into a
  folder (or any app that accepts files) to copy the image file itself.
- **As pixels (`CF_DIBV5`)** — paste into an image editor (Photoshop, GIMP, Paint, …) to
  drop the image straight onto the canvas, no intermediate file.

Details:

- **Transparency is preserved** — the bitmap carries an explicit alpha channel.
- Windows synthesizes `CF_DIB` / `CF_BITMAP` from the `CF_DIBV5` for apps that only read
  those older formats, so essentially any image-aware app can paste it.
- **No visual feedback**, by design — nothing flashes or pops up.
- If the frame is still decoding (a brief moment right after opening a large image), only
  the file is placed on the clipboard.
- **What gets copied depends on the format.** A still image copies itself; an
  [animation](#animation) copies the frame on screen, so pausing first is how you aim; an
  [SVG](#svg) copies at the size the document declares, whatever the current zoom.
- This runs **only on the key press**. It adds no work to opening or switching images, so it
  does not affect display or navigation speed.

## File management

- **`Del`** moves the current image to the **Recycle Bin** (no confirmation) and shows the
  next one. Auto-repeat is ignored, so holding the key cannot delete a run of images.

## Fullscreen

- **`F`** / **`Enter`** toggle borderless fullscreen.
- **`Esc`** exits fullscreen, or closes the window when not in fullscreen.

## Window and system integration

- **Single instance:** opening another image reuses the already-running window instead of
  spawning a second process, which keeps subsequent opens fast.
- **Drag & drop:** dropping an image on the window opens it there, like a double-click
  would, and switches browsing to that image's folder. Zoom and pan carry over just as
  they do with `←` / `→`; a paused animation does not — the dropped file plays. Files that
  are not images (and folders) are ignored — the current image stays on screen.
- **Window position and size are remembered** between runs.
- **File associations:** `install.ps1` registers vgiew so a double-click in Explorer opens
  it (see the README for setup).
- **Hand a file to another viewer:** holding **`Shift`** over that double-click starts vgiew
  only long enough to pass the file on — no window is created here. The viewer is named by
  the registry value `ExternalViewer` under `HKCU\Software\vgiew` (a full path to an `.exe`,
  e.g. `C:\Program Files\XnViewMP\xnviewmp.exe`); until it is set, the modifier does
  nothing. Keep `Shift` down until the other viewer appears — Explorer does not pass the key
  state to the program it launches, so vgiew reads the keyboard as it starts. If the viewer
  cannot be started, the image opens in vgiew as usual.

## Supported formats

JPG, PNG, GIF, BMP, WEBP, SVG, SVGZ. The format is detected by file content, not by
extension. GIF, APNG and WebP **animate** — see [Animation](#animation).

SVG can be dropped at build time (`cargo build --release --no-default-features`), which takes
the binary from ~3.5 MB to ~1.6 MB. Such a build does not recognize `.svg` at all.

## Animation

Animated **GIF**, **APNG** and **WebP** play on open. A file with a single frame is an
ordinary still and costs nothing extra — no timer, no decoder, no memory.

- **`Space`** pauses and resumes; **`.`** steps one frame forward. There is no step back:
  frames in these formats are cumulative, so an earlier one can only be reached by playing
  round again.
- **Pause carries while browsing**, exactly as the zoom does. Arrowing onto another
  animation finds it paused too; the title is the cue. **A dropped file is the exception**
  and always starts playing — a file dragged onto the window is one you want to see move.
- **The title reports the length** — `[500×500, 48 frames]` — and while paused, the frame
  you are looking at: `[500×500, frame 12/48]`.
- **`Ctrl+C` copies the frame on screen**, so `Space` + `.` + `Ctrl+C` is how you extract a
  single frame. Copying without pausing gets whatever frame happened to be showing.
- **Returning to a file starts it over.** Browsing away releases the decoder, so there is no
  position left to resume from.
- **Timing follows the file**, measured against one fixed clock so a long animation cannot
  drift. A delay of 0 — which authors write for "as fast as possible" — plays at 100 ms, the
  same normalisation every browser applies.
- **Playback stops while the window is minimized**, and resumes from the frame it stopped on.
- **Memory is bounded at 256 MB of decoded frames.** An ordinary animation fits entirely and
  loops for free; a very large one (a 1080p screencast, say) streams instead, and will
  visibly hitch once per loop where it has to start reading the file again.
- Transparency, the zoom filtering and the pixel grid all work exactly as they do for a still
  image — an animation frame is real image pixels.

## SVG

Vector documents are **re-rendered for the current zoom** rather than scaled from a fixed
bitmap, so they stay sharp however far you zoom in — and only the visible part is rendered, so
memory does not grow with the zoom level.

- **Zoom stays responsive.** A fresh render runs in the background while the previous one is
  stretched to stand in for it, so a complex drawing goes briefly soft and then snaps sharp
  instead of freezing. Ordinary files render within a frame and never show the soft step.
- **Opening size follows the file.** An SVG that states its size (`width="24"`) is shown at
  that size, capped at 100% like any image; one that states none (`width="100%"`) is fitted to
  the window, enlarging past 100% where a raster would not — there is no blurring to avoid.
- **`Ctrl+C`** copies the image at the size the document declares, so a paste does not depend
  on the zoom you happened to be at.
- **No pixel grid**, and no nearest-neighbour step at high zoom: both mark out image pixels,
  and a re-rendered vector has none.
- **Text uses installed fonts.** A font the document asks for but the system lacks is
  substituted, so text can differ from what its author saw. Loading the font database is
  skipped entirely for documents without text, which is most of them.
- **Not supported:** SMIL/CSS animation, scripting, `<foreignObject>`; filters only in part.
  Exports from Figma, Illustrator and Inkscape render faithfully; animated web assets do not.

## Hotkey reference

| Key | Action |
|-----|--------|
| `←` / `→` | previous / next image |
| Mouse wheel | zoom to cursor |
| Left-drag | pan |
| `0` | fit to window |
| `1` | 100% (1:1) |
| `Space` | pause / resume an animation (nothing on a still image) |
| `.` | step one frame forward in an animation |
| `Ctrl+C` | copy image to clipboard (as file **and** as pixels) |
| `Del` | move image to the Recycle Bin, show the next |
| `F` / `Enter` | toggle fullscreen |
| `Esc` | exit fullscreen / close |
