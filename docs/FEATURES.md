# vgiew — Features

A reference of what the viewer does. The guiding goal is **instant startup and instant
switching**: everything below is built so that opening and browsing images stays fast.

## Viewing

- **Instant display.** The window and first frame appear immediately; decoding runs on a
  background thread, so there is no splash and no wait on double-click.
- **Fit to window** with letterboxing over a neutral dark background.
- **Transparency** is composited over that background.
- **Crisp zoom:** nearest-neighbor when zooming in (sharp pixel edges), bilinear when
  zooming out (no aliasing).
- **Pixel grid at high zoom:** past a threshold zoom, a 1px grid marks the boundary between
  image pixels. The line tint adapts to the underlying color so it stays visible.
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
- **Window position and size are remembered** between runs.
- **File associations:** `install.ps1` registers vgiew so a double-click in Explorer opens
  it (see the README for setup).

## Supported formats

JPG, PNG, GIF (first frame), BMP, WEBP. The format is detected by file content, not by
extension.

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
