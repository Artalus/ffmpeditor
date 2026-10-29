//! Turns a project into ffmpeg invocations: final export and still preview frames.

mod export;
mod graph;
mod preview;
mod process;
#[cfg(test)]
mod tests;

// Unused until the app shell is wired up.
#[allow(unused_imports)]
pub use export::{ExportJob, export_args};
#[allow(unused_imports)]
pub use preview::{Previewer, preview_args};

#[derive(Clone, Debug, PartialEq)]
pub enum ExportStatus {
    Running { progress: f32 },
    Finished,
    Failed(String),
    Cancelled,
}
