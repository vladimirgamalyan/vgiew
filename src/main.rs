// vgiew — a fast image viewer (MVP, Tier C path: winit + softbuffer + CPU).
// Core architecture: the window shows immediately, decoding runs in the background,
// resampling is multithreaded (rayon). Browse ←/→ through images in the folder.
// Neighboring images are prefetched so browsing is instant.
#![windows_subsystem = "windows"]

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rayon::prelude::*;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{EventLoop, EventLoopBuilder, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Icon, WindowBuilder};

const BG: u32 = 0x00F5_F5F5; // viewport background (softbuffer: 0x00RRGGBB)

// Absolute zoom-out floor (1%). Zoom-out is no longer floored at fit, so a below-fit
// zoom can be carried across images while browsing (XnView-style); `0` refits.
const MIN_SCALE: f32 = 0.01;

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "webp"];
const SOUND_EXTS: &[&str] = &["wav"];
#[cfg(windows)]
const REUSE_RUNNING_WINDOW_ON_FILE_OPEN: bool = false;

struct DecodedImage {
    w: u32,
    h: u32,
    px: Vec<u32>, // 0xAARRGGBB
}

enum UserEvent {
    Decoded { path: PathBuf, idx: usize, img: DecodedImage },
    Failed { path: PathBuf, idx: usize },
    // Optional reuse mode: another process handed this window a file to open.
    Open(PathBuf),
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn is_sound(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SOUND_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

// Natural sort (file2 before file10), case-insensitive.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let mut na = String::new();
                    while let Some(c) = ai.peek().copied() {
                        if c.is_ascii_digit() {
                            na.push(c);
                            ai.next();
                        } else {
                            break;
                        }
                    }
                    let mut nb = String::new();
                    while let Some(c) = bi.peek().copied() {
                        if c.is_ascii_digit() {
                            nb.push(c);
                            bi.next();
                        } else {
                            break;
                        }
                    }
                    let va = na.trim_start_matches('0');
                    let vb = nb.trim_start_matches('0');
                    let ord = va.len().cmp(&vb.len()).then_with(|| va.cmp(vb));
                    if ord != Ordering::Equal {
                        return ord;
                    }
                } else {
                    let la = ca.to_ascii_lowercase();
                    let lb = cb.to_ascii_lowercase();
                    if la != lb {
                        return la.cmp(&lb);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

// Builds the list of images in the opened file's folder and the current index.
fn build_siblings(path: &Path) -> (Vec<PathBuf>, usize) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut files: Vec<PathBuf> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image(p))
        .collect();
    files.sort_by(|a, b| {
        let na = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let nb = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
        natural_cmp(na, nb)
    });
    let cur = files.iter().position(|p| p == path).unwrap_or(0);
    (files, cur)
}

// Decode with format detection by content (magic bytes), not by extension.
fn load_rgba(path: &Path) -> Option<image::RgbaImage> {
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    Some(reader.decode().ok()?.to_rgba8())
}

fn pack_rgba(rgba: &image::RgbaImage) -> DecodedImage {
    let (w, h) = rgba.dimensions();
    // Parallel RGBA8 -> 0xAARRGGBB; the collect is order-preserving.
    let px: Vec<u32> = rgba
        .as_raw()
        .par_chunks_exact(4)
        .map(|c| ((c[3] as u32) << 24) | ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32))
        .collect();
    DecodedImage { w, h, px }
}

fn load_app_icon() -> Option<Icon> {
    let rgba = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()?
        .to_rgba8();
    let (w, h) = rgba.dimensions();
    Icon::from_rgba(rgba.into_raw(), w, h).ok()
}

fn spawn_decode(path: PathBuf, idx: usize, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || match load_rgba(&path) {
        Some(rgba) => {
            let _ = proxy.send_event(UserEvent::Decoded {
                path,
                idx,
                img: pack_rgba(&rgba),
            });
        }
        None => {
            let _ = proxy.send_event(UserEvent::Failed { path, idx });
        }
    });
}

// The neighbors (prev, next) of an index, wrapping around the folder.
fn neighbors(current: usize, len: usize) -> (usize, usize) {
    if len <= 1 {
        return (current, current);
    }
    ((current + len - 1) % len, (current + 1) % len)
}

// Start a background decode for `idx` unless it's already cached, in flight, or failed.
fn ensure_decode(
    idx: usize,
    files: &[PathBuf],
    cache: &HashMap<usize, DecodedImage>,
    inflight: &mut HashSet<usize>,
    failed: &HashSet<usize>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    if idx >= files.len()
        || cache.contains_key(&idx)
        || inflight.contains(&idx)
        || failed.contains(&idx)
    {
        return;
    }
    inflight.insert(idx);
    spawn_decode(files[idx].clone(), idx, proxy.clone());
}

// Prefetch the neighbors of the current image so browsing is instant.
fn prefetch(
    current: usize,
    files: &[PathBuf],
    cache: &HashMap<usize, DecodedImage>,
    inflight: &mut HashSet<usize>,
    failed: &HashSet<usize>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    let (prev, next) = neighbors(current, files.len());
    ensure_decode(next, files, cache, inflight, failed, proxy);
    ensure_decode(prev, files, cache, inflight, failed, proxy);
}

// Keep only {prev, current, next} in the cache to bound memory (big images are ~w*h*4 bytes).
fn evict(cache: &mut HashMap<usize, DecodedImage>, current: usize, len: usize) {
    let (prev, next) = neighbors(current, len);
    cache.retain(|&k, _| k == current || k == prev || k == next);
}

