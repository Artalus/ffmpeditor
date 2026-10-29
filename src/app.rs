use crate::config::AppConfig;
use crate::model::Project;

pub struct App {
    config: AppConfig,
    project: Project,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            config: AppConfig::load(),
            project: Project::default(),
        }
    }
}

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
            ui.label(format!("ffmpeg: {}", self.config.ffmpeg_path.display()));
        });
    }
}
