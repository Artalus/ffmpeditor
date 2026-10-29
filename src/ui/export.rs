use std::path::{Path, PathBuf};
use std::process::Command;

use egui::{Align2, CollapsingHeader, Context, ProgressBar, ScrollArea, TextEdit};

use crate::ffmpeg::Ffmpeg;
use crate::model::{Project, Span};
use crate::render::{ExportStatus, MediaInfos, export_args};
use crate::ui::dialogs::{Outcome, confirm_buttons, modal};
use crate::ui::status_bar::format_time;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Range {
    Timeline,
    Selection,
}

pub struct ExportDialog {
    range: Range,
    output: String,
    /// Why the last start attempt failed; the dialog stays open so the user can fix it.
    error: Option<String>,
}

pub struct ExportRequest {
    pub range: Span,
    pub output: PathBuf,
}

pub enum ProgressAction {
    Cancel,
    Close,
}

impl ExportDialog {
    pub fn new(output: &Path, selection: Option<Span>) -> Self {
        Self {
            range: if selection.is_some() {
                Range::Selection
            } else {
                Range::Timeline
            },
            output: output.display().to_string(),
            error: None,
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        project: &Project,
        infos: &MediaInfos,
        ffmpeg: &Ffmpeg,
        selection: Option<Span>,
    ) -> Outcome<ExportRequest> {
        modal(ctx, "Export", |ui| {
            if selection.is_none() {
                self.range = Range::Timeline;
            }
            ui.horizontal(|ui| {
                ui.label("Range");
                let whole = format!("Whole timeline ({})", format_time(project.duration()));
                ui.radio_value(&mut self.range, Range::Timeline, whole);
                let selected = match selection {
                    Some(span) => format!("Selection ({})", format_time(span.len())),
                    None => "Selection".to_owned(),
                };
                ui.add_enabled_ui(selection.is_some(), |ui| {
                    ui.radio_value(&mut self.range, Range::Selection, selected);
                });
            });
            ui.horizontal(|ui| {
                ui.label("Output");
                ui.add(TextEdit::singleline(&mut self.output).desired_width(360.0));
                if ui.button("Browse…").clicked()
                    && let Some(path) =
                        pick_output(Path::new(&self.output), &project.settings.format)
                {
                    self.output = path.display().to_string();
                }
            });

            let request = self.request(project.duration(), selection);
            CollapsingHeader::new("ffmpeg command").show(ui, |ui| match &request {
                Ok(request) => {
                    let mut cmd = ffmpeg.ffmpeg();
                    cmd.args(export_args(project, infos, request.range, &request.output));
                    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        ui.add(
                            TextEdit::multiline(&mut command_line(&cmd).as_str())
                                .code_editor()
                                .desired_width(560.0),
                        );
                    });
                }
                Err(reason) => {
                    ui.weak(*reason);
                }
            });
            if let Err(reason) = &request {
                ui.colored_label(ui.visuals().warn_fg_color, *reason);
            }
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            match confirm_buttons(ui, "Start", request.is_ok()) {
                Some(true) => match request {
                    Ok(request) => Outcome::Accepted(request),
                    Err(_) => Outcome::Open,
                },
                Some(false) => Outcome::Cancelled,
                None => Outcome::Open,
            }
        })
    }

    fn request(
        &self,
        duration: f64,
        selection: Option<Span>,
    ) -> Result<ExportRequest, &'static str> {
        let range = resolve_range(self.range, duration, selection)?;
        let output = self.output.trim();
        if output.is_empty() {
            return Err("Choose an output file.");
        }
        Ok(ExportRequest {
            range,
            output: output.into(),
        })
    }
}

