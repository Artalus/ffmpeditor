//! Pure clip editing logic behind mouse gestures: hit-testing, snapping, drag application.

use super::layout::TimeAxis;
use crate::model::{ClipId, Project, Span, Track};

const EDGE_GRAB: f32 = 6.0;
pub const SNAP_DISTANCE: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Body,
    Start,
    End,
    /// An edge of mute region `index` on an audio lane.
    Region {
        index: usize,
        start: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipDrag {
    pub clip: ClipId,
    pub zone: Zone,
    /// Pointer time minus the grabbed edge at press, so the clip doesn't jump to the pointer.
    grab_offset: f64,
    /// The grabbed mute region before the drag, so an empty result can be undone.
    pub original: Option<Span>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapping {
    pub playhead: f64,
    /// Seconds.
    pub distance: f64,
}

/// Edges win over bodies so overlapped (crossfaded) clips stay trimmable;
/// otherwise the last clip in `clips` (drawn on top) wins.
/// `clips` are `(id, x0, x1)`: horizontal screen extents within one lane.
pub fn hit_clip(clips: &[(ClipId, f32, f32)], x: f32) -> Option<(ClipId, Zone)> {
    let mut edge: Option<(f32, ClipId, Zone)> = None;
    let mut body = None;
    for &(id, x0, x1) in clips {
        let Some((zone, distance)) = zone_at(x0, x1, x) else {
            continue;
        };
        if zone == Zone::Body {
            body = Some(id);
        } else if edge.is_none_or(|(best, ..)| distance < best) {
            edge = Some((distance, id, zone));
        }
    }
    edge.map(|(_, id, zone)| (id, zone))
        .or(body.map(|id| (id, Zone::Body)))
}

/// Zone under `x` with its distance to that edge; edge zones are shrunk on narrow clips
/// so the body stays grabbable.
fn zone_at(x0: f32, x1: f32, x: f32) -> Option<(Zone, f32)> {
    if x < x0 || x > x1 {
        return None;
    }
    let grab = EDGE_GRAB.min((x1 - x0) / 3.0);
    let (from_start, from_end) = (x - x0, x1 - x);
    Some(if from_start <= grab && from_start <= from_end {
        (Zone::Start, from_start)
    } else if from_end <= grab {
        (Zone::End, from_end)
    } else {
        (Zone::Body, 0.0)
    })
}

impl ClipDrag {
    pub fn begin(project: &Project, clip: ClipId, zone: Zone, pointer: f64) -> Option<Self> {
        let clip = project.clip(clip)?;
        let region = match zone {
            Zone::Region { index, .. } => Some(*clip.audio.mute_regions.get(index)?),
            _ => None,
        };
        let edge = match (zone, region) {
            (Zone::Region { start, .. }, Some(r)) => {
                clip.source_to_timeline(if start { r.start } else { r.end })
            }
            (Zone::End, _) => clip.end(),
            _ => clip.start,
        };
        Some(Self {
            clip: clip.id,
            zone,
            grab_offset: pointer - edge,
            original: region,
        })
    }

    /// `track` only applies to moves; trims keep the clip where it is.
    pub fn apply(
        &self,
        project: &mut Project,
        pointer: f64,
        track: Option<Track>,
        source_duration: f64,
        snapping: Snapping,
    ) {
        let Some(clip) = project.clip(self.clip) else {
            return;
        };
        let targets = snap_targets(project, Some(self.clip), Some(snapping.playhead));
        let edge = pointer - self.grab_offset;
        match self.zone {
            Zone::Body => {
                let start = snap_span(edge, clip.duration, &targets, snapping.distance);
                let track = track.unwrap_or(clip.track);
                project.move_clip(self.clip, start, track);
            }
            Zone::Start => {
                let start = snap_time(edge, &targets, snapping.distance);
                project.trim_clip_start(self.clip, start);
            }
            Zone::End => {
                let end = snap_time(edge, &targets, snapping.distance);
                project.trim_clip_end(self.clip, end, source_duration);
            }
            Zone::Region { index, start } => {
                project.set_mute_edge(self.clip, index, start, edge);
            }
        }
    }
}

/// Nearest region edge within `EDGE_GRAB` of `x`; `regions` are the `(start, end)` screen xs of each.
pub fn hit_region_edge(regions: &[(f32, f32)], x: f32) -> Option<(usize, bool)> {
    regions
        .iter()
        .enumerate()
        .flat_map(|(i, &(x0, x1))| [((x0 - x).abs(), i, true), ((x1 - x).abs(), i, false)])
        .filter(|&(d, ..)| d <= EDGE_GRAB)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, i, start)| (i, start))
}

