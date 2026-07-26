// The SVG cost spike. Two questions, deliberately separated:
//
//  1. What does *linking* resvg cost at startup? Run with no arguments: the startup
//     path below is identical to `svg_start_baseline`, so the difference in
//     first_frame_ms / wall-clock between the two binaries is the price of carrying
//     an SVG renderer — larger image, more relocations, extra imports — even when it
//     is never called. The `--bench`/`--fonts` branches are argv-gated on purpose:
//     without a runtime-reachable call the linker strips resvg entirely (a probe
//     binary that only *declared* the dependency came out at 122 KB), and the
//     measurement would compare the baseline against itself.
//
//  2. What does *using* it cost? `--bench <file.svg> [W H]` reports the breakdown a
//     double-clicked .svg would pay before its first pixels appear (fonts, parse,
//     render, premultiplied -> 0xAARRGGBB), plus the render cost at 4x and 16x zoom
//     into the same viewport — the per-gesture cost of the viewport-only
//     re-rasterization design. `--fonts` times system font loading on its own.
//
// Keep the startup path in sync with svg_start_baseline/src/main.rs.
use std::io::Write;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

use rayon::prelude::*;
use resvg::tiny_skia;
use resvg::usvg;

const BG: u32 = 0x00F5_F5F5;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

// The cheap pre-parse test vgiew would use to decide whether system fonts are needed
// at all. Font loading dominates everything else, and most SVGs (icons, logos, chart
// exports) carry no text — so this one scan is what keeps the common case cheap.
fn has_text(data: &[u8]) -> bool {
    data.windows(5).any(|w| w == b"<text") || data.windows(6).any(|w| w == b"<tspan")
}

