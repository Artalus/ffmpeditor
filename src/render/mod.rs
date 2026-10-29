//! Turns a project into ffmpeg invocations: final export and still preview frames.

mod export;
mod graph;
mod process;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

// Unused until the app shell is wired up.
#[allow(unused_imports)]
pub use export::{ExportJob, export_args};

use crate::ffmpeg::Ffmpeg;
use crate::media::MediaInfo;
use crate::model::{Project, SourceId};

pub type MediaInfos = HashMap<SourceId, MediaInfo>;

#[derive(Clone, Debug, PartialEq)]
pub enum ExportStatus {
    Running { progress: f32 },
    Finished,
    Failed(String),
    Cancelled,
}

/// Renders the composited frame at the playhead in the background; newest request wins.
pub struct Previewer {}

/// ffmpeg arguments writing the single frame at timeline `time` as PNG to stdout.
pub fn preview_args(project: &Project, infos: &MediaInfos, time: f64) -> Vec<String> {
    todo!()
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