// Composites a pixel (r,g,b channels + alpha 0..1) over a background color → 0x00RRGGBB.
#[inline(always)]
fn composite(r: f32, g: f32, b: f32, a: f32, br: f32, bg: f32, bb: f32) -> u32 {
    let r = (br + (r - br) * a) as u32;
    let g = (bg + (g - bg) * a) as u32;
    let b = (bb + (b - bb) * a) as u32;
    (r << 16) | (g << 8) | b
}

// Checkerboard color (screen-space, 8px cells) shown behind transparent pixels.
#[inline(always)]
fn checker(dx: usize, dy: usize) -> (f32, f32, f32) {
    if (((dx >> 3) + (dy >> 3)) & 1) == 0 {
        (255.0, 255.0, 255.0) // XnView MP checkerColor1
    } else {
        (240.0, 240.0, 240.0) // XnView MP checkerColor2
    }
}

// Nearest neighbor: crisp pixel edges when zooming in (scale >= 1), no blur.
#[inline(always)]
fn sample_nearest(img: &DecodedImage, sx: f32, sy: f32, br: f32, bg: f32, bb: f32) -> u32 {
    if sx < 0.0 || sy < 0.0 || sx >= img.w as f32 || sy >= img.h as f32 {
        return BG;
    }
    let p = img.px[sy as usize * img.w as usize + sx as usize];
    let a = ((p >> 24) & 0xFF) as f32 / 255.0;
    composite(
        ((p >> 16) & 0xFF) as f32,
        ((p >> 8) & 0xFF) as f32,
        (p & 0xFF) as f32,
        a,
        br,
        bg,
        bb,
    )
}

// Bilinear filtering: smoothing when zooming out (scale < 1), no aliasing.
#[inline(always)]
fn sample(img: &DecodedImage, sx: f32, sy: f32, br: f32, bg: f32, bb: f32) -> u32 {
    if sx < 0.0 || sy < 0.0 || sx > (img.w - 1) as f32 || sy > (img.h - 1) as f32 {
        return BG;
    }
    let x0 = sx as u32;
    let y0 = sy as u32;
    let x1 = (x0 + 1).min(img.w - 1);
    let y1 = (y0 + 1).min(img.h - 1);
    let tx = sx - x0 as f32;
    let ty = sy - y0 as f32;
    let w = img.w as usize;
    let p00 = img.px[y0 as usize * w + x0 as usize];
    let p10 = img.px[y0 as usize * w + x1 as usize];
    let p01 = img.px[y1 as usize * w + x0 as usize];
    let p11 = img.px[y1 as usize * w + x1 as usize];
    let ch = |p: u32, s: u32| ((p >> s) & 0xFF) as f32;
    let bl = |a: f32, b: f32, c: f32, d: f32| {
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        top + (bot - top) * ty
    };
    let a = bl(ch(p00, 24), ch(p10, 24), ch(p01, 24), ch(p11, 24)) / 255.0;
    let r = bl(ch(p00, 16), ch(p10, 16), ch(p01, 16), ch(p11, 16));
    let g = bl(ch(p00, 8), ch(p10, 8), ch(p01, 8), ch(p11, 8));
    let b = bl(ch(p00, 0), ch(p10, 0), ch(p01, 0), ch(p11, 0));
    composite(r, g, b, a, br, bg, bb)
}

fn draw(img: Option<&DecodedImage>, buf: &mut [u32], ww: u32, wh: u32, scale: f32, cx: f32, cy: f32) {
    match img {
        None => buf.iter_mut().for_each(|p| *p = BG),
        Some(im) => {
            let ww_f = ww as f32;
            let wh_f = wh as f32;
            // Zoom in (scale >= 1) — nearest (crisp pixels); zoom out — bilinear.
            let nearest = scale >= 1.0;
            buf.par_chunks_mut(ww as usize)
                .enumerate()
                .for_each(|(dy, row)| {
                    let sy = cy + (dy as f32 - wh_f / 2.0) / scale;
                    for (dx, px) in row.iter_mut().enumerate() {
                        let sx = cx + (dx as f32 - ww_f / 2.0) / scale;
                        let (br, bg, bb) = checker(dx, dy);
                        *px = if nearest {
                            sample_nearest(im, sx, sy, br, bg, bb)
                        } else {
                            sample(im, sx, sy, br, bg, bb)
                        };
                    }
                });
        }
    }
}

fn fit_scale(iw: u32, ih: u32, ww: u32, wh: u32) -> f32 {
    (ww as f32 / iw as f32).min(wh as f32 / ih as f32)
}

// Shrink-to-fit: downscale images larger than the window, but never enlarge a
// small image past 100%. This is the on-open / reset scale and the zoom-out floor.
fn view_fit(iw: u32, ih: u32, ww: u32, wh: u32) -> f32 {
    fit_scale(iw, ih, ww, wh).min(1.0)
}

// Keep the view within bounds after a zoom or pan. `cx`/`cy` is the image point at
// the window center; the visible span on an axis is window/scale image pixels wide.
// Per axis, independently: if the image is smaller than the window on that axis,
// lock it to center; otherwise clamp so neither edge pulls inside the window (no
// gap). Running this after every zoom/pan is what re-centers a smaller-than-window
// image on zoom-out — there is no separate re-center path.
fn clamp_center(cx: &mut f32, cy: &mut f32, scale: f32, iw: u32, ih: u32, ww: f32, wh: f32) {
    if iw as f32 * scale <= ww {
        *cx = iw as f32 / 2.0;
    } else {
        let half = (ww / 2.0) / scale;
        *cx = cx.clamp(half, iw as f32 - half);
    }
    if ih as f32 * scale <= wh {
        *cy = ih as f32 / 2.0;
    } else {
        let half = (wh / 2.0) / scale;
        *cy = cy.clamp(half, ih as f32 - half);
    }
}

