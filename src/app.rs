mod commands;
mod editing;
mod export;
mod files;
mod history;

use std::path::{Path, PathBuf};

use egui::{Context, ViewportCommand};

use crate::config::{AppConfig, config_path};
use crate::ffmpeg::Ffmpeg;
use crate::media::MediaCache;
use crate::model::Project;
use crate::render::Previewer;
use crate::ui;
use crate::ui::dialogs::{Outcome, SettingsDialog, UnsavedChoice};
use crate::ui::export::ExportDialog;
use crate::ui::project_settings::ProjectSettingsDialog;
use crate::ui::timeline::{TimelineResponse, TimelineState};
use commands::{Command, MenuState};
use editing::step_frames;
use export::{RunningExport, start_export};
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
    /// The timeline was mid-gesture last frame; shortcuts would act on a half-finished edit.
    timeline_busy: bool,
    timeline: TimelineState,
    /// Kept after the job ends so its result stays visible until dismissed.
    export: Option<RunningExport>,
    last_export: Option<PathBuf>,
    dialog: Dialog,
    /// The user already agreed to drop unsaved changes, so the next close request goes through.
    quit_confirmed: bool,
    /// Config file that failed to load; saving settings would overwrite it.
    unreadable_config: Option<PathBuf>,
    errors: Vec<String>,
    /// Last title sent to the window, to avoid resending it every frame.
    title: String,
}

/// At most one modal dialog is open at a time.
enum Dialog {
    None,
    Unsaved(Guarded),
    Settings(SettingsDialog),
    /// Settings accepted while the config file on disk is unreadable.
    OverwriteConfig(AppConfig, PathBuf),
    ProjectSettings(ProjectSettingsDialog),
    Export(ExportDialog),
}

/// Actions that would drop unsaved changes.
#[derive(Clone, Copy)]
enum Guarded {
    New,
    Open,
    Quit,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut errors = Vec::new();
        let mut unreadable_config = None;
        let config = AppConfig::load().unwrap_or_else(|err| {
            errors.push(format!("{err:#}; using defaults"));
            unreadable_config = config_path();
            AppConfig::default()
        });
        let ffmpeg = Ffmpeg::new(&config.ffmpeg_path);
        let ffmpeg_status = check_ffmpeg(&ffmpeg);
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
            timeline_busy: false,
            timeline: TimelineState::default(),
            export: None,
            last_export: None,
            dialog: Dialog::None,
            quit_confirmed: false,
            unreadable_config,
            errors,
            title: String::new(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.media.poll();
        self.previewer.poll();
        self.handle_close_request(ui.ctx());
        for command in self.commands(ui) {
            self.run(command, ui.ctx());
        }
        // Before the panels, so what a dialog changes shows up and gets committed this frame.
        self.dialogs(ui.ctx());
        self.export_progress(ui.ctx());
        self.status_bar(ui);
        let timeline = self.timeline_panel(ui);
        self.timeline_busy = timeline.interacting;
        self.viewer(ui);
        self.settle(&timeline);
        ui::dialogs::errors(ui.ctx(), &mut self.errors);
        self.update_title(ui.ctx());
    }
}

impl App {
    fn handle_close_request(&mut self, ctx: &Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.export_running() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.report("An export is running; cancel it before quitting.".to_owned());
            return;
        }
        if self.quit_confirmed || !self.is_dirty() {
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        self.dialog = Dialog::Unsaved(Guarded::Quit);
    }

    fn commands(&self, ui: &mut egui::Ui) -> Vec<Command> {
        let shortcuts_allowed = matches!(self.dialog, Dialog::None)
            && !self.timeline_busy
            && !ui.ctx().text_edit_focused();
        let mut commands = if shortcuts_allowed {
            commands::read_shortcuts(ui.ctx())
        } else {
            Vec::new()
        };
        let state = MenuState {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            clip_selected: self.timeline.selected_clip.is_some(),
            export_running: self.export_running(),
        };
        commands.extend(commands::menu_bar(ui, &state));
        commands
    }

    fn run(&mut self, command: Command, ctx: &Context) {
        let playhead = &mut self.timeline.playhead;
        let duration = self.project.duration();
        let fps = self.project.settings.fps;
        match command {
            Command::New => self.guard(Guarded::New, ctx),
            Command::Open => self.guard(Guarded::Open, ctx),
            Command::Save => {
                self.save();
            }
            Command::SaveAs => {
                self.save_as();
            }
            Command::Import => self.import(),
            Command::Export => self.open_export_dialog(),
            // Goes through handle_close_request, which owns the unsaved-changes check.
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Command::Undo => {
                self.history.undo(&mut self.project);
            }
            Command::Redo => {
                self.history.redo(&mut self.project);
            }
            Command::Split => {
                self.project
                    .split_at(self.timeline.playhead, self.timeline.selected_clip);
            }
            Command::DeleteClip => self.delete_selected_clip(),
            Command::GoToStart => *playhead = 0.0,
            Command::GoToEnd => *playhead = duration,
            Command::PreviousFrame => *playhead = step_frames(*playhead, fps, -1, duration),
            Command::NextFrame => *playhead = step_frames(*playhead, fps, 1, duration),
            Command::ProjectSettings => {
                let dialog = ProjectSettingsDialog::new(&self.project.settings);
                self.dialog = Dialog::ProjectSettings(dialog);
            }
            Command::Settings => self.dialog = Dialog::Settings(SettingsDialog::new(&self.config)),
        }
    }

