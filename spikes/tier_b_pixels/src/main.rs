// Tier B: winit + pixels (a thin GPU blit of a single texture via wgpu).
// Measures the time from main start to the first frame present.
use std::io::Write;
use std::time::Instant;

use pixels::{Pixels, SurfaceTexture};
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

const W: u32 = 1280;
const H: u32 = 800;

fn main() {
    let t0 = Instant::now();
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("tier_b")
        .with_inner_size(winit::dpi::LogicalSize::new(W as f64, H as f64))
        .build(&event_loop)
        .unwrap();

    let size = window.inner_size();
    // SurfaceTexture borrows the window; winit 0.29 allows a non-'static closure,
    // so the window and pixels stay local and the closure borrows them.
    let surface_texture = SurfaceTexture::new(size.width.max(1), size.height.max(1), &window);
    let mut pixels = Pixels::new(W, H, surface_texture).unwrap();

    event_loop
        .run(|event, elwt| match event {
            Event::AboutToWait => window.request_redraw(),
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                let frame = pixels.frame_mut();
                for (i, px) in frame.chunks_exact_mut(4).enumerate() {
                    let x = (i as u32) % W;
                    let y = (i as u32) / W;
                    px[0] = (x * 255 / W) as u8;
                    px[1] = (y * 255 / H) as u8;
                    px[2] = 128;
                    px[3] = 255;
                }
                pixels.render().unwrap();
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                let mut out = std::io::stdout();
                let _ = writeln!(out, "first_frame_ms={:.2}", ms);
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
