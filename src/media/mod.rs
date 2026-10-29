//! Source media inspection: probing, timeline thumbnails, waveforms.

use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::ffmpeg::{self, Ffmpeg};

mod cache;
#[cfg(test)]
mod test_media;
mod thumbnails;
mod waveform;

pub use cache::MediaCache;

/// Used when a stream reports no usable frame rate, so frame-step math never divides by zero.
const FALLBACK_FPS: f64 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo {
    pub duration: f64,
    pub video: Option<VideoInfo>,
    pub has_audio: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Absolute sample peaks in 0..=1, `peaks_per_second` buckets per second of source.
#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    pub peaks_per_second: f64,
    pub peaks: Vec<f32>,
}

pub fn probe(ffmpeg: &Ffmpeg, path: &Path) -> anyhow::Result<MediaInfo> {
    if !path.is_file() {
        bail!("file not found: {}", path.display());
    }
    let output = ffmpeg
        .ffprobe()
        .args(["-v", "error", "-print_format", "json"])
        .args(["-show_format", "-show_streams"])
        .arg(path)
        .output()
        .with_context(|| format!("running {}", ffmpeg.ffprobe.display()))?;
    if !output.status.success() {
        bail!(ffmpeg::failure(
            output.status,
            &String::from_utf8_lossy(&output.stderr)
        ));
    }
    parse_probe(&output.stdout).with_context(|| format!("probing {}", path.display()))
}

impl Waveform {
    /// Highest peak within source time `start..end`.
    pub fn peak(&self, start: f64, end: f64) -> f32 {
        let first = (start * self.peaks_per_second).floor().max(0.0) as usize;
        let last = (end * self.peaks_per_second).ceil().max(0.0) as usize;
        let last = last.min(self.peaks.len());
        self.peaks
            .get(first..last)
            .map_or(0.0, |peaks| peaks.iter().copied().fold(0.0, f32::max))
    }
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: ProbeFormat,
}

#[derive(Default, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    disposition: ProbeDisposition,
}

#[derive(Default, Deserialize)]
struct ProbeDisposition {
    #[serde(default)]
    attached_pic: u8,
}

fn parse_probe(json: &[u8]) -> anyhow::Result<MediaInfo> {
    let probe: Probe = serde_json::from_slice(json).context("parsing ffprobe output")?;
    let video = probe.streams.iter().find_map(video_info);
    let has_audio = probe.streams.iter().any(|s| s.is_type("audio"));
    if video.is_none() && !has_audio {
        bail!("no video or audio streams");
    }
    let duration = parse_seconds(&probe.format.duration)
        .or_else(|| {
            probe
                .streams
                .iter()
                .filter_map(|s| parse_seconds(&s.duration))
                .reduce(f64::max)
        })
        .context("unknown duration")?;
    Ok(MediaInfo {
        duration,
        video,
        has_audio,
    })
}

fn video_info(stream: &ProbeStream) -> Option<VideoInfo> {
    if !stream.is_type("video") || stream.disposition.attached_pic == 1 {
        return None;
    }
    let fps = parse_rate(&stream.avg_frame_rate)
        .or_else(|| parse_rate(&stream.r_frame_rate))
        .unwrap_or(FALLBACK_FPS);
    Some(VideoInfo {
        width: stream.width?,
        height: stream.height?,
        fps,
    })
}