// ── Window geometry persistence (position + size across runs) ──
// Stored in HKCU\Software\vgiew as "x,y,w,h": the outer position (screen pixels) and
// the inner/client size (physical pixels). On the next run we restore it if it parses
// and still fits the current monitor layout; otherwise Windows assigns the default.

// Parse "x,y,w,h". Returns None on any malformed/corrupt input (missing field,
// non-integer, extra field, or a non-positive size).
#[cfg(windows)]
fn parse_geometry(s: &str) -> Option<(i32, i32, u32, u32)> {
    let mut it = s.split(',');
    let x: i32 = it.next()?.trim().parse().ok()?;
    let y: i32 = it.next()?.trim().parse().ok()?;
    let w: u32 = it.next()?.trim().parse().ok()?;
    let h: u32 = it.next()?.trim().parse().ok()?;
    if it.next().is_some() || w == 0 || h == 0 {
        return None;
    }
    Some((x, y, w, h))
}

#[cfg(windows)]
fn load_window_geometry() -> Option<(i32, i32, u32, u32)> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\vgiew")
        .ok()?;
    let s: String = key.get_value("WindowGeometry").ok()?;
    parse_geometry(&s)
}

#[cfg(windows)]
fn save_window_geometry(x: i32, y: i32, w: u32, h: u32) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Software\\vgiew") {
        let _ = key.set_value("WindowGeometry", &format!("{x},{y},{w},{h}"));
    }
}

// The work area (screen minus taskbar) of the monitor a window rect lands on, as
// (left, top, right, bottom). None if the rect is off every monitor — e.g. a monitor
// was removed (MonitorFromRect + MONITOR_DEFAULTTONULL). Used both as the pre-build
// gate ("does the saved window still land somewhere?") and by fit_window_to_screen.
#[cfg(windows)]
fn monitor_work_area(x: i32, y: i32, w: u32, h: u32) -> Option<(i32, i32, i32, i32)> {
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct MonitorInfo {
        cb_size: u32,
        rc_monitor: Rect,
        rc_work: Rect,
        dw_flags: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn MonitorFromRect(lprc: *const Rect, dw_flags: u32) -> *mut core::ffi::c_void;
        fn GetMonitorInfoW(hmonitor: *mut core::ffi::c_void, lpmi: *mut MonitorInfo) -> i32;
    }
    const MONITOR_DEFAULTTONULL: u32 = 0;
    let rect = Rect {
        left: x,
        top: y,
        right: x.saturating_add(w as i32),
        bottom: y.saturating_add(h as i32),
    };
    let mon = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) };
    if mon.is_null() {
        return None;
    }
    let mut mi = MonitorInfo {
        cb_size: core::mem::size_of::<MonitorInfo>() as u32,
        rc_monitor: Rect { left: 0, top: 0, right: 0, bottom: 0 },
        rc_work: Rect { left: 0, top: 0, right: 0, bottom: 0 },
        dw_flags: 0,
    };
    if unsafe { GetMonitorInfoW(mon, &mut mi) } == 0 {
        return None;
    }
    Some((mi.rc_work.left, mi.rc_work.top, mi.rc_work.right, mi.rc_work.bottom))
}

// After the window exists (so its real frame size is known), shrink and nudge it so the
// whole outer window — title bar and borders included — fits the monitor work area. This
// fits a restored window that is now too big or hangs off the edge (a lower-resolution
// monitor) fully on screen. A no-op when it already fits.
#[cfg(windows)]
fn fit_window_to_screen(window: &winit::window::Window) {
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let outer = window.outer_size();
    let inner = window.inner_size();
    let Some((wl, wt, wr, wb)) = monitor_work_area(pos.x, pos.y, outer.width, outer.height) else {
        return;
    };
    let work_w = (wr - wl).max(1) as u32;
    let work_h = (wb - wt).max(1) as u32;
    let frame_w = outer.width.saturating_sub(inner.width);
    let frame_h = outer.height.saturating_sub(inner.height);
    // Cap the outer window at the work area, then set the client size back from it.
    let out_w = outer.width.min(work_w);
    let out_h = outer.height.min(work_h);
    if out_w != outer.width || out_h != outer.height {
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(
            out_w.saturating_sub(frame_w).max(1),
            out_h.saturating_sub(frame_h).max(1),
        ));
    }
    let nx = pos.x.clamp(wl, wr - out_w as i32);
    let ny = pos.y.clamp(wt, wb - out_h as i32);
    if nx != pos.x || ny != pos.y {
        window.set_outer_position(winit::dpi::PhysicalPosition::new(nx, ny));
    }
}

#[cfg(not(windows))]
fn load_window_geometry() -> Option<(i32, i32, u32, u32)> {
    None
}
#[cfg(not(windows))]
fn save_window_geometry(_x: i32, _y: i32, _w: u32, _h: u32) {}
#[cfg(not(windows))]
fn monitor_work_area(_x: i32, _y: i32, _w: u32, _h: u32) -> Option<(i32, i32, i32, i32)> {
    None
}
#[cfg(not(windows))]
fn fit_window_to_screen(_window: &winit::window::Window) {}

// ── Console for CLI subcommands (a windows-subsystem build has no console of its own) ──
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
    }
    unsafe {
        AttachConsole(0xFFFF_FFFF); // ATTACH_PARENT_PROCESS
    }
}
#[cfg(not(windows))]
fn attach_console() {}

#[cfg(windows)]
fn notify_assoc_changed() {
    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(
            w_event_id: i32,
            u_flags: u32,
            dw_item1: *const core::ffi::c_void,
            dw_item2: *const core::ffi::c_void,
        );
    }
    unsafe {
        // SHCNE_ASSOCCHANGED, SHCNF_IDLIST
        SHChangeNotify(0x0800_0000, 0x0000, core::ptr::null(), core::ptr::null());
    }
}

