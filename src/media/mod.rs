//! Source media inspection: probing, timeline thumbnails, waveforms.

use std::collections::HashMap;
use std::path::Path;

use crate::ffmpeg::Ffmpeg;
use crate::model::SourceId;

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

/// Owns everything derived from source files; extraction runs on background threads.
pub struct MediaCache {}

pub fn probe(ffmpeg: &Ffmpeg, path: &Path) -> anyhow::Result<MediaInfo> {
    todo!()
}

impl Waveform {
    /// Highest peak within source time `start..end`.
    pub fn peak(&self, start: f64, end: f64) -> f32 {
        todo!()
    }
}

impl MediaCache {
    pub fn new(ctx: egui::Context, ffmpeg: Ffmpeg) -> Self {
        todo!()
    }

    pub fn set_ffmpeg(&mut self, ffmpeg: Ffmpeg) {
        todo!()
    }

    /// Probes synchronously, then starts thumbnail and waveform extraction in the background.
    pub fn add(&mut self, id: SourceId, path: &Path) -> anyhow::Result<MediaInfo> {
        todo!()
    }

    pub fn clear(&mut self) {
        todo!()
    }

    /// Call once per frame: collects finished background work and uploads thumbnails as textures.
    pub fn poll(&mut self) {
        todo!()
    }

    pub fn info(&self, id: SourceId) -> Option<&MediaInfo> {
        todo!()
    }

    pub fn infos(&self) -> &HashMap<SourceId, MediaInfo> {
        todo!()
    }

    /// Why probing or extraction failed, e.g. missing file.
    pub fn error(&self, id: SourceId) -> Option<&str> {
        todo!()
    }

    /// Nearest extracted thumbnail to `source_time`; None while still extracting.
    pub fn thumbnail(&self, id: SourceId, source_time: f64) -> Option<&egui::TextureHandle> {
        todo!()
    }

    pub fn waveform(&self, id: SourceId) -> Option<&Waveform> {
        todo!()
    }
}
