use std::path::{Path, PathBuf};

use super::App;
use super::history::History;
use crate::model::{Placement, Project, SourceId, Span, Track};
use crate::ui::timeline::TimelineState;
use crate::ui::{file_name, with_default_ext};

const PROJECT_EXT: &str = "toml";
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "mov", "avi", "webm", "flv", "ts"];
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "m4a", "opus", "aac"];

impl App {
    pub(super) fn new_project(&mut self) {
        self.load_project(Project::default(), None);
    }

    pub(super) fn open(&mut self) {
        let Some(path) = self
            .file_dialog()
            .add_filter("ffmpeditor project", &[PROJECT_EXT])
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
            .and_then(file_name)
            .unwrap_or_else(|| format!("project.{PROJECT_EXT}"));
        let Some(path) = self
            .file_dialog()
            .add_filter("ffmpeditor project", &[PROJECT_EXT])
            .set_file_name(name)
            .save_file()
        else {
            return false;
        };
        self.save_to(with_default_ext(path, PROJECT_EXT))
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
        let mut groups = [true, false].map(|video| ImportGroup::new(video, self.timeline.playhead));
        for path in paths {
            match self.probe_import(&path) {
                Ok((video, source, duration, new)) => {
                    let group = &mut groups[usize::from(!video)];
                    group.items.push((source, duration));
                    group.new_sources.extend(new.then_some(source));
                }
                Err(err) => self.report(format!("{}: {err:#}", path.display())),
            }
        }
        for group in groups {
            self.place(group);
        }
    }

    /// Adds the source and probes it; a failed probe leaves no new source behind.
    /// Returns (is video, source, duration, source is new).
    fn probe_import(&mut self, path: &Path) -> anyhow::Result<(bool, SourceId, f64, bool)> {
        let known = self.project.sources.iter().any(|s| s.path == path);
        let id = self.project.add_source(path.to_path_buf());
        let probed = self
            .media
            .add(id, path)
            .and_then(|info| Ok((info.is_video()?, id, info.duration, !known)));
        if probed.is_err() && !known {
            self.project.remove_source(id);
        }
        probed
    }

    /// Places the group where it fits right away, or queues a confirmation for another track.
    fn place(&mut self, mut group: ImportGroup) {
        if group.items.is_empty() {
            return;
        }
        let video = group.video;
        let first = if video {
            Track::Video(0)
        } else {
            Track::Audio(0)
        };
        let from = self
            .timeline
            .selected_track
            .filter(|t| t.is_video() == video && t.index() < self.project.tracks.count(video))
            .unwrap_or(first);
        let total: f64 = group.items.iter().map(|(_, d)| d).sum();
        match self
            .project
            .placement(from, Span::new(group.start, group.start + total))
        {
            Placement::Free(track) => self.insert_group(&group, track),
            Placement::Above(target) => {
                (group.from, group.target, group.create) = (from, target, false);
                self.placements.push_back(group);
            }
            Placement::NewTrack => {
                let target = from.with_index(self.project.tracks.count(video));
                (group.from, group.target, group.create) = (from, target, true);
                self.placements.push_back(group);
            }
        }
    }

    pub(super) fn finish_placement(&mut self, group: ImportGroup, accepted: bool) {
        if !accepted {
            for id in group.new_sources {
                if self.project.clips.iter().all(|c| c.source != id) {
                    self.project.remove_source(id);
                }
            }
            return;
        }
        if group.create {
            self.project.add_track(group.video);
        }
        self.insert_group(&group, group.target);
    }

    fn insert_group(&mut self, group: &ImportGroup, track: Track) {
        let mut start = group.start;
        for &(source, duration) in &group.items {
            self.project.insert_clip(source, track, start, duration);
            start += duration;
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
}

/// Probed files of one kind, placed back to back from `start`.
pub struct ImportGroup {
    video: bool,
    start: f64,
    /// (source, duration)
    items: Vec<(SourceId, f64)>,
    /// Sources this import added to the project, dropped again if the placement is cancelled.
    new_sources: Vec<SourceId>,
    /// Track the group was aimed at, and the one the confirmation offers instead.
    pub from: Track,
    pub target: Track,
    /// The target track does not exist yet.
    pub create: bool,
}

impl ImportGroup {
    fn new(video: bool, start: f64) -> Self {
        let track = if video {
            Track::Video(0)
        } else {
            Track::Audio(0)
        };
        Self {
            video,
            start,
            items: Vec::new(),
            new_sources: Vec::new(),
            from: track,
            target: track,
            create: false,
        }
    }
}
