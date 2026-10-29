use std::path::{Path, PathBuf};

use super::App;
use super::history::History;
use crate::model::Project;
use crate::ui::timeline::TimelineState;

const PROJECT_EXTENSIONS: &[&str] = &["toml"];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "mov", "avi", "webm", "flv", "ts"];
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "m4a", "opus", "aac"];

impl App {
    pub(super) fn new_project(&mut self) {
        self.load_project(Project::default(), None);
    }

    pub(super) fn open(&mut self) {
        let Some(path) = self
            .file_dialog()
            .add_filter("ffmpeditor project", PROJECT_EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        match Project::load(&path) {
            Ok(project) => self.load_project(project, Some(path)),
            Err(err) => self.report(format!("{err:#}")),
        }
    }

    /// Saves to the current file, asking for one if there is none; false if nothing was saved.
    pub(super) fn save(&mut self) -> bool {
        match self.project_path.clone() {
            Some(path) => self.save_to(path),
            None => self.save_as(),
        }
    }

    pub(super) fn save_as(&mut self) -> bool {
        let name = self
            .project_path
            .as_deref()
            .and_then(Path::file_name)
            .map_or("project.toml".to_owned(), |name| {
                name.to_string_lossy().into_owned()
            });
        let Some(mut path) = self
            .file_dialog()
            .add_filter("ffmpeditor project", PROJECT_EXTENSIONS)
            .set_file_name(name)
            .save_file()
        else {
            return false;
        };
        // Not every platform's dialog appends the filter's extension to a bare name.
        if path.extension().is_none() {
            path.set_extension(PROJECT_EXTENSIONS[0]);
        }
        self.save_to(path)
    }

    pub(super) fn import(&mut self) {
        let Some(paths) = self
            .file_dialog()
            .set_title("Import media")
            .add_filter("Video", VIDEO_EXTENSIONS)
            .add_filter("Audio", AUDIO_EXTENSIONS)
            .add_filter("All files", &["*"])
            .pick_files()
        else {
            return;
        };
        for path in paths {
            if let Err(err) = self.import_file(&path) {
                self.report(format!("{}: {err:#}", path.display()));
            }
        }
    }

    /// Probes sources the media cache does not know yet; failures are reported, the sources stay.
    pub(super) fn probe_sources(&mut self) {
        let mut failures = Vec::new();
        for source in &self.project.sources {
            if self.media.info(source.id).is_none()
                && let Err(err) = self.media.add(source.id, &source.path)
            {
                failures.push(format!("{}: {err:#}", source.path.display()));
            }
        }
        for failure in failures {
            self.report(failure);
        }
    }

    fn load_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.media.clear();
        self.history = History::new(&project);
        self.saved = project.clone();
        self.project = project;
        self.project_path = path;
        self.timeline = TimelineState::default();
        self.last_export = None;
        self.probe_sources();
    }

    fn file_dialog(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match self.project_path.as_deref().and_then(Path::parent) {
            Some(dir) if dir.is_dir() => dialog.set_directory(dir),
            _ => dialog,
        }
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        match self.project.save(&path) {
            Ok(()) => {
                self.saved = self.project.clone();
                self.project_path = Some(path);
                true
            }
            Err(err) => {
                self.report(format!("{err:#}"));
                false
            }
        }
    }

    fn import_file(&mut self, path: &Path) -> anyhow::Result<()> {
        let known = self.project.sources.iter().any(|s| s.path == path);
        let id = self.project.add_source(path.to_path_buf());
        let placed = self.media.add(id, path).and_then(|info| {
            let track = info.default_track()?;
            self.project.append_clip(id, track, info.duration);
            Ok(())
        });
        if placed.is_err() && !known {
            self.project.remove_source(id);
        }
        placed
    }
}
