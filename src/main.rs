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
use winit::event_loop::{EventLoopBuilder, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, WindowBuilder};

const BG: u32 = 0x001E_1E1E; // dark background (softbuffer: 0x00RRGGBB)

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "webp"];

struct DecodedImage {
    w: u32,
    h: u32,
    px: Vec<u32>, // 0xAARRGGBB
}

enum UserEvent {
    Decoded { idx: usize, img: DecodedImage },
    Failed { idx: usize },
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
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

fn spawn_decode(path: PathBuf, idx: usize, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || match load_rgba(&path) {
        Some(rgba) => {
            let _ = proxy.send_event(UserEvent::Decoded {
                idx,
                img: pack_rgba(&rgba),
            });
        }
        None => {
            let _ = proxy.send_event(UserEvent::Failed { idx });
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
        (0x2B as f32, 0x2B as f32, 0x2B as f32)
    } else {
        (0x35 as f32, 0x35 as f32, 0x35 as f32)
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

fn print_help() {
    println!(
        "vgiew — a fast image viewer\n\n\
         Usage:\n  \
         vgiew <file>                        open an image\n  \
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
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
        .build()
        .unwrap();
    let proxy = event_loop.create_proxy();

    let window = Rc::new(
        WindowBuilder::new()
            .with_title("vgiew")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
            // Create hidden: winit shows the window at Windows' default size first and
            // only then applies the requested inner size, which flashes as a brief
            // resize. We reveal it below, already sized and painted.
            .with_visible(false)
            .build(&event_loop)
            .unwrap(),
    );
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    // Image list and current index.
    let (files, mut current): (Vec<PathBuf>, usize) = match &arg {
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
            *scale = fit_scale(im.w, im.h, ww, wh);
            *cx = im.w as f32 / 2.0;
            *cy = im.h as f32 / 2.0;
        }
    };

    // Paint the first frame (dark background) into the already-sized surface, then reveal
    // the window — so it appears at its final size with content, with no startup flash.
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
    window.set_visible(true);
    window.request_redraw();

    event_loop
        .run(move |event, elwt| match event {
            Event::UserEvent(ue) => match ue {
                UserEvent::Decoded { idx, img: new } => {
                    inflight.remove(&idx);
                    cache.insert(idx, new);
                    if idx == current {
                        let size = window.inner_size();
                        fit_mode = true;
                        apply_fit(cache.get(&current), size.width, size.height, &mut scale, &mut cx, &mut cy);
                        update_title(&window, cache.get(&current), scale, &files, current);
                        window.request_redraw();
                        // Current is on screen — now prefetch its neighbors.
                        prefetch(current, &files, &cache, &mut inflight, &failed, &proxy);
                    }
                    evict(&mut cache, current, files.len());
                }
                UserEvent::Failed { idx } => {
                    inflight.remove(&idx);
                    failed.insert(idx);
                    if idx == current {
                        update_title(&window, None, scale, &files, current);
                    }
                }
            },
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Resized(_) => {
                    if fit_mode {
                        let size = window.inner_size();
                        apply_fit(cache.get(&current), size.width, size.height, &mut scale, &mut cx, &mut cy);
                    }
                    window.request_redraw();
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let new = (position.x as f32, position.y as f32);
                    if dragging {
                        cx -= (new.0 - mouse.0) / scale;
                        cy -= (new.1 - mouse.1) / scale;
                        fit_mode = false;
                        window.request_redraw();
                    }
                    mouse = new;
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    if button == MouseButton::Left {
                        dragging = state == ElementState::Pressed;
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.0,
                    };
                    if dy != 0.0 && cache.contains_key(&current) {
                        let size = window.inner_size();
                        let (ww, wh) = (size.width as f32, size.height as f32);
                        // Source point under the cursor before zoom.
                        let sx = cx + (mouse.0 - ww / 2.0) / scale;
                        let sy = cy + (mouse.1 - wh / 2.0) / scale;
                        let factor = if dy > 0.0 { 1.25 } else { 0.8 };
                        scale = (scale * factor).clamp(0.01, 64.0);
                        // Keep the same source point under the cursor.
                        cx = sx - (mouse.0 - ww / 2.0) / scale;
                        cy = sy - (mouse.1 - wh / 2.0) / scale;
                        fit_mode = false;
                        update_title(&window, cache.get(&current), scale, &files, current);
                        window.request_redraw();
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
                        | Key::Named(NamedKey::Space)
                        | Key::Named(NamedKey::ArrowLeft) => {
                            if !files.is_empty() {
                                let forward = !matches!(lk, Key::Named(NamedKey::ArrowLeft));
                                current = if forward {
                                    (current + 1) % files.len()
                                } else {
                                    (current + files.len() - 1) % files.len()
                                };
                                if cache.contains_key(&current) {
                                    // Prefetched — show instantly.
                                    fit_mode = true;
                                    apply_fit(cache.get(&current), size.width, size.height, &mut scale, &mut cx, &mut cy);
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
                            if let Some(im) = cache.get(&current) {
                                scale = 1.0;
                                cx = im.w as f32 / 2.0;
                                cy = im.h as f32 / 2.0;
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
            _ => {}
        })
        .unwrap();
}