// Cloak/uncloak the window at the DWM level. A cloaked window is composited (its surface
// exists, so a GDI present lands in it) but not displayed. This lets us reveal the window
// only after its first frame is painted, with no white flash on show.
#[cfg(windows)]
fn set_cloak(window: &winit::window::Window, cloak: bool) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut core::ffi::c_void,
            attr: u32,
            value: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    const DWMWA_CLOAK: u32 = 13;
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    let value: i32 = cloak as i32;
    unsafe {
        DwmSetWindowAttribute(
            h.hwnd.get() as *mut core::ffi::c_void,
            DWMWA_CLOAK,
            &value as *const i32 as *const core::ffi::c_void,
            core::mem::size_of::<i32>() as u32,
        );
    }
}
#[cfg(not(windows))]
fn set_cloak(_window: &winit::window::Window, _cloak: bool) {}

// Set the window class background brush to `rgb` (0xRRGGBB). If anything ever erases the
// window before our first paint (e.g. a stray WM_ERASEBKGND), it fills that color instead
// of white — a backstop to the DWM cloak in set_cloak. The brush lives for the process; the
// OS reclaims it on exit.
#[cfg(windows)]
fn set_class_background(window: &winit::window::Window, rgb: u32) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[link(name = "gdi32")]
    extern "system" {
        fn CreateSolidBrush(color: u32) -> *mut core::ffi::c_void;
    }
    #[link(name = "user32")]
    extern "system" {
        fn SetClassLongPtrW(hwnd: *mut core::ffi::c_void, index: i32, value: isize) -> isize;
    }
    const GCLP_HBRBACKGROUND: i32 = -10;
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    // COLORREF is 0x00BBGGRR; swap R and B from our 0xRRGGBB.
    let color = ((rgb & 0xFF) << 16) | (rgb & 0x00FF00) | ((rgb >> 16) & 0xFF);
    unsafe {
        let brush = CreateSolidBrush(color);
        SetClassLongPtrW(
            h.hwnd.get() as *mut core::ffi::c_void,
            GCLP_HBRBACKGROUND,
            brush as isize,
        );
    }
}
#[cfg(not(windows))]
fn set_class_background(_window: &winit::window::Window, _rgb: u32) {}

// ── Optional reuse-window IPC (currently disabled by default) ──
// When enabled, a viewer that is already open adopts a freshly opened image instead
// of spawning a second window. The first instance runs a named-pipe server; a second
// instance connects, writes the file path, and exits. The server forwards the path
// to the event loop as UserEvent::Open. The pipe name is per-session so separate
// desktop sessions of the same machine stay independent.

// The desktop session this process runs in — used to scope the pipe name.
#[cfg(windows)]
fn session_id() -> u32 {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcessId() -> u32;
        fn ProcessIdToSessionId(dw_process_id: u32, p_session_id: *mut u32) -> i32;
    }
    let mut sid = 0u32;
    unsafe {
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut sid) == 0 {
            return 0;
        }
    }
    sid
}

#[cfg(windows)]
fn ipc_pipe_name() -> String {
    format!(r"\\.\pipe\vgiew-{}", session_id())
}

// Absolute path from a possibly-relative CLI arg, without touching the filesystem or
// adding a \\?\ verbatim prefix (which canonicalize would). Explorer already passes
// absolute paths; this covers a relative path typed on the command line.
#[cfg(windows)]
fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

// Grant any process permission to pull its window to the foreground. The launching
// (secondary) process holds foreground rights; it calls this so the running instance's
// SetForegroundWindow (via focus_window) is allowed to succeed.
#[cfg(windows)]
fn allow_foreground_any() {
    #[link(name = "user32")]
    extern "system" {
        fn AllowSetForegroundWindow(dw_process_id: u32) -> i32;
    }
    const ASFW_ANY: u32 = 0xFFFF_FFFF;
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
    }
}

// Try to hand `arg` to an already-running instance. Returns true if a running instance
// accepted it (we should exit); false if none is running (we become the primary).
#[cfg(windows)]
fn forward_to_running_instance(arg: &Path) -> bool {
    use std::io::Write;
    let path = absolutize(arg);
    // Open the server's pipe as a client. If it isn't there, no instance is running.
    match std::fs::OpenOptions::new().write(true).open(ipc_pipe_name()) {
        Ok(mut f) => {
            // Let the running instance raise its window before we send the path.
            allow_foreground_any();
            // Send the OS-native path bytes; the server reconstructs them losslessly.
            f.write_all(path.as_os_str().as_encoded_bytes()).is_ok()
        }
        Err(_) => false,
    }
}