pub fn range_selection(project: &Project, anchor: f64, pointer: f64, distance: f64) -> Span {
    let targets = snap_targets(project, None, None);
    Span::new(
        snap_time(anchor.max(0.0), &targets, distance),
        snap_time(pointer.max(0.0), &targets, distance),
    )
}

/// Time of the selection edge farther from `x`, if the nearer one is within `EDGE_GRAB` of it;
/// anchoring a range drag there makes the grabbed edge follow the pointer.
pub fn opposite_edge(axis: &TimeAxis, selection: Span, x: f32) -> Option<f64> {
    let from_start = (axis.x(selection.start) - x).abs();
    let from_end = (axis.x(selection.end) - x).abs();
    (from_start.min(from_end) <= EDGE_GRAB).then_some(if from_start <= from_end {
        selection.end
    } else {
        selection.start
    })
}

fn snap_targets(project: &Project, exclude: Option<ClipId>, playhead: Option<f64>) -> Vec<f64> {
    let edges = project
        .clips
        .iter()
        .filter(|c| Some(c.id) != exclude)
        .flat_map(|c| [c.start, c.end()]);
    [0.0].into_iter().chain(playhead).chain(edges).collect()
}

fn snap_time(t: f64, targets: &[f64], distance: f64) -> f64 {
    nearest(t, targets, distance).unwrap_or(t)
}

/// Snaps whichever edge of `start..start + duration` is closer to a target; returns the new start.
fn snap_span(start: f64, duration: f64, targets: &[f64], distance: f64) -> f64 {
    let by_start = nearest(start, targets, distance);
    let by_end = nearest(start + duration, targets, distance).map(|t| t - duration);
    [by_start, by_end]
        .into_iter()
        .flatten()
        .min_by(|a, b| (a - start).abs().total_cmp(&(b - start).abs()))
        .unwrap_or(start)
}

