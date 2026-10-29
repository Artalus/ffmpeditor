mod edit;
mod layout;

use crate::media::MediaCache;
use crate::model::{ClipId, Project, Span};

pub struct TimelineState {
    pub pixels_per_second: f32,
    /// Timeline time at the left edge of the view, seconds.
    pub scroll: f64,
    pub playhead: f64,
    /// Export / preview range picked on the ruler.
    pub selection: Option<Span>,
    pub selected_clip: Option<ClipId>,
}

pub struct TimelineResponse {
    /// A drag is in progress; the app holds off undo snapshots until it ends.
    pub interacting: bool,
    pub playhead_moved: bool,
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut TimelineState,
    project: &mut Project,
    media: &MediaCache,
) -> TimelineResponse {
    todo!()
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            pixels_per_second: 20.0,
            scroll: 0.0,
            playhead: 0.0,
            selection: None,
            selected_clip: None,
        }
    }
}
