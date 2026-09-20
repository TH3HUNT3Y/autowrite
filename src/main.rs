mod app;
mod config;
mod input;
mod text;
mod typing;

fn main() -> eframe::Result {
    eframe::run_native(
        "Auto Write",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1240.0, 780.0])
                .with_min_inner_size([920.0, 620.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::AutoWriteApp::new(cc)))),
    )
}
