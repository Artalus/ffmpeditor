use std::path::{Path, PathBuf};

use egui::{Align, Align2, Context, Id, Label, Layout, Modal, ScrollArea, TextEdit};

use crate::config::AppConfig;
use crate::ffmpeg::Ffmpeg;

pub enum Outcome<T> {
    Open,
    Cancelled,
    Accepted(T),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

pub struct SettingsDialog {
    ffmpeg_path: String,
    test: Option<Result<String, String>>,
}

pub fn unsaved_changes(ctx: &Context, project_name: &str) -> Option<UnsavedChoice> {
    let outcome = modal(ctx, "Unsaved changes", |ui| {
        ui.label(format!("Save changes to {project_name}?"));
        ui.separator();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Cancel").clicked() {
                return Outcome::Cancelled;
            }
            if ui.button("Discard").clicked() {
                return Outcome::Accepted(UnsavedChoice::Discard);
            }
            if ui.button("Save").clicked() {
                return Outcome::Accepted(UnsavedChoice::Save);
            }
            Outcome::Open
        })
        .inner
    });
    match outcome {
        Outcome::Open => None,
        Outcome::Cancelled => Some(UnsavedChoice::Cancel),
        Outcome::Accepted(choice) => Some(choice),
    }
}

/// Non-blocking list of messages, each dismissable.
pub fn errors(ctx: &Context, errors: &mut Vec<String>) {
    if errors.is_empty() {
        return;
    }
    let mut dismissed = None;
    let mut dismiss_all = false;
    egui::Window::new("Errors")
        .anchor(Align2::RIGHT_TOP, [-8.0, 32.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for (i, error) in errors.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        if ui.small_button("×").clicked() {
                            dismissed = Some(i);
                        }
                        ui.add(Label::new(error.as_str()).wrap());
                    });
                }
            });
            if errors.len() > 1 && ui.button("Dismiss all").clicked() {
                dismiss_all = true;
            }
        });
    if dismiss_all {
        errors.clear();
    } else if let Some(i) = dismissed {
        errors.remove(i);
    }
}

impl SettingsDialog {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            ffmpeg_path: config.ffmpeg_path.display().to_string(),
            test: None,
        }
    }

    pub fn show(&mut self, ctx: &Context) -> Outcome<AppConfig> {
        modal(ctx, "Settings", |ui| {
            ui.label(
                "ffmpeg executable; ffprobe is expected next to it. Bare \"ffmpeg\" searches PATH.",
            );
            ui.horizontal(|ui| {
                let edit = ui.add(TextEdit::singleline(&mut self.ffmpeg_path).desired_width(360.0));
                if edit.changed() {
                    self.test = None;
                }
                if ui.button("Browse…").clicked()
                    && let Some(path) = pick_ffmpeg()
                {
                    self.ffmpeg_path = path.display().to_string();
                    self.test = None;
                }
                if ui.button("Test").clicked() {
                    let version = Ffmpeg::new(self.path()).version();
                    self.test = Some(version.map_err(|e| format!("{e:#}")));
                }
            });
            match &self.test {
                Some(Ok(version)) => {
                    ui.label(version);
                }
                Some(Err(err)) => {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                }
                None => {}
            }
            match confirm_buttons(ui, "OK", !self.path().as_os_str().is_empty()) {
                Some(true) => Outcome::Accepted(AppConfig {
                    ffmpeg_path: self.path().into(),
                }),
                Some(false) => Outcome::Cancelled,
                None => Outcome::Open,
            }
        })
    }

    fn path(&self) -> &Path {
        Path::new(self.ffmpeg_path.trim())
    }
}

/// Modal closed only by its own buttons or Escape: the stock click-outside-to-close would throw away edits.
pub fn modal<T>(
    ctx: &Context,
    title: &str,
    content: impl FnOnce(&mut egui::Ui) -> Outcome<T>,
) -> Outcome<T> {
    let response = Modal::new(Id::unique(("modal", title))).show(ctx, |ui| {
        ui.set_min_width(320.0);
        ui.heading(title);
        ui.add_space(4.0);
        content(ui)
    });
    if matches!(response.inner, Outcome::Open)
        && response.should_close()
        && !response.backdrop_response.clicked()
    {
        return Outcome::Cancelled;
    }
    response.inner
}

/// Right-aligned confirm/cancel row; `Some(true)` when confirmed.
pub fn confirm_buttons(ui: &mut egui::Ui, confirm: &str, enabled: bool) -> Option<bool> {
    ui.separator();
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.button("Cancel").clicked() {
            return Some(false);
        }
        if ui
            .add_enabled(enabled, egui::Button::new(confirm))
            .clicked()
        {
            return Some(true);
        }
        None
    })
    .inner
}

fn pick_ffmpeg() -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new().set_title("Locate ffmpeg");
    let dialog = if cfg!(windows) {
        dialog.add_filter("ffmpeg", &["exe"])
    } else {
        dialog
    };
    dialog.add_filter("All files", &["*"]).pick_file()
}
