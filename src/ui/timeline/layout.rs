//! Pure timeline geometry: screen regions, time <-> x mapping, track lanes, ruler ticks.

use egui::{Pos2, Rect, pos2};

use crate::model::{Clip, Project, Track};

const HEADER_WIDTH: f32 = 70.0;
const RULER_HEIGHT: f32 = 24.0;
const VIDEO_LANE_HEIGHT: f32 = 44.0;
const AUDIO_LANE_HEIGHT: f32 = 30.0;
const TRACK_GAP: f32 = 3.0;
const MIN_PIXELS_PER_SECOND: f32 = 0.5;
const MAX_PIXELS_PER_SECOND: f32 = 2000.0;

const MIN_LABEL_SPACING: f32 = 80.0;
const MIN_TICK_SPACING: f32 = 8.0;
const TICK_STEPS: [f64; 22] = [
    0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
    900.0, 1800.0, 3600.0, 7200.0, 18000.0, 36000.0,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Areas {
    pub corner: Rect,
    pub ruler: Rect,
    pub header: Rect,
    pub rows: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeAxis {
    /// Screen x where `scroll` is drawn.
    pub origin: f32,
    pub scroll: f64,
    pub pixels_per_second: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackCounts {
    video: u32,
    audio: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneKind {
    Video,
    Audio,
}

/// One horizontal strip; a video track has a video lane with its linked-audio lane beneath.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lane {
    pub track: Track,
    pub kind: LaneKind,
    /// Relative to the top of the rows content.
    top: f32,
    height: f32,
}

pub struct View {
    pub axis: TimeAxis,
    pub lanes: Vec<Lane>,
    pub rows: Rect,
    pub content_top: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ticks {
    major: f64,
    minor: f64,
}

impl Areas {
    pub fn new(rect: Rect) -> Self {
        let left = (rect.left() + HEADER_WIDTH).min(rect.right());
        let top = (rect.top() + RULER_HEIGHT).min(rect.bottom());
        Self {
            corner: Rect::from_min_max(rect.min, pos2(left, top)),
            ruler: Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right(), top)),
            header: Rect::from_min_max(pos2(rect.left(), top), pos2(left, rect.bottom())),
            rows: Rect::from_min_max(pos2(left, top), rect.max),
        }
    }

    pub fn all(&self) -> Rect {
        self.corner.union(self.rows)
    }
}

impl TimeAxis {
    pub fn x(&self, t: f64) -> f32 {
        self.origin + ((t - self.scroll) * self.pixels_per_second as f64) as f32
    }

    pub fn time(&self, x: f32) -> f64 {
        self.scroll + (x - self.origin) as f64 / self.pixels_per_second as f64
    }

    pub fn seconds(&self, pixels: f32) -> f64 {
        pixels as f64 / self.pixels_per_second as f64
    }

    /// Zooms by `factor` keeping the time under `anchor_x` in place.
    pub fn zoomed(&self, anchor_x: f32, factor: f32) -> TimeAxis {
        let anchor = self.time(anchor_x);
        let pixels_per_second =
            (self.pixels_per_second * factor).clamp(MIN_PIXELS_PER_SECOND, MAX_PIXELS_PER_SECOND);
        TimeAxis {
            origin: self.origin,
            scroll: anchor - (anchor_x - self.origin) as f64 / pixels_per_second as f64,
            pixels_per_second,
        }
    }
}

impl TrackCounts {
    pub fn of(project: &Project) -> Self {
        Self {
            video: project.tracks.video,
            audio: project.tracks.audio,
        }
    }
}

pub fn layout_lanes(counts: TrackCounts) -> Vec<Lane> {
    let mut lanes = Vec::new();
    let mut y = 0.0;
    let mut push = |track, kind, height, gap_after| {
        lanes.push(Lane {
            track,
            kind,
            top: y,
            height,
        });
        y += height + gap_after;
    };
    for i in (0..counts.video).rev() {
        push(Track::Video(i), LaneKind::Video, VIDEO_LANE_HEIGHT, 0.0);
        push(
            Track::Video(i),
            LaneKind::Audio,
            AUDIO_LANE_HEIGHT,
            TRACK_GAP,
        );
    }
    for i in 0..counts.audio {
        push(
            Track::Audio(i),
            LaneKind::Audio,
            AUDIO_LANE_HEIGHT,
            TRACK_GAP,
        );
    }
    lanes
}

pub fn content_height(lanes: &[Lane]) -> f32 {
    lanes.last().map_or(0.0, |l| l.bottom() + TRACK_GAP)
}

impl Lane {
    fn bottom(&self) -> f32 {
        self.top + self.height
    }

    /// Clip labels go on the topmost lane of each track.
    pub fn is_primary(&self) -> bool {
        self.track.is_video() == (self.kind == LaneKind::Video)
    }

    fn distance(&self, y: f32) -> f32 {
        if y < self.top {
            self.top - y
        } else if y > self.bottom() {
            y - self.bottom()
        } else {
            0.0
        }
    }
}

impl View {
    pub fn lane_rect(&self, lane: &Lane) -> Rect {
        Rect::from_x_y_ranges(
            self.rows.x_range(),
            self.content_top + lane.top..=self.content_top + lane.bottom(),
        )
    }

    pub fn lane_at(&self, pos: Pos2) -> Option<&Lane> {
        let y = pos.y - self.content_top;
        self.lanes.iter().find(|l| l.distance(y) == 0.0)
    }

    /// Track of the given kind closest to screen `y`, so a dragged clip stays among its kind.
    pub fn nearest_track(&self, y: f32, video: bool) -> Option<Track> {
        let y = y - self.content_top;
        self.lanes
            .iter()
            .filter(|l| l.track.is_video() == video)
            .min_by(|a, b| a.distance(y).total_cmp(&b.distance(y)))
            .map(|l| l.track)
    }

    /// Clips on `track` within view, in draw order (later on top).
    pub fn visible_clips<'p>(&self, project: &'p Project, track: Track) -> Vec<&'p Clip> {
        let (start, end) = (
            self.axis.time(self.rows.left()),
            self.axis.time(self.rows.right()),
        );
        let mut clips: Vec<_> = project
            .clips_on(track)
            .filter(|c| c.end() >= start && c.start <= end)
            .collect();
        clips.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.id.cmp(&b.id)));
        clips
    }
}

