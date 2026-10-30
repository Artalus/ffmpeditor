use crate::model::{Clip, ClipId, Project};

/// The selected clip if it spans `at`, otherwise every clip under `at`.
pub fn split_targets(project: &Project, selected: Option<ClipId>, at: f64) -> Vec<ClipId> {
    let spans = |clip: &Clip| clip.start < at && at < clip.end();
    if let Some(clip) = selected.and_then(|id| project.clip(id))
        && spans(clip)
    {
        return vec![clip.id];
    }
    project
        .clips
        .iter()
        .filter(|clip| spans(clip))
        .map(|clip| clip.id)
        .collect()
}

/// `time` moved by whole `frames`, snapped to the frame grid and kept within `0..=end`,
/// unless it already is past `end` (set by a ruler click).
pub fn step_frames(time: f64, fps: f64, frames: i32, end: f64) -> f64 {
    if fps <= 0.0 {
        return time;
    }
    let frame = (time * fps).round() + f64::from(frames);
    (frame / fps).clamp(0.0, end.max(time))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Track;

    /// V1: 0..10, 10..20; V2: 5..15
    fn project() -> (Project, [ClipId; 3]) {
        let mut project = Project::default();
        let source = project.add_source("a.mp4".into());
        let a = project.append_clip(source, Track::Video(0), 10.0);
        let b = project.append_clip(source, Track::Video(0), 10.0);
        let c = project.append_clip(source, Track::Video(1), 10.0);
        project.move_clip(c, 5.0, Track::Video(1));
        (project, [a, b, c])
    }

    #[test]
    fn selected_clip_under_playhead_is_split_alone() {
        let (project, [a, _, c]) = project();
        assert_eq!(split_targets(&project, Some(c), 7.0), vec![c]);
        assert_eq!(split_targets(&project, Some(a), 7.0), vec![a]);
    }

    #[test]
    fn without_usable_selection_all_clips_under_playhead_are_split() {
        let (project, [a, b, c]) = project();
        assert_eq!(split_targets(&project, None, 7.0), vec![a, c]);
        assert_eq!(split_targets(&project, Some(b), 7.0), vec![a, c]);
        assert_eq!(split_targets(&project, Some(ClipId(99)), 12.0), vec![b, c]);
    }

    #[test]
    fn clip_edges_are_not_split() {
        let (project, [_, _, c]) = project();
        assert_eq!(split_targets(&project, None, 10.0), vec![c]);
        assert_eq!(split_targets(&project, None, 0.0), vec![]);
        assert_eq!(split_targets(&project, None, 20.0), vec![]);
    }

    #[test]
    fn frame_steps_snap_to_grid() {
        assert_eq!(step_frames(0.0, 60.0, 1, 10.0), 1.0 / 60.0);
        assert_eq!(step_frames(1.0, 30.0, -1, 10.0), 29.0 / 30.0);
        assert_eq!(step_frames(0.51, 10.0, 1, 10.0), 0.6);
        assert_eq!(step_frames(0.0, 60.0, -1, 10.0), 0.0);
        assert_eq!(step_frames(10.0, 60.0, 1, 10.0), 10.0);
        assert_eq!(step_frames(3.0, 0.0, 1, 10.0), 3.0);
        assert_eq!(step_frames(12.0, 60.0, 1, 10.0), 12.0);
        assert_eq!(step_frames(12.0, 60.0, -1, 10.0), 719.0 / 60.0);
    }
}