/// Progress of a running export, then its result until closed.
pub fn progress(ctx: &Context, status: &ExportStatus, output: &Path) -> Option<ProgressAction> {
    egui::Window::new("Export progress")
        .anchor(Align2::CENTER_TOP, [0.0, 48.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.label(output.display().to_string());
            match status {
                ExportStatus::Running { progress } => {
                    ui.add(ProgressBar::new(*progress).show_percentage().animate(true));
                    ui.button("Cancel")
                        .clicked()
                        .then_some(ProgressAction::Cancel)
                }
                ExportStatus::Finished => {
                    ui.label("Finished.");
                    close_button(ui)
                }
                ExportStatus::Cancelled => {
                    ui.label("Cancelled.");
                    close_button(ui)
                }
                ExportStatus::Failed(stderr) => {
                    ui.colored_label(ui.visuals().error_fg_color, "Export failed:");
                    ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        ui.add(
                            TextEdit::multiline(&mut stderr.as_str())
                                .code_editor()
                                .desired_width(560.0),
                        );
                    });
                    close_button(ui)
                }
            }
        })
        .and_then(|response| response.inner.flatten())
}

pub fn default_output(
    project_path: Option<&Path>,
    previous: Option<&Path>,
    fallback_dir: Option<&Path>,
    format: &str,
) -> PathBuf {
    if let Some(previous) = previous {
        return previous.with_extension(format);
    }
    if let Some(project) = project_path {
        return project.with_extension(format);
    }
    let name = format!("export.{format}");
    match fallback_dir {
        Some(dir) => dir.join(name),
        None => name.into(),
    }
}

fn pick_output(current: &Path, format: &str) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Export to")
        .add_filter(format, &[format]);
    if let Some(name) = current.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

fn resolve_range(
    range: Range,
    duration: f64,
    selection: Option<Span>,
) -> Result<Span, &'static str> {
    let span = match range {
        Range::Timeline => Span::new(0.0, duration),
        Range::Selection => selection.ok_or("Nothing is selected.")?,
    };
    if span.is_empty() {
        return Err(match range {
            Range::Timeline => "The timeline is empty.",
            Range::Selection => "The selection is empty.",
        });
    }
    Ok(span)
}

/// Shell-like rendering for copy-pasting into a terminal while debugging.
fn command_line(cmd: &Command) -> String {
    std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|arg| quote(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn close_button(ui: &mut egui::Ui) -> Option<ProgressAction> {
    ui.button("Close")
        .clicked()
        .then_some(ProgressAction::Close)
}

fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.,:/\\=+@%".contains(c));
    if plain {
        arg.to_owned()
    } else {
        format!("\"{}\"", arg.replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_defaults() {
        let previous = Path::new("D:/out/clip.mkv");
        let project = Path::new("D:/proj/match.toml");
        let videos = Path::new("C:/Users/me/Videos");
        assert_eq!(
            default_output(Some(project), Some(previous), Some(videos), "mp4"),
            Path::new("D:/out/clip.mp4")
        );
        assert_eq!(
            default_output(Some(project), None, Some(videos), "mp4"),
            Path::new("D:/proj/match.mp4")
        );
        assert_eq!(
            default_output(None, None, Some(videos), "webm"),
            Path::new("C:/Users/me/Videos/export.webm")
        );
        assert_eq!(
            default_output(None, None, None, "mp4"),
            Path::new("export.mp4")
        );
    }

    #[test]
    fn range_resolution() {
        let selection = Some(Span::new(2.0, 5.0));
        assert_eq!(
            resolve_range(Range::Timeline, 10.0, selection),
            Ok(Span::new(0.0, 10.0))
        );
        assert_eq!(
            resolve_range(Range::Selection, 10.0, selection),
            Ok(Span::new(2.0, 5.0))
        );
        assert!(resolve_range(Range::Selection, 10.0, None).is_err());
        assert!(resolve_range(Range::Timeline, 0.0, None).is_err());
        assert!(resolve_range(Range::Selection, 10.0, Some(Span::new(3.0, 3.0))).is_err());
    }

    #[test]
    fn command_is_quoted_where_needed() {
        let mut cmd = Command::new("C:/ffmpeg/bin/ffmpeg.exe");
        cmd.args([
            "-i",
            "D:/rec/match 1.mp4",
            "-filter_complex",
            "[0:v]scale=1920:1080[v]",
            "",
            "say \"hi\"",
        ]);
        assert_eq!(
            command_line(&cmd),
            r#"C:/ffmpeg/bin/ffmpeg.exe -i "D:/rec/match 1.mp4" -filter_complex "[0:v]scale=1920:1080[v]" "" "say \"hi\"""#
        );
    }
}