impl Ticks {
    pub fn for_zoom(pixels_per_second: f32) -> Self {
        let px = |step: f64| step * pixels_per_second as f64;
        let major = TICK_STEPS
            .into_iter()
            .find(|&s| px(s) >= MIN_LABEL_SPACING as f64)
            .unwrap_or(TICK_STEPS[TICK_STEPS.len() - 1]);
        let minor = TICK_STEPS
            .into_iter()
            .find(|&s| px(s) >= MIN_TICK_SPACING as f64 && divides(major, s))
            .unwrap_or(major);
        Self { major, minor }
    }

    /// Fractional digits that tell major ticks apart: none for whole seconds, then one per decade.
    pub fn label_decimals(self) -> usize {
        usize::from(self.major < 1.0) + usize::from(self.major < 0.1)
    }

    /// `(time, is_major)` for every tick within `start..=end`.
    pub fn within(self, start: f64, end: f64) -> impl Iterator<Item = (f64, bool)> {
        let per_major = (self.major / self.minor).round() as i64;
        let first = (start.max(0.0) / self.minor).floor() as i64;
        let last = (end / self.minor).ceil() as i64;
        (first..=last).map(move |i| (i as f64 * self.minor, i % per_major == 0))
    }
}

fn divides(major: f64, minor: f64) -> bool {
    let ratio = major / minor;
    (ratio - ratio.round()).abs() < 1e-6
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::project;

    fn counts_of<const N: usize>(tracks: [Track; N]) -> TrackCounts {
        TrackCounts::of(&project(tracks.map(|track| (track, 0.0, 1.0))).0)
    }

    #[test]
    fn axis_roundtrip() {
        let axis = TimeAxis {
            origin: 100.0,
            scroll: 10.0,
            pixels_per_second: 20.0,
        };
        assert_eq!(axis.x(10.0), 100.0);
        assert_eq!(axis.x(12.5), 150.0);
        assert_eq!(axis.time(150.0), 12.5);
        assert_eq!(axis.seconds(40.0), 2.0);
    }

    #[test]
    fn zoom_keeps_anchor_in_place() {
        let axis = TimeAxis {
            origin: 0.0,
            scroll: 5.0,
            pixels_per_second: 10.0,
        };
        let zoomed = axis.zoomed(200.0, 2.0);
        assert_eq!(zoomed.pixels_per_second, 20.0);
        assert!((zoomed.time(200.0) - axis.time(200.0)).abs() < 1e-9);
        let clamped = axis.zoomed(0.0, 1e6);
        assert_eq!(clamped.pixels_per_second, MAX_PIXELS_PER_SECOND);
        assert_eq!(clamped.scroll, 5.0);
    }

    #[test]
    fn track_counts_follow_the_project() {
        let empty = TrackCounts::of(&Project::default());
        assert_eq!(empty, TrackCounts { video: 1, audio: 1 });
        let used = counts_of([Track::Video(0), Track::Video(2)]);
        assert_eq!(used, TrackCounts { video: 3, audio: 1 });
        let audio = counts_of([Track::Audio(0), Track::Audio(1)]);
        assert_eq!(audio, TrackCounts { video: 1, audio: 2 });
    }

    #[test]
    fn lanes_go_from_highest_video_down_to_audio() {
        let lanes = layout_lanes(TrackCounts { video: 2, audio: 2 });
        let order: Vec<_> = lanes.iter().map(|l| (l.track, l.kind)).collect();
        assert_eq!(
            order,
            [
                (Track::Video(1), LaneKind::Video),
                (Track::Video(1), LaneKind::Audio),
                (Track::Video(0), LaneKind::Video),
                (Track::Video(0), LaneKind::Audio),
                (Track::Audio(0), LaneKind::Audio),
                (Track::Audio(1), LaneKind::Audio),
            ]
        );
        assert_eq!(lanes[0].top, 0.0);
        assert_eq!(lanes[1].top, VIDEO_LANE_HEIGHT);
        assert_eq!(
            lanes[2].top,
            VIDEO_LANE_HEIGHT + AUDIO_LANE_HEIGHT + TRACK_GAP
        );
        assert!(lanes[0].is_primary() && !lanes[1].is_primary() && lanes[4].is_primary());
    }

    fn view(counts: TrackCounts) -> View {
        View {
            axis: TimeAxis {
                origin: 0.0,
                scroll: 0.0,
                pixels_per_second: 1.0,
            },
            lanes: layout_lanes(counts),
            rows: Rect::from_min_max(Pos2::new(0.0, 100.0), Pos2::new(500.0, 300.0)),
            content_top: 50.0,
        }
    }

    #[test]
    fn lane_lookup_uses_scrolled_content() {
        let view = view(TrackCounts { video: 1, audio: 1 });
        assert_eq!(view.lane_at(Pos2::new(0.0, 49.0)), None);
        assert_eq!(
            view.lane_at(Pos2::new(0.0, 51.0)).map(|l| l.kind),
            Some(LaneKind::Video)
        );
        let linked = view.lane_at(Pos2::new(0.0, 50.0 + VIDEO_LANE_HEIGHT + 1.0));
        assert_eq!(
            linked.map(|l| (l.track, l.kind)),
            Some((Track::Video(0), LaneKind::Audio))
        );
        assert_eq!(view.lane_rect(&view.lanes[0]).top(), 50.0);
    }

    #[test]
    fn nearest_track_stays_within_kind() {
        let view = view(TrackCounts { video: 2, audio: 2 });
        let a2_y = 50.0 + view.lanes[5].top + 1.0;
        assert_eq!(view.nearest_track(a2_y, true), Some(Track::Video(0)));
        assert_eq!(view.nearest_track(a2_y, false), Some(Track::Audio(1)));
        assert_eq!(view.nearest_track(-1000.0, true), Some(Track::Video(1)));
        assert_eq!(view.nearest_track(-1000.0, false), Some(Track::Audio(0)));
    }

    #[test]
    fn visible_clips_are_culled_and_ordered() {
        let (mut project, [x, y, _, w, _]) = project([
            (Track::Video(0), 300.0, 100.0),
            (Track::Video(0), 250.0, 100.0),
            (Track::Video(0), 0.0, 100.0),
            (Track::Video(0), 250.0, 100.0),
            (Track::Video(0), 800.0, 100.0),
        ]);
        // Stored against id order, so only the explicit tie-break puts y before w.
        project.clips.reverse();
        let mut view = view(TrackCounts { video: 1, audio: 1 });
        view.axis.scroll = 200.0;
        let visible = view.visible_clips(&project, Track::Video(0));
        let ids: Vec<_> = visible.iter().map(|c| c.id).collect();
        assert_eq!(ids, [y, w, x]);
    }

    #[test]
    fn areas_split_widget() {
        let areas = Areas::new(Rect::from_min_max(pos2(10.0, 20.0), pos2(510.0, 220.0)));
        assert_eq!(
            areas.rows.min,
            pos2(10.0 + HEADER_WIDTH, 20.0 + RULER_HEIGHT)
        );
        assert_eq!(areas.ruler.x_range(), areas.rows.x_range());
        assert_eq!(areas.header.y_range(), areas.rows.y_range());
        assert_eq!(areas.all().max, pos2(510.0, 220.0));
    }

    #[test]
    fn tick_steps_follow_zoom() {
        assert_eq!(
            Ticks::for_zoom(100.0),
            Ticks {
                major: 1.0,
                minor: 0.1
            }
        );
        assert_eq!(
            Ticks::for_zoom(20.0),
            Ticks {
                major: 5.0,
                minor: 0.5
            }
        );
        assert_eq!(
            Ticks::for_zoom(0.5),
            Ticks {
                major: 300.0,
                minor: 30.0
            }
        );
        assert_eq!(
            Ticks::for_zoom(MAX_PIXELS_PER_SECOND),
            Ticks {
                major: 0.05,
                minor: 0.01
            }
        );
    }

    #[test]
    fn ticks_cover_range_with_majors_marked() {
        let ticks = Ticks {
            major: 10.0,
            minor: 5.0,
        };
        let all: Vec<_> = ticks.within(-3.0, 21.0).collect();
        assert_eq!(
            all,
            [
                (0.0, true),
                (5.0, false),
                (10.0, true),
                (15.0, false),
                (20.0, true),
                (25.0, false)
            ]
        );
    }

    #[test]
    fn labels_get_decimals_below_a_second() {
        let decimals = |step| {
            Ticks {
                major: step,
                minor: step,
            }
            .label_decimals()
        };
        assert_eq!([5.0, 1.0, 0.5, 0.1, 0.05].map(decimals), [0, 0, 1, 1, 2]);
    }
}
