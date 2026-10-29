use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

use super::{THUMBNAIL_HEIGHT, VideoInfo, read_full};
use crate::ffmpeg::Ffmpeg;

/// Long recordings get sparser thumbnails instead of more of them.
const MAX_THUMBNAILS: f64 = 300.0;
const MIN_INTERVAL_MS: u64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Whole milliseconds, so ffmpeg's `fps` filter and `index` agree exactly.
    interval_ms: u64,
    pub width: u32,
    pub height: u32,
}

impl Layout {
    pub fn new(duration: f64, video: &VideoInfo) -> Self {
        let interval_ms =
            ((duration / MAX_THUMBNAILS * 1000.0).round() as u64).max(MIN_INTERVAL_MS);
        let aspect = if video.width > 0 && video.height > 0 {
            f64::from(video.width) / f64::from(video.height)
        } else {
            16.0 / 9.0
        };
        let half_width = (f64::from(THUMBNAIL_HEIGHT) * aspect / 2.0).round() as u32;
        Self {
            interval_ms,
            width: half_width.max(1) * 2,
            height: THUMBNAIL_HEIGHT,
        }
    }

    pub fn interval(&self) -> f64 {
        self.interval_ms as f64 / 1000.0
    }

    pub fn index(&self, source_time: f64) -> usize {
        (source_time / self.interval()).round().max(0.0) as usize
    }
}

/// Raw RGBA frames, one per `layout.interval()` of source time, on stdout.
pub fn command(ffmpeg: &Ffmpeg, path: &Path, layout: &Layout) -> Command {
    // `eof_action=pass` keeps the last keyframe: with non-keyframes skipped its duration is
    // still one frame, so the default rounding drops it.
    let filter = format!(
        "fps=1000/{}:eof_action=pass,scale={}:{}",
        layout.interval_ms, layout.width, layout.height
    );
    let mut cmd = ffmpeg.ffmpeg();
    // Decoding keyframes only is what makes this fast; `fps` repeats the latest one per slot.
    cmd.args(["-v", "error", "-nostats", "-skip_frame", "nokey", "-i"])
        .arg(path)
        // `V` skips cover art.
        .args(["-map", "0:V:0", "-vf", &filter])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
    cmd
}

pub fn read_frames(
    mut rgba: impl Read,
    layout: &Layout,
    mut on_frame: impl FnMut(egui::ColorImage),
) -> io::Result<()> {
    let size = [layout.width as usize, layout.height as usize];
    let mut frame = vec![0; size[0] * size[1] * 4];
    while read_full(&mut rgba, &mut frame)? == frame.len() {
        on_frame(egui::ColorImage::from_rgba_unmultiplied(size, &frame));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::test_media::{self, TestMedia};

    fn video(width: u32, height: u32) -> VideoInfo {
        VideoInfo {
            width,
            height,
            fps: 60.0,
        }
    }

    #[test]
    fn interval_spreads_long_sources() {
        let interval = |duration| Layout::new(duration, &video(1920, 1080)).interval();
        assert_eq!(interval(0.0), 1.0);
        assert_eq!(interval(3.0), 1.0);
        assert_eq!(interval(300.0), 1.0);
        assert_eq!(interval(3000.0), 10.0);
        assert_eq!(interval(1000.1), 3.334);
    }

    #[test]
    fn size_keeps_aspect_with_even_width() {
        let size = |w, h| {
            let layout = Layout::new(10.0, &video(w, h));
            (layout.width, layout.height)
        };
        assert_eq!(size(1920, 1080), (128, THUMBNAIL_HEIGHT));
        assert_eq!(size(320, 240), (96, THUMBNAIL_HEIGHT));
        assert_eq!(size(2560, 1080), (170, THUMBNAIL_HEIGHT));
        assert_eq!(size(1, 1000), (2, THUMBNAIL_HEIGHT));
        assert_eq!(size(0, 0), (128, THUMBNAIL_HEIGHT));
    }

    #[test]
    fn index_rounds_to_nearest_thumbnail() {
        let layout = Layout::new(3000.0, &video(1920, 1080));
        assert_eq!(layout.index(0.0), 0);
        assert_eq!(layout.index(4.9), 0);
        assert_eq!(layout.index(5.1), 1);
        assert_eq!(layout.index(2999.0), 300);
        assert_eq!(layout.index(-3.0), 0);
        assert_eq!(layout.index(f64::NAN), 0);
    }

    #[test]
    fn partial_trailing_frame_is_dropped() {
        let layout = Layout::new(10.0, &video(16, 9));
        let frame_len = (layout.width * layout.height * 4) as usize;
        let rgba = vec![200; frame_len * 2 + frame_len / 2];
        let mut frames = Vec::new();
        read_frames(rgba.as_slice(), &layout, |frame| frames.push(frame)).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(
            frames[0].size,
            [layout.width as usize, layout.height as usize]
        );
    }

    #[test]
    fn one_thumbnail_per_second_from_keyframes() {
        let media = TestMedia::new("thumbnails");
        let layout = Layout::new(3.0, &video(320, 240));
        let output = command(&test_media::ffmpeg(), &media.video_with_audio(), &layout)
            .output()
            .unwrap();
        assert!(output.status.success());
        let mut frames = Vec::new();
        read_frames(output.stdout.as_slice(), &layout, |frame| {
            frames.push(frame)
        })
        .unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].size, [96, 72]);
        assert_ne!(frames[0], frames[2]);
    }
}
