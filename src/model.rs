use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

const MIN_CLIP_DURATION: f64 = 0.04;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub settings: ProjectSettings,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub video_codec: String,
    pub video_bitrate_kbps: u32,
    pub audio_codec: String,
    pub audio_bitrate_kbps: u32,
    /// Container as file extension, e.g. "mp4"; also the default export extension.
    pub format: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClipId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub path: PathBuf,
}

/// On a video track a clip contributes its video and its audio (so audio follows video);
/// on an audio track only its audio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub source: SourceId,
    pub track: Track,
    /// Timeline position.
    pub start: f64,
    /// Offset into the source.
    pub source_in: f64,
    pub duration: f64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub video: VideoProps,
    #[serde(default, skip_serializing_if = "is_default")]
    pub audio: AudioProps,
}

/// Serialized as "V1", "A2", ... (1-based) to stay readable in project files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Track {
    Video(u32),
    Audio(u32),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoProps {
    pub fade_in: f64,
    pub fade_out: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioProps {
    pub muted: bool,
    pub volume: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    /// In source time, so regions stick to the content when the clip is moved or trimmed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mute_regions: Vec<Span>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

impl Project {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }

    pub fn duration(&self) -> f64 {
        latest_end(self.clips.iter())
    }

    pub fn source(&self, id: SourceId) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == id)
    }

    pub fn clips_on(&self, track: Track) -> impl Iterator<Item = &Clip> {
        self.clips.iter().filter(move |c| c.track == track)
    }

    fn track_end(&self, track: Track) -> f64 {
        latest_end(self.clips_on(track))
    }

    pub fn add_source(&mut self, path: PathBuf) -> SourceId {
        if let Some(existing) = self.sources.iter().find(|s| s.path == path) {
            return existing.id;
        }
        let id = SourceId(next_id(self.sources.iter().map(|s| s.id.0)));
        self.sources.push(Source { id, path });
        id
    }

    pub fn append_clip(&mut self, source: SourceId, track: Track, source_duration: f64) -> ClipId {
        let id = self.next_clip_id();
        let start = self.track_end(track);
        self.clips
            .push(Clip::new(id, source, track, start, source_duration));
        id
    }

    pub fn remove_clip(&mut self, id: ClipId) {
        self.clips.retain(|c| c.id != id);
    }

    pub fn move_clip(&mut self, id: ClipId, start: f64, track: Track) {
        if let Some(clip) = self.clip_mut(id) {
            clip.start = start.max(0.0);
            clip.track = track;
        }
    }

    /// Splits at timeline time `at`; returns the id of the new right-hand part.
    pub fn split_clip(&mut self, id: ClipId, at: f64) -> Option<ClipId> {
        let new_id = self.next_clip_id();
        let clip = self.clip_mut(id)?;
        let left_duration = at - clip.start;
        if left_duration < MIN_CLIP_DURATION || clip.duration - left_duration < MIN_CLIP_DURATION {
            return None;
        }
        let mut right = clip.clone();
        right.id = new_id;
        right.start = at;
        right.source_in += left_duration;
        right.duration -= left_duration;
        right.video.fade_in = 0.0;
        right.audio.fade_in = 0.0;
        clip.duration = left_duration;
        clip.video.fade_out = 0.0;
        clip.audio.fade_out = 0.0;
        self.clips.push(right);
        Some(new_id)
    }

    pub fn trim_clip_start(&mut self, id: ClipId, start: f64) {
        let Some(clip) = self.clip_mut(id) else {
            return;
        };
        let end = clip.end();
        let earliest = (clip.start - clip.source_in).max(0.0);
        let start = start.min(end - MIN_CLIP_DURATION).max(earliest);
        clip.source_in += start - clip.start;
        clip.start = start;
        clip.duration = end - start;
    }

    pub fn trim_clip_end(&mut self, id: ClipId, end: f64, source_duration: f64) {
        let Some(clip) = self.clip_mut(id) else {
            return;
        };
        let latest = clip.start + (source_duration - clip.source_in);
        let end = end.min(latest).max(clip.start + MIN_CLIP_DURATION);
        clip.duration = end - clip.start;
    }

    /// Overlap of the clip's head with an earlier clip on the same track.
    /// Overlaps are rendered as crossfades.
    pub fn overlap_before(&self, id: ClipId) -> f64 {
        self.overlap(id, true)
    }

    pub fn overlap_after(&self, id: ClipId) -> f64 {
        self.overlap(id, false)
    }

    fn overlap(&self, id: ClipId, head: bool) -> f64 {
        let Some(clip) = self.clip(id) else {
            return 0.0;
        };
        self.clips_on(clip.track)
            .map(|other| if head { (other, clip) } else { (clip, other) })
            .filter(|(earlier, later)| earlier.starts_before(later))
            .map(|(earlier, later)| (earlier.end().min(later.end()) - later.start).max(0.0))
            .fold(0.0, f64::max)
    }

    fn next_clip_id(&self) -> ClipId {
        ClipId(next_id(self.clips.iter().map(|c| c.id.0)))
    }
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 60.0,
            video_codec: "libx264".into(),
            video_bitrate_kbps: 12_000,
            audio_codec: "aac".into(),
            audio_bitrate_kbps: 192,
            format: "mp4".into(),
        }
    }
}