fn bench(path: &str, ww: u32, wh: u32) {
    let t_read = Instant::now();
    let data = std::fs::read(path).expect("read");
    let read_ms = ms(t_read);

    let text = has_text(&data);

    let mut opt = usvg::Options {
        resources_dir: std::fs::canonicalize(path)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
        ..usvg::Options::default()
    };

    // Only pay for fonts when the file actually has text.
    let t_fonts = Instant::now();
    if text {
        opt.fontdb_mut().load_system_fonts();
    }
    let fonts_ms = ms(t_fonts);

    let t_parse = Instant::now();
    let tree = match usvg::Tree::from_data(&data, &opt) {
        Ok(t) => t,
        Err(e) => {
            println!("file={path} ERROR={e}");
            return;
        }
    };
    let parse_ms = ms(t_parse);

    let size = tree.size();
    let (iw, ih) = (size.width(), size.height());
    let fit = (ww as f32 / iw).min(wh as f32 / ih);

    let mut pixmap = tiny_skia::Pixmap::new(ww, wh).expect("pixmap");

    // The transform that puts the image, scaled by `s`, centred in the viewport.
    // Rendering straight into a viewport-sized pixmap is the L2 model: cost tracks the
    // *visible* area and memory stays ww*wh*4 whatever the zoom.
    let view = |s: f32| {
        tiny_skia::Transform::from_translate((ww as f32 - iw * s) / 2.0, (wh as f32 - ih * s) / 2.0)
            .pre_scale(s, s)
    };

    // Warm-up: the first render of a tree pays one-time costs (lazy caches, first touch
    // of the pixmap's pages). Timing it as "render at fit" inflated fit relative to the
    // zoom levels measured after it.
    resvg::render(&tree, view(fit), &mut pixmap.as_mut());

    // Median of 3 per scale, to blunt scheduler noise.
    let mut render_ms = [0.0f64; 3];
    for (i, mul) in [1.0f32, 4.0, 16.0].iter().enumerate() {
        let xf = view(fit * mul);
        let mut samples = [0.0f64; 3];
        for s in samples.iter_mut() {
            pixmap.fill(tiny_skia::Color::TRANSPARENT);
            let t = Instant::now();
            resvg::render(&tree, xf, &mut pixmap.as_mut());
            *s = ms(t);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        render_ms[i] = samples[1];
    }

    // resvg::render is single-threaded, but the viewport can be cut into horizontal
    // strips rendered independently — a strip is just a shorter pixmap with a shifted
    // transform. vgiew already has rayon, so this is the headroom that decides whether
    // an interactive zoom on a complex file can keep up. Measured at 4x, the worst of
    // the three scales above for the mid-size files.
    let strips = rayon::current_num_threads().min(16) as u32;
    let xf4 = view(fit * 4.0);
    let mut tiled_ms = f64::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        let rows: Vec<tiny_skia::Pixmap> = (0..strips)
            .into_par_iter()
            .map(|k| {
                let y0 = wh * k / strips;
                let y1 = wh * (k + 1) / strips;
                let mut pm = tiny_skia::Pixmap::new(ww, y1 - y0).unwrap();
                resvg::render(&tree, xf4.post_translate(0.0, -(y0 as f32)), &mut pm.as_mut());
                pm
            })
            .collect();
        tiled_ms = tiled_ms.min(ms(t));
        std::hint::black_box(&rows);
    }

    // Premultiplied RGBA8 -> straight 0xAARRGGBB, what vgiew's DecodedImage holds.
    // Skipping the demultiply would darken every antialiased edge, so it is part of
    // the real cost, not an optimisation to hand-wave away.
    let t_conv = Instant::now();
    let px: Vec<u32> = pixmap
        .pixels()
        .iter()
        .map(|p| {
            let c = p.demultiply();
            ((c.alpha() as u32) << 24)
                | ((c.red() as u32) << 16)
                | ((c.green() as u32) << 8)
                | (c.blue() as u32)
        })
        .collect();
    let conv_ms = ms(t_conv);

    let total_first = fonts_ms + parse_ms + render_ms[0] + conv_ms;
    println!(
        "file={:<22} bytes={:>9} text={:<5} size={:.0}x{:.0} read={:.2} fonts={:.2} parse={:.2} \
         fit={:.2} 4x={:.2} 16x={:.2} 4x_tiled{}={:.2} conv={:.2} first_pixels={:.2} px={}",
        std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path),
        data.len(),
        text,
        iw,
        ih,
        read_ms,
        fonts_ms,
        parse_ms,
        render_ms[0],
        render_ms[1],
        render_ms[2],
        strips,
        tiled_ms,
        conv_ms,
        total_first,
        px.len(),
    );
}

// Twice in one process: the first pass reads every font file off disk, the second finds
// them in the OS file cache. The gap between the two is the whole story — the cold number
// is what the first text-bearing SVG after a boot would pay.
fn bench_fonts() {
    for pass in ["cold", "warm"] {
        let t = Instant::now();
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        println!("load_system_fonts[{pass}]_ms={:.2} faces={}", ms(t), db.len());
    }
}

fn main() {
    let t0 = Instant::now();
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--fonts") {
        bench_fonts();
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--bench") {
        let path = args.get(i + 1).expect("--bench <file.svg>");
        let ww: u32 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(1600);
        let wh: u32 = args.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(1000);
        bench(path, ww, wh);
        return;
    }

    // ── startup path: identical to svg_start_baseline ──
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("svg_start_resvg")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
            .build(&event_loop)
            .unwrap(),
    );

    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    event_loop
        .run(move |event, elwt| match event {
            Event::AboutToWait => window.request_redraw(),
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                let size = window.inner_size();
                let (w, h) = (size.width.max(1), size.height.max(1));
                surface
                    .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                    .unwrap();
                let mut buffer = surface.buffer_mut().unwrap();
                buffer.iter_mut().for_each(|p| *p = BG);
                buffer.present().unwrap();
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                let mut out = std::io::stdout();
                let _ = writeln!(out, "first_frame_ms={ms:.2}");
                let _ = out.flush();
                elwt.exit();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),
            _ => {}
        })
        .unwrap();
}