// Primary instance: run the named-pipe server that receives paths from later launches.
#[cfg(windows)]
fn spawn_ipc_server(name: String, proxy: EventLoopProxy<UserEvent>) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt as _;
    type Handle = *mut core::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateNamedPipeW(
            lp_name: *const u16,
            dw_open_mode: u32,
            dw_pipe_mode: u32,
            n_max_instances: u32,
            n_out_buffer_size: u32,
            n_in_buffer_size: u32,
            n_default_time_out: u32,
            lp_security_attributes: *const core::ffi::c_void,
        ) -> Handle;
        fn ConnectNamedPipe(h_named_pipe: Handle, lp_overlapped: *mut core::ffi::c_void) -> i32;
        fn DisconnectNamedPipe(h_named_pipe: Handle) -> i32;
        fn ReadFile(
            h_file: Handle,
            lp_buffer: *mut core::ffi::c_void,
            n_number_of_bytes_to_read: u32,
            lp_number_of_bytes_read: *mut u32,
            lp_overlapped: *mut core::ffi::c_void,
        ) -> i32;
        fn CloseHandle(h_object: Handle) -> i32;
        fn GetLastError() -> u32;
    }
    const PIPE_ACCESS_INBOUND: u32 = 0x0000_0001;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_WAIT: u32 = 0x0000_0000;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const ERROR_PIPE_CONNECTED: u32 = 535;

    let wide: Vec<u16> = OsStr::new(&name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    std::thread::spawn(move || loop {
        let invalid: Handle = usize::MAX as Handle; // INVALID_HANDLE_VALUE (-1)
        let h = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                0,
                64 * 1024,
                0,
                core::ptr::null(),
            )
        };
        if h == invalid {
            break;
        }
        let connected = unsafe { ConnectNamedPipe(h, core::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if connected {
            let mut buf: Vec<u8> = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let mut read = 0u32;
                let ok = unsafe {
                    ReadFile(
                        h,
                        tmp.as_mut_ptr() as *mut core::ffi::c_void,
                        tmp.len() as u32,
                        &mut read,
                        core::ptr::null_mut(),
                    )
                };
                if ok == 0 || read == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..read as usize]);
                if (read as usize) < tmp.len() {
                    break;
                }
            }
            if !buf.is_empty() {
                // The bytes came from OsStr::as_encoded_bytes in the sibling instance,
                // so reconstructing them with from_encoded_bytes_unchecked is sound.
                let os = unsafe { OsStr::from_encoded_bytes_unchecked(&buf) };
                let _ = proxy.send_event(UserEvent::Open(PathBuf::from(os)));
            }
        }
        unsafe {
            DisconnectNamedPipe(h);
            CloseHandle(h);
        }
    });
}

fn print_help() {
    println!(
        "vgiew — a fast image and sound viewer\n\n\
         Usage:\n  \
         vgiew <file>                        open an image or sound\n  \
         vgiew --register                    register associations (HKCU, no admin)\n  \
         vgiew --unregister                  remove associations\n  \
         vgiew --dump <in> <out.png> [W H]   headless frame render\n  \
         vgiew --help                        this help"
    );
}

// Registers vgiew as a candidate handler for images in HKCU (no admin rights).
// The path comes from current_exe() — register the installed .exe.
#[cfg(windows)]
fn register() -> std::io::Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy().replace('/', "\\");
    let cmd = format!("\"{exe_str}\" \"%1\"");
    let classes = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Software\\Classes")?
        .0;

    classes
        .create_subkey("vgiew.image")?
        .0
        .set_value("", &"Image (vgiew)")?;
    classes
        .create_subkey("vgiew.image\\DefaultIcon")?
        .0
        .set_value("", &format!("{exe_str},0"))?;
    classes
        .create_subkey("vgiew.image\\shell\\open\\command")?
        .0
        .set_value("", &cmd)?;

    // Applications\vgiew.exe entry — so the app shows up nicely in "Open with".
    classes
        .create_subkey("Applications\\vgiew.exe\\shell\\open\\command")?
        .0
        .set_value("", &cmd)?;
    classes
        .create_subkey("Applications\\vgiew.exe")?
        .0
        .set_value("FriendlyAppName", &"vgiew")?;
    let supported = classes
        .create_subkey("Applications\\vgiew.exe\\SupportedTypes")?
        .0;

    for ext in IMAGE_EXTS {
        // Add vgiew as a candidate for the extension WITHOUT touching the default (no association hijacking).
        classes
            .create_subkey(format!(".{ext}\\OpenWithProgids"))?
            .0
            .set_value("vgiew.image", &"")?;
        supported.set_value(format!(".{ext}"), &"")?;
    }
    notify_assoc_changed();
    Ok(())
}

#[cfg(windows)]
fn unregister() -> std::io::Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let classes = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Software\\Classes", KEY_ALL_ACCESS)?;
    let _ = classes.delete_subkey_all("vgiew.image");
    let _ = classes.delete_subkey_all("Applications\\vgiew.exe");
    for ext in IMAGE_EXTS {
        if let Ok(owp) =
            classes.open_subkey_with_flags(format!(".{ext}\\OpenWithProgids"), KEY_SET_VALUE)
        {
            let _ = owp.delete_value("vgiew.image");
        }
    }
    // Remove our own settings key (persisted window geometry) so uninstall leaves nothing behind.
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all("Software\\vgiew");
    notify_assoc_changed();
    Ok(())
}

// Headless pipeline check: vgiew --dump <in> <out.png> [W H]
fn dump(args: &[String]) {
    let src = &args[2];
    let out = &args[3];
    let ww: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1280);
    let wh: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(800);
    let t_dec = std::time::Instant::now();
    let rgba = load_rgba(Path::new(src)).expect("decode");
    let dec_ms = t_dec.elapsed().as_secs_f64() * 1000.0;
    let (w, h) = rgba.dimensions();
    let t_pack = std::time::Instant::now();
    let img = pack_rgba(&rgba);
    let pack_ms = t_pack.elapsed().as_secs_f64() * 1000.0;
    let scale = fit_scale(w, h, ww, wh);
    let mut buf = vec![0u32; (ww as usize) * (wh as usize)];
    draw(Some(&img), &mut buf, ww, wh, scale, w as f32 / 2.0, h as f32 / 2.0);
    let mut out_img = image::RgbImage::new(ww, wh);
    for (i, p) in buf.iter().enumerate() {
        let x = (i as u32) % ww;
        let y = (i as u32) / ww;
        out_img.put_pixel(
            x,
            y,
            image::Rgb([((p >> 16) & 0xFF) as u8, ((p >> 8) & 0xFF) as u8, (p & 0xFF) as u8]),
        );
    }
    out_img.save(out).expect("save");
    println!("{w}x{h}: decode {dec_ms:.1} ms + pack {pack_ms:.1} ms  (-> {ww}x{wh}, fit {scale:.4})");
}

