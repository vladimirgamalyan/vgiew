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
  is still decoding.

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
  they do with `←` / `→`. Files that are not images (and folders) are ignored — the
  current image stays on screen.
- **Window position and size are remembered** between runs.
- **File associations:** `install.ps1` registers vgiew so a double-click in Explorer opens
  it (see the README for setup).

## Supported formats

JPG, PNG, GIF (first frame), BMP, WEBP, SVG, SVGZ. The format is detected by file content,
not by extension.

SVG can be dropped at build time (`cargo build --release --no-default-features`), which takes
the binary from ~3.5 MB to ~1.6 MB. Such a build does not recognize `.svg` at all.

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
| `Ctrl+C` | copy image to clipboard (as file **and** as pixels) |
| `Del` | move image to the Recycle Bin, show the next |
| `F` / `Enter` | toggle fullscreen |
| `Esc` | exit fullscreen / close |