fn parse_seconds(value: &Option<String>) -> Option<f64> {
    let seconds: f64 = value.as_deref()?.parse().ok()?;
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

/// "num/den" as reported by ffprobe; "0/0" means unknown.
fn parse_rate(value: &Option<String>) -> Option<f64> {
    let (num, den) = value.as_deref()?.split_once('/')?;
    let rate = num.parse::<f64>().ok()? / den.parse::<f64>().ok()?;
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

impl ProbeStream {
    fn is_type(&self, codec_type: &str) -> bool {
        self.codec_type.as_deref() == Some(codec_type)
    }
}

#[cfg(test)]
mod tests {
    use super::test_media::TestMedia;
    use super::*;
    use crate::test_support;

    #[test]
    fn probe_video_with_audio() {
        let json = r#"{
            "streams": [
                {"index": 0, "codec_type": "video", "width": 1920, "height": 1080,
                 "r_frame_rate": "60/1", "avg_frame_rate": "60/1", "duration": "12.000000",
                 "disposition": {"default": 1, "attached_pic": 0}},
                {"index": 1, "codec_type": "audio", "r_frame_rate": "0/0", "avg_frame_rate": "0/0",
                 "duration": "12.010000", "disposition": {"default": 1, "attached_pic": 0}}
            ],
            "format": {"filename": "a.mp4", "duration": "12.010000"}
        }"#;
        let info = parse_probe(json.as_bytes()).unwrap();
        assert_eq!(info.duration, 12.01);
        assert!(info.has_audio);
        let video = info.video.unwrap();
        assert_eq!((video.width, video.height, video.fps), (1920, 1080, 60.0));
    }

    #[test]
    fn probe_skips_cover_art() {
        let json = r#"{
            "streams": [
                {"codec_type": "audio", "duration": "180.5"},
                {"codec_type": "video", "width": 600, "height": 600, "r_frame_rate": "90000/1",
                 "avg_frame_rate": "0/0", "disposition": {"attached_pic": 1}},
                {"codec_type": "video", "width": 1280, "height": 720, "avg_frame_rate": "30/1"}
            ],
            "format": {"duration": "180.500000"}
        }"#;
        let video = parse_probe(json.as_bytes()).unwrap().video.unwrap();
        assert_eq!((video.width, video.height), (1280, 720));

        let json = r#"{
            "streams": [
                {"codec_type": "audio"},
                {"codec_type": "video", "width": 600, "height": 600, "disposition": {"attached_pic": 1}}
            ],
            "format": {"duration": "180.500000"}
        }"#;
        let info = parse_probe(json.as_bytes()).unwrap();
        assert_eq!(info.video, None);
        assert!(info.has_audio);
    }

    #[test]
    fn probe_frame_rates() {
        let json = r#"{
            "streams": [
                {"codec_type": "video", "width": 640, "height": 480,
                 "avg_frame_rate": "0/0", "r_frame_rate": "30000/1001"}
            ],
            "format": {"duration": "1.0"}
        }"#;
        let fps = parse_probe(json.as_bytes()).unwrap().video.unwrap().fps;
        assert!((fps - 29.97).abs() < 0.001, "{fps}");

        let json = r#"{
            "streams": [{"codec_type": "video", "width": 640, "height": 480, "avg_frame_rate": "0/0"}],
            "format": {"duration": "1.0"}
        }"#;
        let fps = parse_probe(json.as_bytes()).unwrap().video.unwrap().fps;
        assert_eq!(fps, FALLBACK_FPS);
    }

    #[test]
    fn probe_duration_falls_back_to_longest_stream() {
        let json = r#"{
            "streams": [
                {"codec_type": "video", "width": 640, "height": 480, "duration": "9.5"},
                {"codec_type": "audio", "duration": "10.25"},
                {"codec_type": "audio"}
            ],
            "format": {"filename": "a.mkv"}
        }"#;
        assert_eq!(parse_probe(json.as_bytes()).unwrap().duration, 10.25);
    }

    #[test]
    fn probe_rejects_unusable_files() {
        let subtitles =
            r#"{"streams": [{"codec_type": "subtitle"}], "format": {"duration": "3.0"}}"#;
        assert!(parse_probe(subtitles.as_bytes()).is_err());
        let no_duration = r#"{"streams": [{"codec_type": "audio"}], "format": {}}"#;
        assert!(parse_probe(no_duration.as_bytes()).is_err());
        assert!(parse_probe(b"not json").is_err());
    }

    #[test]
    fn probe_generated_media() {
        let media = TestMedia::new("probe");
        let ffmpeg = test_support::ffmpeg();

        let info = probe(&ffmpeg, &media.video_with_audio()).unwrap();
        assert!((info.duration - 3.0).abs() < 0.1, "{info:?}");
        assert!(info.has_audio);
        let video = VideoInfo {
            width: 320,
            height: 240,
            fps: 30.0,
        };
        assert_eq!(info.video, Some(video));

        let info = probe(&ffmpeg, &media.audio_only()).unwrap();
        assert!((info.duration - 2.0).abs() < 0.1, "{info:?}");
        assert_eq!((info.video, info.has_audio), (None, true));

        let info = probe(&ffmpeg, &media.video_only()).unwrap();
        assert!(info.video.is_some() && !info.has_audio, "{info:?}");

        let error = probe(&ffmpeg, &media.missing()).unwrap_err();
        assert!(error.to_string().contains("not found"), "{error}");
    }

    #[test]
    fn waveform_peak_covers_touched_buckets() {
        let waveform = Waveform {
            peaks_per_second: 10.0,
            peaks: vec![0.1, 0.5, 0.2, 0.9, 0.3],
        };
        assert_eq!(waveform.peak(0.0, 0.1), 0.1);
        assert_eq!(waveform.peak(0.0, 0.25), 0.5);
        assert_eq!(waveform.peak(0.21, 0.22), 0.2);
        assert_eq!(waveform.peak(0.15, 0.31), 0.9);
        assert_eq!(waveform.peak(-5.0, 100.0), 0.9);
        assert_eq!(waveform.peak(0.4, 1.0), 0.3);
    }

    #[test]
    fn waveform_peak_out_of_range_is_silent() {
        let waveform = Waveform {
            peaks_per_second: 10.0,
            peaks: vec![0.5; 5],
        };
        assert_eq!(waveform.peak(0.5, 1.0), 0.0);
        assert_eq!(waveform.peak(-1.0, -0.5), 0.0);
        assert_eq!(waveform.peak(0.3, 0.3), 0.0);
        assert_eq!(waveform.peak(0.3, 0.1), 0.0);
        assert_eq!(waveform.peak(f64::NAN, 0.3), 0.5);
        let empty = Waveform {
            peaks_per_second: 10.0,
            peaks: Vec::new(),
        };
        assert_eq!(empty.peak(0.0, 1.0), 0.0);
    }
}