impl Clip {
    pub fn new(id: ClipId, source: SourceId, track: Track, start: f64, duration: f64) -> Self {
        Self {
            id,
            source,
            track,
            start,
            source_in: 0.0,
            duration,
            video: VideoProps::default(),
            audio: AudioProps::default(),
        }
    }

    pub fn end(&self) -> f64 {
        self.start + self.duration
    }

    pub fn span(&self) -> Span {
        Span {
            start: self.start,
            end: self.end(),
        }
    }

    pub fn source_out(&self) -> f64 {
        self.source_in + self.duration
    }

    pub fn timeline_to_source(&self, t: f64) -> f64 {
        t - self.start + self.source_in
    }

    pub fn source_to_timeline(&self, t: f64) -> f64 {
        t - self.source_in + self.start
    }

    fn starts_before(&self, other: &Clip) -> bool {
        (self.start, self.id) < (other.start, other.id)
    }
}

impl Default for AudioProps {
    fn default() -> Self {
        Self {
            muted: false,
            volume: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            mute_regions: Vec::new(),
        }
    }
}

impl Span {
    pub fn new(start: f64, end: f64) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
        }
    }

    pub fn len(&self) -> f64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.len() <= 0.0
    }

    pub fn contains(&self, t: f64) -> bool {
        self.start <= t && t < self.end
    }

    pub fn intersect(&self, other: Span) -> Option<Span> {
        let span = Span {
            start: self.start.max(other.start),
            end: self.end.min(other.end),
        };
        (!span.is_empty()).then_some(span)
    }
}

impl Track {
    pub fn index(self) -> u32 {
        match self {
            Track::Video(i) | Track::Audio(i) => i,
        }
    }

    pub fn is_video(self) -> bool {
        matches!(self, Track::Video(_))
    }
}

impl fmt::Display for Track {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Track::Video(i) => write!(f, "V{}", i + 1),
            Track::Audio(i) => write!(f, "A{}", i + 1),
        }
    }
}

impl FromStr for Track {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (kind, number) = s.split_at_checked(1).context("empty track name")?;
        let number: u32 = number
            .parse()
            .with_context(|| format!("bad track name {s:?}"))?;
        if number == 0 {
            bail!("track numbers start at 1: {s:?}");
        }
        match kind {
            "V" | "v" => Ok(Track::Video(number - 1)),
            "A" | "a" => Ok(Track::Audio(number - 1)),
            _ => bail!("track name must start with V or A: {s:?}"),
        }
    }
}

impl From<Track> for String {
    fn from(track: Track) -> Self {
        track.to_string()
    }
}

impl TryFrom<String> for Track {
    type Error = anyhow::Error;

    fn try_from(s: String) -> anyhow::Result<Self> {
        s.parse()
    }
}

fn latest_end<'a>(clips: impl Iterator<Item = &'a Clip>) -> f64 {
    clips.map(Clip::end).fold(0.0, f64::max)
}

