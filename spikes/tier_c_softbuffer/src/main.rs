// Tier C: winit + softbuffer (outputs a CPU pixel buffer, no GPU device).
// Measures the time from main start to the first frame present.
use std::io::Write;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

fn main() {
    let t0 = Instant::now();
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("tier_c")
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
                for y in 0..h {
                    for x in 0..w {
                        let r = x * 255 / w;
                        let g = y * 255 / h;
                        let b = 128u32;
                        buffer[(y * w + x) as usize] = (r << 16) | (g << 8) | b;
                    }
                }
                buffer.present().unwrap();
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
