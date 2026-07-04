// Tier A: eframe/egui + wgpu. A full immediate-mode GUI toolkit with GPU.
// Measures the time from main start to the first update call (after wgpu init).
use std::io::Write;
use std::time::Instant;

use eframe::egui;

fn main() {
    let t0 = Instant::now();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tier_a",
        options,
        Box::new(move |_cc| Ok(Box::new(App { t0 }))),
    )
    .unwrap();
}

struct App {
    t0: Instant,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("first frame");
        });
        let ms = self.t0.elapsed().as_secs_f64() * 1000.0;
        let mut out = std::io::stdout();
        let _ = writeln!(out, "first_frame_ms={:.2}", ms);
        let _ = out.flush();
        std::process::exit(0);
    }
}
