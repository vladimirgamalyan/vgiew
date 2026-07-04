// Measures the interactive cost of CPU resampling (Tier C path).
// Source is a synthetic 6000x4000 image (24 MP). Window 1600x1000.
// Measures ms/frame for bilinear resampling source->window at different scales,
// single-threaded and multi-threaded (std::thread::scope). Plus the present cost separately.
use std::io::Write;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

const SRC_W: u32 = 6000;
const SRC_H: u32 = 4000;
const WIN_W: u32 = 1600;
const WIN_H: u32 = 1000;

fn build_source() -> Vec<u32> {
    // High-frequency pattern so resampling actually reads varied data.
    let mut v = vec![0u32; (SRC_W * SRC_H) as usize];
    for y in 0..SRC_H {
        for x in 0..SRC_W {
            let r = (x ^ y) & 0xFF;
            let g = (x.wrapping_mul(3) ^ y.wrapping_mul(7)) & 0xFF;
            let b = ((x / 8 + y / 8) & 0xFF) as u32;
            v[(y * SRC_W + x) as usize] = (r << 16) | (g << 8) | b;
        }
    }
    v
}

#[inline(always)]
fn sample(src: &[u32], fx: f32, fy: f32) -> u32 {
    let x = fx.clamp(0.0, (SRC_W - 1) as f32);
    let y = fy.clamp(0.0, (SRC_H - 1) as f32);
    let x0 = x as u32;
    let y0 = y as u32;
    let x1 = (x0 + 1).min(SRC_W - 1);
    let y1 = (y0 + 1).min(SRC_H - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = src[(y0 * SRC_W + x0) as usize];
    let p10 = src[(y0 * SRC_W + x1) as usize];
    let p01 = src[(y1 * SRC_W + x0) as usize];
    let p11 = src[(y1 * SRC_W + x1) as usize];
    let ch = |p: u32, s: u32| ((p >> s) & 0xFF) as f32;
    let bl = |a, b, c, d| {
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        (top + (bot - top) * ty) as u32
    };
    let r = bl(ch(p00, 16), ch(p10, 16), ch(p01, 16), ch(p11, 16));
    let g = bl(ch(p00, 8), ch(p10, 8), ch(p01, 8), ch(p11, 8));
    let b = bl(ch(p00, 0), ch(p10, 0), ch(p01, 0), ch(p11, 0));
    (r << 16) | (g << 8) | b
}

fn render_row(src: &[u32], row: &mut [u32], dy: u32, scale: f32, ox: f32, oy: f32) {
    let sy = dy as f32 / scale + oy;
    for dx in 0..WIN_W {
        let sx = dx as f32 / scale + ox;
        row[dx as usize] = sample(src, sx, sy);
    }
}

fn render_st(src: &[u32], dst: &mut [u32], scale: f32, ox: f32, oy: f32) {
    for (li, row) in dst.chunks_exact_mut(WIN_W as usize).enumerate() {
        render_row(src, row, li as u32, scale, ox, oy);
    }
}

fn render_mt(src: &[u32], dst: &mut [u32], scale: f32, ox: f32, oy: f32, threads: usize) {
    let rows_per = (WIN_H as usize + threads - 1) / threads;
    std::thread::scope(|s| {
        for (ti, chunk) in dst.chunks_mut(rows_per * WIN_W as usize).enumerate() {
            s.spawn(move || {
                let y_start = ti * rows_per;
                for (li, row) in chunk.chunks_exact_mut(WIN_W as usize).enumerate() {
                    render_row(src, row, (y_start + li) as u32, scale, ox, oy);
                }
            });
        }
    });
}

fn stats(mut v: Vec<f64>) -> String {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = v[0];
    let med = v[v.len() / 2];
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    format!(
        "min={:5.2}  med={:5.2}  mean={:5.2} ms/frame  (~{:.0} FPS by med)",
        min, med, mean, 1000.0 / med
    )
}

fn bench(name: &str, src: &[u32], dst: &mut [u32], scale: f32, threads: usize, mt: bool) {
    // warmup
    for i in 0..10 {
        let ox = (i as f32) * 3.0;
        if mt {
            render_mt(src, dst, scale, ox, 0.0, threads);
        } else {
            render_st(src, dst, scale, ox, 0.0);
        }
    }
    let frames = 120;
    let mut times = Vec::with_capacity(frames);
    for i in 0..frames {
        // Pan each frame — emulate dragging / zoom animation.
        let ox = (i as f32) * 5.0;
        let oy = (i as f32) * 2.0;
        let t = Instant::now();
        if mt {
            render_mt(src, dst, scale, ox, oy, threads);
        } else {
            render_st(src, dst, scale, ox, oy);
        }
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let mode = if mt { format!("MT x{threads}") } else { "ST".to_string() };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "  [{name:8}] {mode:8}: {}", stats(times));
    let _ = out.flush();
}

fn main() {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
    let src = build_source();
    let mut dst = vec![0u32; (WIN_W * WIN_H) as usize];

    let fit = (WIN_W as f32 / SRC_W as f32).min(WIN_H as f32 / SRC_H as f32); // ~0.25

    let mut out = std::io::stdout();
    let _ = writeln!(
        out,
        "Source {SRC_W}x{SRC_H} (24 MP) -> window {WIN_W}x{WIN_H}, threads available: {threads}"
    );
    let _ = writeln!(out, "Threshold: 60 Hz = 16.67 ms, 144 Hz = 6.94 ms per frame");
    let _ = out.flush();

    for (name, scale) in [("fit", fit), ("100%", 1.0), ("300%", 3.0)] {
        bench(name, &src, &mut dst, scale, threads, false);
        bench(name, &src, &mut dst, scale, threads, true);
    }

    // Present cost (GDI window blit) — measured separately via softbuffer.
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("interactive_cpu")
            .with_inner_size(winit::dpi::LogicalSize::new(WIN_W as f64, WIN_H as f64))
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
                let mut present_times = Vec::new();
                for _ in 0..60 {
                    let mut buffer = surface.buffer_mut().unwrap();
                    let n = buffer.len().min(dst.len());
                    buffer[..n].copy_from_slice(&dst[..n]);
                    let t = Instant::now();
                    buffer.present().unwrap();
                    present_times.push(t.elapsed().as_secs_f64() * 1000.0);
                }
                let mut out = std::io::stdout();
                let _ = writeln!(
                    out,
                    "  [present ] window {w}x{h}: {}",
                    stats(present_times)
                );
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
