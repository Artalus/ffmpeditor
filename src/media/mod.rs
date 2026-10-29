//! Source media inspection: probing, timeline thumbnails, waveforms.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::ffmpeg::{self, Ffmpeg};
use crate::model::{SourceId, Track};

mod cache;
mod thumbnails;
mod waveform;

pub use cache::MediaCache;

pub type MediaInfos = HashMap<SourceId, MediaInfo>;

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

impl MediaInfo {
    /// Track a freshly imported source goes to: video to V1 (bringing its audio along), audio-only to A1.
    pub fn default_track(&self) -> anyhow::Result<Track> {
        if self.duration <= 0.0 {
            bail!("unknown duration");
        }
        match (&self.video, self.has_audio) {
            (Some(_), _) => Ok(Track::Video(0)),
            (None, true) => Ok(Track::Audio(0)),
            (None, false) => bail!("no video or audio stream"),
        }
    }
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
    use serde_json::{Value, json};

    use super::*;
    use crate::test_support::{self, fixtures, media_info};

    /// `format.duration` is optional, like in real ffprobe output.
    fn parse(streams: Value, duration: Option<&str>) -> anyhow::Result<MediaInfo> {
        let probe = json!({"streams": streams, "format": {"duration": duration}});
        parse_probe(probe.to_string().as_bytes())
    }

    #[test]
    fn probe_skips_cover_art() {
        let cover = json!({
            "codec_type": "video", "width": 600, "height": 600, "r_frame_rate": "90000/1",
            "avg_frame_rate": "0/0", "disposition": {"attached_pic": 1}
        });
        let streams = json!([
            {"codec_type": "audio", "duration": "180.5"},
            cover,
            {"codec_type": "video", "width": 1280, "height": 720, "avg_frame_rate": "30/1"}
        ]);
        let video = parse(streams, Some("180.5")).unwrap().video.unwrap();
        assert_eq!((video.width, video.height), (1280, 720));

        let info = parse(json!([{"codec_type": "audio"}, cover]), Some("180.5")).unwrap();
        assert_eq!(info.video, None);
        assert!(info.has_audio);
    }

    #[test]
    fn probe_frame_rates() {
        let fps = |avg: &str, r: Option<&str>| {
            let stream = json!({
                "codec_type": "video", "width": 640, "height": 480,
                "avg_frame_rate": avg, "r_frame_rate": r
            });
            parse(json!([stream]), Some("1.0"))
                .unwrap()
                .video
                .unwrap()
                .fps
        };
        let ntsc = fps("0/0", Some("30000/1001"));
        assert!((ntsc - 29.97).abs() < 0.001, "{ntsc}");
        assert_eq!(fps("0/0", None), FALLBACK_FPS);
    }

    #[test]
    fn probe_duration_falls_back_to_longest_stream() {
        let streams = json!([
            {"codec_type": "video", "width": 640, "height": 480, "duration": "9.5"},
            {"codec_type": "audio", "duration": "10.25"},
            {"codec_type": "audio"}
        ]);
        assert_eq!(parse(streams, None).unwrap().duration, 10.25);
    }

    #[test]
    fn probe_rejects_unusable_files() {
        assert!(parse(json!([{"codec_type": "subtitle"}]), Some("3.0")).is_err());
        assert!(parse(json!([{"codec_type": "audio"}]), None).is_err());
        assert!(parse_probe(b"not json").is_err());
    }

    #[test]
    fn probe_generated_media() {
        let fx = fixtures();
        let ffmpeg = test_support::ffmpeg();

        let info = probe(&ffmpeg, &fx.testsrc).unwrap();
        assert!((info.duration - 3.0).abs() < 0.1, "{info:?}");
        assert!(info.has_audio);
        let video = VideoInfo {
            width: 320,
            height: 240,
            fps: 30.0,
        };
        assert_eq!(info.video, Some(video));

        let info = probe(&ffmpeg, &fx.audio_only).unwrap();
        assert!((info.duration - 2.0).abs() < 0.1, "{info:?}");
        assert_eq!((info.video, info.has_audio), (None, true));

        let info = probe(&ffmpeg, &fx.video_only).unwrap();
        assert!(info.video.is_some() && !info.has_audio, "{info:?}");

        let error = probe(&ffmpeg, Path::new("missing.mp4")).unwrap_err();
        assert!(error.to_string().contains("not found"), "{error}");
    }

    #[test]
    fn imported_media_goes_to_first_track_of_its_kind() {
        let track = |video, audio| media_info(video, audio).default_track().unwrap();
        assert_eq!(track(true, true), Track::Video(0));
        assert_eq!(track(true, false), Track::Video(0));
        assert_eq!(track(false, true), Track::Audio(0));
    }

    #[test]
    fn unusable_media_has_no_track() {
        assert!(media_info(false, false).default_track().is_err());
        let mut info = media_info(true, true);
        info.duration = 0.0;
        assert!(info.default_track().is_err());
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
