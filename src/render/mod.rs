//! Turns a project into ffmpeg invocations: final export and still preview frames.

mod export;
mod graph;
mod preview;
mod process;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

// Unused until the app shell is wired up.
#[allow(unused_imports)]
pub use export::{ExportJob, export_args};
#[allow(unused_imports)]
pub use preview::{Previewer, preview_args};

use crate::media::MediaInfo;
use crate::model::SourceId;

pub type MediaInfos = HashMap<SourceId, MediaInfo>;

#[derive(Clone, Debug, PartialEq)]
pub enum ExportStatus {
    Running { progress: f32 },
    Finished,
    Failed(String),
    Cancelled,
}