fn nearest(t: f64, targets: &[f64], distance: f64) -> Option<f64> {
    targets
        .iter()
        .copied()
        .filter(|target| (target - t).abs() <= distance)
        .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::project;

    /// V1: [0, 10) and [20, 30); A1: [5, 8).
    fn sample() -> (Project, [ClipId; 3]) {
        project([
            (Track::Video(0), 0.0, 10.0),
            (Track::Video(0), 20.0, 10.0),
            (Track::Audio(0), 5.0, 3.0),
        ])
    }

    const SNAP: Snapping = Snapping {
        playhead: 15.0,
        distance: 0.5,
    };

    #[test]
    fn zones_by_edge_distance() {
        assert_eq!(zone_at(100.0, 200.0, 99.0), None);
        assert_eq!(zone_at(100.0, 200.0, 103.0), Some((Zone::Start, 3.0)));
        assert_eq!(zone_at(100.0, 200.0, 150.0), Some((Zone::Body, 0.0)));
        assert_eq!(zone_at(100.0, 200.0, 196.0), Some((Zone::End, 4.0)));
        // Narrow clip: edges shrink to a third each.
        assert_eq!(zone_at(0.0, 9.0, 4.5).map(|z| z.0), Some(Zone::Body));
        assert_eq!(zone_at(0.0, 9.0, 2.0).map(|z| z.0), Some(Zone::Start));
    }

    #[test]
    fn edges_beat_overlapping_bodies() {
        let clips = [(ClipId(1), 0.0, 100.0), (ClipId(2), 80.0, 200.0)];
        assert_eq!(hit_clip(&clips, 50.0), Some((ClipId(1), Zone::Body)));
        assert_eq!(hit_clip(&clips, 90.0), Some((ClipId(2), Zone::Body)));
        assert_eq!(hit_clip(&clips, 97.0), Some((ClipId(1), Zone::End)));
        assert_eq!(hit_clip(&clips, 83.0), Some((ClipId(2), Zone::Start)));
        assert_eq!(hit_clip(&clips, 300.0), None);
    }

    #[test]
    fn region_edges_by_distance() {
        let regions = [(100.0, 150.0), (152.0, 200.0)];
        assert_eq!(hit_region_edge(&regions, 96.0), Some((0, true)));
        assert_eq!(hit_region_edge(&regions, 125.0), None);
        assert_eq!(hit_region_edge(&regions, 151.5), Some((1, true)));
        assert_eq!(hit_region_edge(&regions, 149.0), Some((0, false)));
        assert_eq!(hit_region_edge(&[], 1.0), None);
    }

    #[test]
    fn span_snaps_nearer_edge() {
        let targets = [0.0, 10.0, 20.0];
        assert_eq!(snap_span(10.3, 5.0, &targets, 0.5), 10.0);
        assert_eq!(snap_span(4.8, 5.0, &targets, 0.5), 5.0);
        assert_eq!(snap_span(12.0, 5.0, &targets, 0.5), 12.0);
        assert_eq!(snap_span(9.6, 10.2, &targets, 0.5), 9.8);
    }

    #[test]
    fn targets_skip_dragged_clip() {
        let (project, [a, ..]) = sample();
        let mut targets = snap_targets(&project, Some(a), Some(15.0));
        targets.sort_by(f64::total_cmp);
        assert_eq!(targets, [0.0, 5.0, 8.0, 15.0, 20.0, 30.0]);
    }

    #[test]
    fn move_keeps_grab_offset_and_snaps() {
        let (mut project, [a, ..]) = sample();
        let drag = ClipDrag::begin(&project, a, Zone::Body, 4.0).unwrap();
        drag.apply(&mut project, 6.0, None, 10.0, SNAP);
        assert_eq!(project.clip(a).unwrap().start, 2.0);
        let snapping = Snapping {
            playhead: 16.0,
            ..SNAP
        };
        // End lands 0.2 before the playhead.
        drag.apply(&mut project, 9.8, Some(Track::Video(1)), 10.0, snapping);
        let clip = project.clip(a).unwrap();
        assert_eq!((clip.start, clip.track), (6.0, Track::Video(1)));
    }

    #[test]
    fn move_is_clamped_at_zero() {
        let (mut project, [_, b, _]) = sample();
        let drag = ClipDrag::begin(&project, b, Zone::Body, 25.0).unwrap();
        drag.apply(&mut project, 2.0, None, 10.0, SNAP);
        assert_eq!(project.clip(b).unwrap().start, 0.0);
    }

    #[test]
    fn trims_snap_and_respect_source() {
        let (mut project, [_, b, _]) = sample();
        project.clip_mut(b).unwrap().source_in = 10.0;
        let start = ClipDrag::begin(&project, b, Zone::Start, 20.5).unwrap();
        start.apply(&mut project, 15.8, Some(Track::Video(3)), 20.0, SNAP);
        let clip = project.clip(b).unwrap();
        assert_eq!(
            (clip.start, clip.duration, clip.track),
            (15.0, 15.0, Track::Video(0))
        );

        let end = ClipDrag::begin(&project, b, Zone::End, 29.5).unwrap();
        end.apply(&mut project, 100.0, None, 18.0, SNAP);
        assert_eq!(project.clip(b).unwrap().end(), 28.0);
    }

    #[test]
    fn range_snaps_to_clip_edges() {
        let (project, _) = sample();
        assert_eq!(
            range_selection(&project, 10.2, 3.0, 0.5),
            Span::new(3.0, 10.0)
        );
        assert_eq!(
            range_selection(&project, -4.0, 0.0, 0.5),
            Span::new(0.0, 0.0)
        );
    }

    #[test]
    fn selection_edge_grab() {
        let axis = TimeAxis {
            origin: 100.0,
            scroll: 0.0,
            pixels_per_second: 10.0,
        };
        // Edges at x = 120 and 150.
        let selection = Span::new(2.0, 5.0);
        assert_eq!(opposite_edge(&axis, selection, 124.0), Some(5.0));
        assert_eq!(opposite_edge(&axis, selection, 144.0), Some(2.0));
        assert_eq!(opposite_edge(&axis, selection, 135.0), None);
        assert_eq!(opposite_edge(&axis, selection, 100.0), None);
        // Overlapping grab zones: the nearer edge wins.
        let narrow = Span::new(2.0, 2.4);
        assert_eq!(opposite_edge(&axis, narrow, 121.0), Some(2.4));
        assert_eq!(opposite_edge(&axis, narrow, 123.0), Some(2.0));
    }
}
