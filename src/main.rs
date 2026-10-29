#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Modules land ahead of the code using them; drop once everything is wired up.
#![allow(dead_code, unused_variables)]

mod config;
mod ffmpeg;
mod model;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native("ffmpeditor", options, Box::new(|_cc| Ok(Box::new(App))))
}

struct App;

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label("ffmpeditor");
        });
    }
}
