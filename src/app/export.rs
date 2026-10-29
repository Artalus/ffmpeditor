use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use egui::Context;

use super::{App, Dialog};
use crate::ffmpeg::Ffmpeg;
use crate::media::MediaInfos;
use crate::model::Project;
use crate::render::{ExportJob, ExportStatus, export_args};
use crate::ui::export::{ExportDialog, ExportRequest, ProgressAction, default_output, progress};

pub struct RunningExport {
    job: ExportJob,
    output: PathBuf,
    started: Instant,
    /// Set once the job is seen ended, so the shown time stops growing.
    took: Option<Duration>,
}

impl App {
    pub(super) fn open_export_dialog(&mut self) {
        if self.export_running() {
            self.report("An export is already running.".to_owned());
            return;
        }
        self.export = None;
        let user_dirs = directories::UserDirs::new();
        let output = default_output(
            self.project_path.as_deref(),
            self.last_export.as_deref(),
            user_dirs.as_ref().and_then(|dirs| dirs.video_dir()),
            &self.project.settings.format,
        );
        self.dialog = Dialog::Export(ExportDialog::new(&output, self.timeline.selection));
    }

    /// Progress window of the running export, kept open with the result once it ends.
    pub(super) fn export_progress(&mut self, ctx: &Context) {
        let Some(export) = &mut self.export else {
            return;
        };
        let status = export.job.status();
        if matches!(status, ExportStatus::Running { .. }) {
            // The job has no egui context to wake the UI with.
            ctx.request_repaint_after(Duration::from_millis(250));
        } else {
            export.took.get_or_insert_with(|| export.started.elapsed());
        }
        let elapsed = export.took.unwrap_or_else(|| export.started.elapsed());
        match progress(ctx, &status, &export.output, elapsed) {
            Some(ProgressAction::Cancel) => export.job.cancel(),
            Some(ProgressAction::Close) => self.export = None,
            Some(ProgressAction::ShowFile) => {
                if let Err(err) = show_in_file_manager(&export.output) {
                    self.report(format!("Opening file manager: {err}"));
                }
            }
            None => {}
        }
    }

    pub(super) fn export_fraction(&self) -> Option<f32> {
        match self.export.as_ref()?.job.status() {
            ExportStatus::Running { progress } => Some(progress),
            _ => None,
        }
    }

    pub(super) fn export_running(&self) -> bool {
        self.export_fraction().is_some()
    }
}

pub fn start_export(
    ffmpeg: &Ffmpeg,
    project: &Project,
    infos: &MediaInfos,
    request: ExportRequest,
) -> anyhow::Result<RunningExport> {
    let args = export_args(project, infos, request.range, &request.output);
    let job = ExportJob::start(ffmpeg, args, request.range.len())?;
    Ok(RunningExport {
        job,
        output: request.output,
        started: Instant::now(),
        took: None,
    })
}

fn show_in_file_manager(file: &Path) -> std::io::Result<()> {
    file_manager_command(file).spawn().map(drop)
}

#[cfg(windows)]
fn file_manager_command(file: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new("explorer");
    // Explorer parses /select itself; the quoting std adds around a plain arg breaks it.
    cmd.raw_arg(format!("/select,\"{}\"", file.display()));
    cmd
}

#[cfg(target_os = "macos")]
fn file_manager_command(file: &Path) -> Command {
    let mut cmd = Command::new("open");
    cmd.arg("-R").arg(file);
    cmd
}

#[cfg(not(any(windows, target_os = "macos")))]
fn file_manager_command(file: &Path) -> Command {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(file.parent().unwrap_or(file));
    cmd
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn explorer_gets_the_path_unescaped_in_quotes() {
        let cmd = file_manager_command(Path::new(r"D:\out\my clip.mp4"));
        assert_eq!(
            cmd.get_args().collect::<Vec<_>>(),
            [r#"/select,"D:\out\my clip.mp4""#]
        );
    }
}