    fn guard(&mut self, action: Guarded, ctx: &Context) {
        if self.is_dirty() {
            self.dialog = Dialog::Unsaved(action);
        } else {
            self.perform(action, ctx);
        }
    }

    fn perform(&mut self, action: Guarded, ctx: &Context) {
        match action {
            Guarded::New => self.new_project(),
            Guarded::Open => self.open(),
            Guarded::Quit => {
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn delete_selected_clip(&mut self) {
        if let Some(id) = self.timeline.selected_clip.take() {
            self.project.remove_clip(id);
        }
    }

    fn dialogs(&mut self, ctx: &Context) {
        match &mut self.dialog {
            Dialog::None => {}
            Dialog::Unsaved(action) => {
                let action = *action;
                self.unsaved_changes(ctx, action);
            }
            Dialog::Settings(dialog) => match dialog.show(ctx) {
                Outcome::Open => {}
                Outcome::Cancelled => self.dialog = Dialog::None,
                Outcome::Accepted(config) => match self.unreadable_config.clone() {
                    Some(path) => self.dialog = Dialog::OverwriteConfig(config, path),
                    None => {
                        self.dialog = Dialog::None;
                        self.apply_config(config);
                    }
                },
            },
            Dialog::OverwriteConfig(config, path) => {
                match ui::dialogs::confirm_overwrite(ctx, path) {
                    Outcome::Open => {}
                    Outcome::Cancelled => self.dialog = Dialog::None,
                    Outcome::Accepted(()) => {
                        let config = config.clone();
                        self.dialog = Dialog::None;
                        self.apply_config(config);
                    }
                }
            }
            Dialog::ProjectSettings(dialog) => match dialog.show(ctx) {
                Outcome::Open => {}
                Outcome::Cancelled => self.dialog = Dialog::None,
                Outcome::Accepted(settings) => {
                    self.dialog = Dialog::None;
                    self.project.settings = settings;
                }
            },
            Dialog::Export(dialog) => {
                let selection = self.timeline.selection;
                let infos = self.media.infos();
                match dialog.show(ctx, &self.project, infos, &self.ffmpeg, selection) {
                    Outcome::Open => {}
                    Outcome::Cancelled => self.dialog = Dialog::None,
                    Outcome::Accepted(request) => {
                        let output = request.output.clone();
                        match start_export(&self.ffmpeg, &self.project, infos, request) {
                            Ok(export) => {
                                self.dialog = Dialog::None;
                                self.export = Some(export);
                                self.last_export = Some(output);
                            }
                            Err(err) => dialog.set_error(format!("{err:#}")),
                        }
                    }
                }
            }
        }
    }

    fn unsaved_changes(&mut self, ctx: &Context, action: Guarded) {
        let name = display_name(self.project_path.as_deref());
        let proceed = match ui::dialogs::unsaved_changes(ctx, &name) {
            Outcome::Open => return,
            Outcome::Cancelled => false,
            Outcome::Accepted(UnsavedChoice::Save) => self.save(),
            Outcome::Accepted(UnsavedChoice::Discard) => true,
        };
        self.dialog = Dialog::None;
        if proceed {
            self.perform(action, ctx);
        }
    }

    fn apply_config(&mut self, config: AppConfig) {
        match config.save() {
            Ok(()) => self.unreadable_config = None,
            Err(err) => self.report(format!("Saving settings: {err:#}")),
        }
        let ffmpeg_changed = config.ffmpeg_path != self.config.ffmpeg_path;
        self.config = config;
        self.ffmpeg = Ffmpeg::new(&self.config.ffmpeg_path);
        self.ffmpeg_status = check_ffmpeg(&self.ffmpeg);
        self.media.set_ffmpeg(self.ffmpeg.clone());
        // Sources that failed to probe with a missing ffmpeg may work now.
        if ffmpeg_changed {
            self.probe_sources();
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        let status = ui::status_bar::Status {
            playhead: self.timeline.playhead,
            duration: self.project.duration(),
            selection: self.timeline.selection,
            ffmpeg: &self.ffmpeg_status,
            export_progress: self.export_fraction(),
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

    fn report(&mut self, message: String) {
        if !self.errors.contains(&message) {
            self.errors.push(message);
        }
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
