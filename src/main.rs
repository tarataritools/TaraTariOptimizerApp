mod cleanup;
mod inventory;
mod memory;
mod storage;
mod ui;

fn main() -> eframe::Result<()> {
    let app_icon = image::load_from_memory(include_bytes!(
        "../assets/TaraTariOptimizerAppLogoCircle.png"
    ))
    .expect("the embedded TaraTari app logo must be a valid PNG")
    .into_rgba8();
    let icon_data = eframe::egui::IconData {
        width: app_icon.width(),
        height: app_icon.height(),
        rgba: app_icon.into_raw(),
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([820.0, 600.0])
            .with_icon(icon_data),
        ..Default::default()
    };

    eframe::run_native(
        "TaraTari Optimizer App",
        options,
        Box::new(|creation_context| Ok(Box::new(ui::OptimizerApp::new(creation_context)))),
    )
}