fn next_id(ids: impl Iterator<Item = u64>) -> u64 {
    ids.max().map_or(1, |id| id + 1)
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with_clip(duration: f64) -> (Project, ClipId) {
        let mut project = Project::default();
        let source = project.add_source("a.mp4".into());
        let clip = project.append_clip(source, Track::Video(0), duration);
        (project, clip)
    }

    #[test]
    fn toml_roundtrip() {
        let (mut project, clip) = project_with_clip(10.0);
        let c = project.clip_mut(clip).unwrap();
        c.audio.mute_regions = vec![Span::new(1.0, 2.0)];
        c.video.fade_in = 0.5;
        let text = toml::to_string_pretty(&project).unwrap();
        assert!(text.contains("track = \"V1\""), "{text}");
        let parsed: Project = toml::from_str(&text).unwrap();
        assert_eq!(parsed, project);
    }

    #[test]
    fn default_props_are_not_serialized() {
        let (project, _) = project_with_clip(10.0);
        let text = toml::to_string_pretty(&project).unwrap();
        assert!(!text.contains("[clips.audio]"), "{text}");
        assert!(!text.contains("[clips.video]"), "{text}");
    }

    #[test]
    fn track_names() {
        assert_eq!("V1".parse::<Track>().unwrap(), Track::Video(0));
        assert_eq!("a3".parse::<Track>().unwrap(), Track::Audio(2));
        assert_eq!(Track::Audio(1).to_string(), "A2");
        assert!("V0".parse::<Track>().is_err());
        assert!("X1".parse::<Track>().is_err());
        assert!("".parse::<Track>().is_err());
    }

    #[test]
    fn add_source_dedupes_by_path() {
        let mut project = Project::default();
        let a = project.add_source("a.mp4".into());
        let b = project.add_source("b.mp4".into());
        assert_ne!(a, b);
        assert_eq!(project.add_source("a.mp4".into()), a);
    }

    #[test]
    fn append_places_clips_back_to_back() {
        let (mut project, first) = project_with_clip(10.0);
        let source = project.clip(first).unwrap().source;
        let second = project.append_clip(source, Track::Video(0), 5.0);
        assert_eq!(project.clip(second).unwrap().start, 10.0);
        assert_eq!(project.duration(), 15.0);
        let audio = project.append_clip(source, Track::Audio(0), 3.0);
        assert_eq!(project.clip(audio).unwrap().start, 0.0);
    }

    #[test]
    fn split_keeps_content_continuous() {
        let (mut project, left) = project_with_clip(10.0);
        let c = project.clip_mut(left).unwrap();
        c.source_in = 2.0;
        c.video.fade_in = 1.0;
        c.video.fade_out = 1.0;
        let right = project.split_clip(left, 4.0).unwrap();
        let (l, r) = (project.clip(left).unwrap(), project.clip(right).unwrap());
        assert_eq!((l.start, l.duration, l.source_in), (0.0, 4.0, 2.0));
        assert_eq!((r.start, r.duration, r.source_in), (4.0, 6.0, 6.0));
        assert_eq!((l.video.fade_in, l.video.fade_out), (1.0, 0.0));
        assert_eq!((r.video.fade_in, r.video.fade_out), (0.0, 1.0));
    }

    #[test]
    fn split_at_edge_is_rejected() {
        let (mut project, clip) = project_with_clip(10.0);
        assert!(project.split_clip(clip, 0.0).is_none());
        assert!(project.split_clip(clip, 10.0).is_none());
        assert!(project.split_clip(clip, 11.0).is_none());
        assert_eq!(project.clips.len(), 1);
    }

    #[test]
    fn trim_start_is_bounded_by_source() {
        let (mut project, clip) = project_with_clip(10.0);
        project.move_clip(clip, 5.0, Track::Video(0));
        project.trim_clip_start(clip, 7.0);
        let c = project.clip(clip).unwrap();
        assert_eq!((c.start, c.source_in, c.duration), (7.0, 2.0, 8.0));
        project.trim_clip_start(clip, 0.0);
        let c = project.clip(clip).unwrap();
        assert_eq!((c.start, c.source_in, c.duration), (5.0, 0.0, 10.0));
    }

    #[test]
    fn trim_end_is_bounded_by_source() {
        let (mut project, clip) = project_with_clip(10.0);
        project.trim_clip_end(clip, 4.0, 10.0);
        assert_eq!(project.clip(clip).unwrap().duration, 4.0);
        project.trim_clip_end(clip, 20.0, 10.0);
        assert_eq!(project.clip(clip).unwrap().duration, 10.0);
        project.trim_clip_end(clip, -1.0, 10.0);
        assert_eq!(project.clip(clip).unwrap().duration, MIN_CLIP_DURATION);
    }

    #[test]
    fn trim_keeps_degenerate_clips_valid() {
        let (mut project, clip) = project_with_clip(0.01);
        project.trim_clip_start(clip, 5.0);
        let c = project.clip(clip).unwrap();
        assert_eq!((c.start, c.source_in, c.duration), (0.0, 0.0, 0.01));
        project.trim_clip_end(clip, 5.0, 0.0);
        assert_eq!(project.clip(clip).unwrap().duration, MIN_CLIP_DURATION);

        // The source got shorter than the clip's `source_in`.
        let (mut project, clip) = project_with_clip(2.0);
        project.clip_mut(clip).unwrap().source_in = 5.0;
        project.trim_clip_end(clip, 1.0, 3.0);
        assert_eq!(project.clip(clip).unwrap().duration, MIN_CLIP_DURATION);
    }

    #[test]
    fn overlaps_on_same_track() {
        let (mut project, first) = project_with_clip(10.0);
        let source = project.clip(first).unwrap().source;
        let second = project.append_clip(source, Track::Video(0), 10.0);
        project.move_clip(second, 8.0, Track::Video(0));
        let other_track = project.append_clip(source, Track::Video(1), 10.0);
        assert_eq!(project.overlap_after(first), 2.0);
        assert_eq!(project.overlap_before(second), 2.0);
        assert_eq!(project.overlap_before(first), 0.0);
        assert_eq!(project.overlap_after(second), 0.0);
        assert_eq!(project.overlap_before(other_track), 0.0);
    }

    #[test]
    fn span_intersection() {
        let a = Span::new(0.0, 5.0);
        assert_eq!(a.intersect(Span::new(3.0, 8.0)), Some(Span::new(3.0, 5.0)));
        assert_eq!(a.intersect(Span::new(5.0, 8.0)), None);
        assert_eq!(Span::new(4.0, 1.0), Span::new(1.0, 4.0));
    }
}
