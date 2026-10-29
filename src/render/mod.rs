//! Turns a project into ffmpeg invocations: final export and still preview frames.

mod export;
mod graph;
mod preview;
mod process;
#[cfg(test)]
mod tests;

pub use export::{ExportJob, export_args};
pub use preview::Previewer;

#[derive(Clone, Debug, PartialEq)]
pub enum ExportStatus {
    Running { progress: f32 },
    Finished,
    Failed(String),
    Cancelled,
}
