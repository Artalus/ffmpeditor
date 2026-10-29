//! Turns a project into ffmpeg invocations: final export and still preview frames.

mod graph;
mod process;

use std::collections::HashMap;
use std::path::Path;

use crate::ffmpeg::Ffmpeg;
use crate::media::MediaInfo;
use crate::model::{Project, SourceId, Span};

pub type MediaInfos = HashMap<SourceId, MediaInfo>;

#[derive(Clone, Debug, PartialEq)]
pub enum ExportStatus {
    Running { progress: f32 },
    Finished,
    Failed(String),
    Cancelled,
}

pub struct ExportJob {}

/// Renders the composited frame at the playhead in the background; newest request wins.
pub struct Previewer {}

/// ffmpeg arguments (without the program) rendering timeline `range` of `project` into `output`.
pub fn export_args(
    project: &Project,
    infos: &MediaInfos,
    range: Span,
    output: &Path,
) -> Vec<String> {
    todo!()
}

/// ffmpeg arguments writing the single frame at timeline `time` as PNG to stdout.
pub fn preview_args(project: &Project, infos: &MediaInfos, time: f64) -> Vec<String> {
    todo!()
}

impl ExportJob {
    /// `duration` is the length of the rendered range, used to compute progress.
    pub fn start(ffmpeg: &Ffmpeg, args: Vec<String>, duration: f64) -> anyhow::Result<Self> {
        todo!()
    }

    pub fn status(&self) -> ExportStatus {
        todo!()
    }

    pub fn cancel(&mut self) {
        todo!()
    }
}

impl Previewer {
    pub fn new(ctx: egui::Context) -> Self {
        todo!()
    }

    pub fn request(&mut self, ffmpeg: &Ffmpeg, project: &Project, infos: &MediaInfos, time: f64) {
        todo!()
    }

    /// Call once per frame: picks up a finished frame and uploads it as a texture.
    pub fn poll(&mut self) {
        todo!()
    }

    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        todo!()
    }

    pub fn error(&self) -> Option<&str> {
        todo!()
    }

    pub fn is_busy(&self) -> bool {
        todo!()
    }
}
