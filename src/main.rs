#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Module APIs are stubbed ahead of their implementations; drop once everything is wired up.
#![allow(dead_code, unused_variables)]

mod app;
mod config;
mod ffmpeg;
mod media;
mod model;
mod render;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ffmpeditor")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ffmpeditor",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