// ── Sound path ──────────────────────────────────────────────────────────────
// A sound file opens a minimal window and plays once — no controls, no repeat.
// Audio is initialized lazily here (only when a sound is actually opened), so the
// image path pays nothing for linking rodio: no output device is opened otherwise.
fn run_sound(path: &Path) {
    // Best-effort playback: open the default output, decode, and queue the file.
    // Keep the stream and player alive for the window's lifetime — dropping them
    // stops playback. Any failure (no device, undecodable file) leaves the window
    // up with no sound rather than aborting.
    let _audio = rodio::DeviceSinkBuilder::open_default_sink()
        .ok()
        .map(|stream| {
            let player = rodio::Player::connect_new(stream.mixer());
            if let Ok(file) = std::fs::File::open(path) {
                if let Ok(source) = rodio::Decoder::try_from(file) {
                    player.append(source);
                }
            }
            (stream, player)
        });

    let event_loop = EventLoop::new().unwrap();
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("—");
    let window = Rc::new(
        WindowBuilder::new()
            .with_title(format!("vgiew — {name}"))
            .with_window_icon(load_app_icon())
            .with_inner_size(winit::dpi::LogicalSize::new(420.0, 120.0))
            .with_resizable(false)
            .with_visible(false)
            .build(&event_loop)
            .unwrap(),
    );
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    // No-flash reveal (ADR 0003): cloak, show, paint the neutral background, uncloak.
    // The window is a plain background pane — no widgets yet — while the file plays.
    set_class_background(&window, BG);
    set_cloak(&window, true);
    window.set_visible(true);
    {
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .unwrap();
        let mut buffer = surface.buffer_mut().unwrap();
        buffer.iter_mut().for_each(|p| *p = BG);
        buffer.present().unwrap();
    }
    set_cloak(&window, false);

    event_loop
        .run(move |event, elwt| {
            if let Event::WindowEvent { event, .. } = event {
                match event {
                    WindowEvent::CloseRequested => elwt.exit(),
                    WindowEvent::KeyboardInput { event: key, .. } => {
                        if key.state == ElementState::Pressed
                            && matches!(key.logical_key.as_ref(), Key::Named(NamedKey::Escape))
                        {
                            elwt.exit();
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        let size = window.inner_size();
                        let (w, h) = (size.width.max(1), size.height.max(1));
                        surface
                            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                            .unwrap();
                        let mut buffer = surface.buffer_mut().unwrap();
                        buffer.iter_mut().for_each(|p| *p = BG);
                        buffer.present().unwrap();
                    }
                    _ => {}
                }
            }
        })
        .unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("--register") => {
            attach_console();
            match register() {
                Ok(()) => println!("vgiew: image associations registered (HKCU)."),
                Err(e) => {
                    eprintln!("vgiew --register: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("--unregister") => {
            attach_console();
            match unregister() {
                Ok(()) => println!("vgiew: associations removed."),
                Err(e) => {
                    eprintln!("vgiew --unregister: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("--dump") => {
            attach_console();
            dump(&args);
            return;
        }
        Some("--help") | Some("-h") => {
            attach_console();
            print_help();
            return;
        }
        _ => {}
    }
    let arg = args.get(1).cloned();

    // Sound files take the audio path; everything else is treated as an image.
    if let Some(a) = &arg {
        if is_sound(Path::new(a)) {
            run_sound(Path::new(a));
            return;
        }
    }

    // Optional reuse mode: if re-enabled, a file launch can hand its path to a
    // running viewer and exit instead of opening a second window.
    #[cfg(windows)]
    if REUSE_RUNNING_WINDOW_ON_FILE_OPEN {
        if let Some(a) = &arg {
            if forward_to_running_instance(Path::new(a)) {
                return;
            }
        }
    }

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
        .build()
        .unwrap();
    let proxy = event_loop.create_proxy();

    // Optional reuse mode: serve future launches that want to reuse this window.
    #[cfg(windows)]
    if REUSE_RUNNING_WINDOW_ON_FILE_OPEN {
        spawn_ipc_server(ipc_pipe_name(), proxy.clone());
    }

    // Restore the saved window position and size if it is present, valid, and still
    // lands on a monitor that exists; otherwise leave placement to Windows (first run,
    // corrupt data, or a monitor that was removed). The image is fit into whatever window
    // we end up with, so no header pre-read is needed to size it.
    let saved_geometry =
        load_window_geometry().filter(|&(x, y, w, h)| monitor_work_area(x, y, w, h).is_some());

    let mut builder = WindowBuilder::new()
        .with_title("vgiew")
        .with_window_icon(load_app_icon())
        // Create hidden; the startup block below reveals it via DWM cloak once the
        // first frame is painted, avoiding both the resize and white flash on show.
        .with_visible(false);
    if let Some((x, y, w, h)) = saved_geometry {
        builder = builder
            .with_position(winit::dpi::PhysicalPosition::new(x, y))
            .with_inner_size(winit::dpi::PhysicalSize::new(w, h));
    }
    let window = Rc::new(builder.build(&event_loop).unwrap());
    // Now that the window and its real frame exist, fit a restored window fully on
    // screen — shrinking/nudging it if the monitor is smaller than when it was saved.
    if saved_geometry.is_some() {
        fit_window_to_screen(&window);
    }
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    // Image list and current index.
    let (mut files, mut current): (Vec<PathBuf>, usize) = match &arg {
        Some(a) => build_siblings(Path::new(a)),
        None => (Vec::new(), 0),
    };

    // Decoded-image cache with neighbor prefetch; bounded to {prev, current, next}.
    let mut cache: HashMap<usize, DecodedImage> = HashMap::new();
    let mut inflight: HashSet<usize> = HashSet::new();
    let mut failed: HashSet<usize> = HashSet::new();

    // View state.
    let mut scale = 1.0f32;
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    let mut fit_mode = true;
    let mut fullscreen = false;

    // Input.
    let mut mouse = (0.0f32, 0.0f32);
    let mut dragging = false;

    // Last known windowed (non-fullscreen) geometry; persisted to the registry on exit.
    let mut win_geom: (i32, i32, u32, u32) = {
        let pos = window
            .outer_position()
            .unwrap_or(winit::dpi::PhysicalPosition::new(0, 0));
        let size = window.inner_size();
        (pos.x, pos.y, size.width, size.height)
    };

    if !files.is_empty() {
        ensure_decode(current, &files, &cache, &mut inflight, &failed, &proxy);
    }

    let update_title = |window: &winit::window::Window,
                        img: Option<&DecodedImage>,
                        scale: f32,
                        files: &[PathBuf],
                        current: usize| {
        let name = files
            .get(current)
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("—");
        match img {
            Some(im) => window.set_title(&format!(
                "vgiew — {name}  [{}×{}]  {:.0}%",
                im.w,
                im.h,
                scale * 100.0
            )),
            None => window.set_title(&format!("vgiew — {name}  (loading…)")),
        }
    };
    update_title(&window, None, scale, &files, current);

    let apply_fit = |img: Option<&DecodedImage>,
                     ww: u32,
                     wh: u32,
                     scale: &mut f32,
                     cx: &mut f32,
                     cy: &mut f32| {
        if let Some(im) = img {
            *scale = view_fit(im.w, im.h, ww, wh);
            *cx = im.w as f32 / 2.0;
            *cy = im.h as f32 / 2.0;
        }
    };

    // Startup without a white flash. A GDI present into a hidden window is discarded, so
    // painting before show does not populate what DWM displays on reveal. Instead we cloak
    // the window at the DWM level and then show it: it is composited (its surface exists)
    // but not displayed. We paint the first frame into that surface, then uncloak — the
    // window appears already sized and painted. As a backstop, a class background brush in
    // the same BG color keeps any stray erase from flashing white even if cloaking is unavailable.
    set_class_background(&window, BG);
    set_cloak(&window, true);
    window.set_visible(true);
    {
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .unwrap();
        let mut buffer = surface.buffer_mut().unwrap();
        let slice: &mut [u32] = &mut buffer;
        draw(cache.get(&current), slice, w, h, scale, cx, cy);
        buffer.present().unwrap();
    }
    set_cloak(&window, false);
    window.request_redraw();

    event_loop
        .run(move |event, elwt| match event {
            Event::UserEvent(ue) => match ue {
                UserEvent::Decoded { path, idx, img: new } => {
                    // Drop results for a file list we have since replaced (Open swapped
                    // folders): the same index now points at a different file.
                    if files.get(idx) != Some(&path) {
                        return;
                    }
                    inflight.remove(&idx);
                    cache.insert(idx, new);
                    if idx == current {
                        let size = window.inner_size();
                        // Fit on first load / folder open (those paths set fit_mode); when a
                        // browse-miss decode lands while zoomed, carry the literal scale.
                        if fit_mode {
                            apply_fit(cache.get(&current), size.width, size.height, &mut scale, &mut cx, &mut cy);
                        } else if let Some(im) = cache.get(&current) {
                            // Carry zoom and pan: keep cx/cy, clamp to the new image.
                            clamp_center(&mut cx, &mut cy, scale, im.w, im.h, size.width as f32, size.height as f32);
                        }
                        update_title(&window, cache.get(&current), scale, &files, current);
                        window.request_redraw();
                        // Current is on screen — now prefetch its neighbors.
                        prefetch(current, &files, &cache, &mut inflight, &failed, &proxy);
                    }
                    evict(&mut cache, current, files.len());
                }
                UserEvent::Failed { path, idx } => {
                    if files.get(idx) != Some(&path) {
                        return;
                    }
                    inflight.remove(&idx);
                    failed.insert(idx);
                    if idx == current {
                        update_title(&window, None, scale, &files, current);
                    }
                }
                UserEvent::Open(path) => {
                    // Optional reuse mode handed this window a file. Rebuild the
                    // folder list and drop all caches: old indices refer to the
                    // previous folder, and stale in-flight decodes are ignored by
                    // the path check above.
                    let (new_files, new_current) = build_siblings(&path);
                    files = new_files;
                    current = new_current;
                    cache.clear();
                    inflight.clear();
                    failed.clear();
                    fit_mode = true;
                    if !files.is_empty() {
                        ensure_decode(current, &files, &cache, &mut inflight, &failed, &proxy);
                    }
                    update_title(&window, None, scale, &files, current);
                    // Bring this window to the front for the user who just opened the file.
                    window.set_minimized(false);
                    window.focus_window();
                    window.request_redraw();
                }
            },
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Moved(pos) => {
                    if !fullscreen {
                        win_geom.0 = pos.x;
                        win_geom.1 = pos.y;
                    }
                }
                WindowEvent::Resized(_) => {
                    let size = window.inner_size();
                    if !fullscreen {
                        win_geom.2 = size.width;
                        win_geom.3 = size.height;
                    }
                    if fit_mode {
                        apply_fit(cache.get(&current), size.width, size.height, &mut scale, &mut cx, &mut cy);
                    } else if let Some(im) = cache.get(&current) {
                        clamp_center(&mut cx, &mut cy, scale, im.w, im.h, size.width as f32, size.height as f32);
                    }
                    window.request_redraw();
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let new = (position.x as f32, position.y as f32);
                    if dragging {
                        cx -= (new.0 - mouse.0) / scale;
                        cy -= (new.1 - mouse.1) / scale;
                        if let Some(im) = cache.get(&current) {
                            let size = window.inner_size();
                            clamp_center(&mut cx, &mut cy, scale, im.w, im.h, size.width as f32, size.height as f32);
                        }
                        fit_mode = false;
                        window.request_redraw();
                    }
                    mouse = new;
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    if button == MouseButton::Left || button == MouseButton::Right {
                        dragging = state == ElementState::Pressed;
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.0,
                    };
                    if dy != 0.0 {
                        if let Some((iw, ih)) = cache.get(&current).map(|im| (im.w, im.h)) {
                            let size = window.inner_size();
                            let (ww, wh) = (size.width as f32, size.height as f32);
                            // Source point under the cursor before zoom.
                            let sx = cx + (mouse.0 - ww / 2.0) / scale;
                            let sy = cy + (mouse.1 - wh / 2.0) / scale;
                            let factor = if dy > 0.0 { 1.25 } else { 0.8 };
                            // Zoom-out floor is an absolute minimum (not fit), so a below-fit
                            // zoom is reachable and can be carried while browsing; `0` refits.
                            scale = (scale * factor).clamp(MIN_SCALE, 64.0);
                            // Keep the same source point under the cursor, then bound/center.
                            cx = sx - (mouse.0 - ww / 2.0) / scale;
                            cy = sy - (mouse.1 - wh / 2.0) / scale;
                            clamp_center(&mut cx, &mut cy, scale, iw, ih, ww, wh);
                            fit_mode = false;
                            update_title(&window, cache.get(&current), scale, &files, current);
                            window.request_redraw();
                        }
                    }
                }
                WindowEvent::KeyboardInput { event: key, .. } => {
                    if key.state != ElementState::Pressed {
                        return;
                    }
                    let size = window.inner_size();
                    let lk = key.logical_key.as_ref();
                    match lk {
                        Key::Named(NamedKey::ArrowRight)
                        | Key::Named(NamedKey::ArrowLeft) => {
                            if !files.is_empty() {
                                let forward = !matches!(lk, Key::Named(NamedKey::ArrowLeft));
                                current = if forward {
                                    (current + 1) % files.len()
                                } else {
                                    (current + files.len() - 1) % files.len()
                                };
                                if cache.contains_key(&current) {
                                    // Prefetched — show instantly. Refit only if we were at
                                    // fit; otherwise carry the current zoom onto the new image
                                    // (recenter + clamp), keeping the literal scale.
                                    if fit_mode {
                                        apply_fit(cache.get(&current), size.width, size.height, &mut scale, &mut cx, &mut cy);
                                    } else if let Some(im) = cache.get(&current) {
                                        // Carry zoom and pan: keep cx/cy, clamp to the new image.
                                        clamp_center(&mut cx, &mut cy, scale, im.w, im.h, size.width as f32, size.height as f32);
                                    }
                                    update_title(&window, cache.get(&current), scale, &files, current);
                                    window.request_redraw();
                                } else {
                                    // Miss — kick off decode; keep the previous frame on screen until it arrives.
                                    ensure_decode(current, &files, &cache, &mut inflight, &failed, &proxy);
                                    update_title(&window, None, scale, &files, current);
                                }
                                prefetch(current, &files, &cache, &mut inflight, &failed, &proxy);
                                evict(&mut cache, current, files.len());
                            }
                        }
                        Key::Named(NamedKey::Escape) => {
                            if fullscreen {
                                fullscreen = false;
                                window.set_fullscreen(None);
                            } else {
                                elwt.exit();
                            }
                        }
                        Key::Named(NamedKey::Enter) | Key::Character("f") => {
                            fullscreen = !fullscreen;
                            window.set_fullscreen(if fullscreen {
                                Some(Fullscreen::Borderless(None))
                            } else {
                                None
                            });
                        }
                        Key::Character("0") => {
                            fit_mode = true;
                            apply_fit(cache.get(&current), size.width, size.height, &mut scale, &mut cx, &mut cy);
                            update_title(&window, cache.get(&current), scale, &files, current);
                            window.request_redraw();
                        }
                        Key::Character("1") => {
                            if let Some((iw, ih)) = cache.get(&current).map(|im| (im.w, im.h)) {
                                scale = 1.0;
                                cx = iw as f32 / 2.0;
                                cy = ih as f32 / 2.0;
                                clamp_center(&mut cx, &mut cy, scale, iw, ih, size.width as f32, size.height as f32);
                                fit_mode = false;
                                update_title(&window, cache.get(&current), scale, &files, current);
                                window.request_redraw();
                            }
                        }
                        _ => {}
                    }
                }
                WindowEvent::RedrawRequested => {
                    let size = window.inner_size();
                    let (w, h) = (size.width.max(1), size.height.max(1));
                    surface
                        .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                        .unwrap();
                    let mut buffer = surface.buffer_mut().unwrap();
                    let slice: &mut [u32] = &mut buffer;
                    draw(cache.get(&current), slice, w, h, scale, cx, cy);
                    buffer.present().unwrap();
                }
                _ => {}
            },
            Event::LoopExiting => {
                save_window_geometry(win_geom.0, win_geom.1, win_geom.2, win_geom.3);
            }
            _ => {}
        })
        .unwrap();
}
