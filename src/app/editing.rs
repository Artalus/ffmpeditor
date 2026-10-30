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
