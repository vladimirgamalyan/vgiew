// Cold-start baseline for the SVG question: winit + softbuffer, nothing else linked.
// Its only job is to be byte-for-byte comparable to `svg_start_resvg`, whose startup
// path is identical — the only difference between the two binaries is that the other
// one has resvg/usvg/tiny-skia linked in. The delta between them IS the price of
// linking an SVG renderer into vgiew.
//
// Keep this file and svg_start_resvg's startup path in sync; the duplication is
// deliberate (two crates = two link units = the thing being measured).
use std::io::Write;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

const BG: u32 = 0x00F5_F5F5;

fn main() {
    let t0 = Instant::now();
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("svg_start_baseline")
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
