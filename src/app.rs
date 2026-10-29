mod commands;
mod editing;
mod history;

use std::path::{Path, PathBuf};

use egui::{Context, ViewportCommand};

use crate::config::AppConfig;
use crate::ffmpeg::Ffmpeg;
use crate::media::MediaCache;
use crate::model::Project;
use crate::render::Previewer;
use crate::ui;
use crate::ui::timeline::{TimelineResponse, TimelineState};
use commands::{Command, MenuState};
use editing::{split_targets, step_frames};
use history::History;

pub struct App {
    config: AppConfig,
    ffmpeg: Ffmpeg,
    /// `ffmpeg -version` line, or why it could not be run.
    ffmpeg_status: Result<String, String>,
    project: Project,
    project_path: Option<PathBuf>,
    /// Snapshot as last saved or loaded, for dirty detection.
    saved: Project,
    history: History,
    media: MediaCache,
    previewer: Previewer,
    timeline: TimelineState,
    errors: Vec<String>,
    /// Last title sent to the window, to avoid resending it every frame.
    title: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        let ffmpeg = Ffmpeg::new(&config.ffmpeg_path);
        let ffmpeg_status = check_ffmpeg(&ffmpeg);
        let mut errors = Vec::new();
        if let Err(err) = &ffmpeg_status {
            errors.push(format!(
                "ffmpeg not usable ({err}); set its location in Options → Settings."
            ));
        }
        let project = Project::default();
        Self {
            media: MediaCache::new(cc.egui_ctx.clone(), ffmpeg.clone()),
            previewer: Previewer::new(cc.egui_ctx.clone()),
            history: History::new(&project),
            saved: project.clone(),
            project,
            project_path: None,
            config,
            ffmpeg,
            ffmpeg_status,
            timeline: TimelineState::default(),
            errors,
            title: String::new(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.media.poll();
        self.previewer.poll();
        for command in self.commands(ui) {
            self.run(command, ui.ctx());
        }
        self.status_bar(ui);
        let timeline = self.timeline_panel(ui);
        self.viewer(ui);
        self.settle(&timeline);
        ui::dialogs::errors(ui.ctx(), &mut self.errors);
        self.update_title(ui.ctx());
    }
}

impl App {
    fn commands(&self, ui: &mut egui::Ui) -> Vec<Command> {
        let mut commands = if ui.ctx().text_edit_focused() {
            Vec::new()
        } else {
            commands::read_shortcuts(ui.ctx())
        };
        let state = MenuState {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            clip_selected: self.timeline.selected_clip.is_some(),
        };
        commands.extend(commands::menu_bar(ui, &state));
        commands
    }

    fn run(&mut self, command: Command, ctx: &Context) {
        let playhead = &mut self.timeline.playhead;
        let duration = self.project.duration();
        let fps = self.project.settings.fps;
        match command {
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Command::Undo => {
                self.history.undo(&mut self.project);
            }
            Command::Redo => {
                self.history.redo(&mut self.project);
            }
            Command::Split => self.split_at_playhead(),
            Command::DeleteClip => self.delete_selected_clip(),
            Command::GoToStart => *playhead = 0.0,
            Command::GoToEnd => *playhead = duration,
            Command::PreviousFrame => *playhead = step_frames(*playhead, fps, -1, duration),
            Command::NextFrame => *playhead = step_frames(*playhead, fps, 1, duration),
        }
    }

    fn split_at_playhead(&mut self) {
        let at = self.timeline.playhead;
        for id in split_targets(&self.project, self.timeline.selected_clip, at) {
            self.project.split_clip(id, at);
        }
    }

    fn delete_selected_clip(&mut self) {
        if let Some(id) = self.timeline.selected_clip.take() {
            self.project.remove_clip(id);
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        let status = ui::status_bar::Status {
            playhead: self.timeline.playhead,
            duration: self.project.duration(),
            selection: self.timeline.selection,
            ffmpeg: &self.ffmpeg_status,
            export_progress: None,
        };
        egui::Panel::bottom("status").show(ui, |ui| ui::status_bar::show(ui, &status));
    }

    fn timeline_panel(&mut self, ui: &mut egui::Ui) -> TimelineResponse {
        let default_height = ui.ctx().content_rect().height() * 0.45;
        egui::Panel::bottom("timeline")
            .resizable(true)
            .default_size(default_height)
            .min_size(120.0)
            .show(ui, |ui| {
                ui::timeline::show(ui, &mut self.timeline, &mut self.project, &self.media)
            })
            .inner
    }

    fn viewer(&self, ui: &mut egui::Ui) {
        let placeholder = if self.project.clips.is_empty() {
            "Import media to start: File → Import media…"
        } else {
            "No preview"
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| ui::viewer::show(ui, &self.previewer, placeholder));
    }

    /// Turns finished edits into undo steps and keeps the preview at the playhead.
    fn settle(&mut self, timeline: &TimelineResponse) {
        if !timeline.interacting {
            self.history.commit(&self.project);
        }
        if let Some(id) = self.timeline.selected_clip
            && self.project.clip(id).is_none()
        {
            self.timeline.selected_clip = None;
        }
        // Scrubbing leaves the project untouched, so its frames are worth rendering mid-drag.
        if !timeline.interacting || timeline.playhead_moved {
            self.previewer.request(
                &self.ffmpeg,
                &self.project,
                self.media.infos(),
                self.timeline.playhead,
            );
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let title = window_title(self.project_path.as_deref(), self.is_dirty());
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn is_dirty(&self) -> bool {
        self.project != self.saved
    }
}

fn check_ffmpeg(ffmpeg: &Ffmpeg) -> Result<String, String> {
    ffmpeg.version().map_err(|e| format!("{e:#}"))
}

fn window_title(path: Option<&Path>, dirty: bool) -> String {
    let dirty = if dirty { "*" } else { "" };
    format!("ffmpeditor — {}{dirty}", display_name(path))
}

fn display_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_name)
        .map_or("untitled".to_owned(), |name| {
            name.to_string_lossy().into_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_shows_file_and_dirty_mark() {
        assert_eq!(window_title(None, false), "ffmpeditor — untitled");
        assert_eq!(window_title(None, true), "ffmpeditor — untitled*");
        assert_eq!(
            window_title(Some(Path::new("D:/proj/match.toml")), true),
            "ffmpeditor — match.toml*"
        );
        assert_eq!(
            window_title(Some(Path::new("match.toml")), false),
            "ffmpeditor — match.toml"
        );
    }
}
