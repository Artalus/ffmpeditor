use std::path::{Path, PathBuf};

use anyhow::bail;

use super::App;
use super::history::History;
use crate::media::MediaInfo;
use crate::model::{Project, Track};
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
        let placed = self
            .media
            .add(id, path)
            .and_then(|info| placement(&info))
            .map(|(track, duration)| {
                self.project.append_clip(id, track, duration);
            });
        if placed.is_err() && !known {
            self.project.sources.retain(|s| s.id != id);
        }
        placed
    }
}

/// Track and length of a freshly imported clip: video goes to V1 (bringing its audio along), audio-only to A1.
fn placement(info: &MediaInfo) -> anyhow::Result<(Track, f64)> {
    if info.duration <= 0.0 {
        bail!("unknown duration");
    }
    let track = match (&info.video, info.has_audio) {
        (Some(_), _) => Track::Video(0),
        (None, true) => Track::Audio(0),
        (None, false) => bail!("no video or audio stream"),
    };
    Ok((track, info.duration))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::VideoInfo;

    fn info(video: bool, has_audio: bool, duration: f64) -> MediaInfo {
        MediaInfo {
            duration,
            video: video.then_some(VideoInfo {
                width: 1920,
                height: 1080,
                fps: 60.0,
            }),
            has_audio,
        }
    }

    #[test]
    fn video_goes_to_first_video_track() {
        let (track, duration) = placement(&info(true, true, 12.5)).unwrap();
        assert_eq!((track, duration), (Track::Video(0), 12.5));
        let (track, _) = placement(&info(true, false, 1.0)).unwrap();
        assert_eq!(track, Track::Video(0));
    }

    #[test]
    fn audio_only_goes_to_first_audio_track() {
        let (track, _) = placement(&info(false, true, 3.0)).unwrap();
        assert_eq!(track, Track::Audio(0));
    }

    #[test]
    fn unusable_media_is_rejected() {
        assert!(placement(&info(false, false, 3.0)).is_err());
        assert!(placement(&info(true, true, 0.0)).is_err());
    }
}
