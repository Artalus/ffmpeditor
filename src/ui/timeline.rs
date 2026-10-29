mod edit;
mod layout;
mod paint;

use egui::{Sense, Ui, Vec2};

use self::layout::{Areas, TimeAxis, TrackCounts, View, content_height, layout_lanes};
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
    /// Vertical scroll of the track rows, points.
    scroll_y: f32,
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
    let playhead_before = state.playhead;
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let areas = Areas::new(rect);

    let lanes = layout_lanes(TrackCounts::of(project));
    navigate(ui, state, project, &areas, content_height(&lanes));
    let view = View {
        axis: axis(state, &areas),
        lanes,
        rows: areas.rows,
        content_top: areas.rows.top() - state.scroll_y,
    };

    let scene = paint::Scene {
        areas: &areas,
        view: &view,
        state,
        project,
        media,
        visuals: ui.visuals(),
    };
    paint::timeline(ui.painter(), &scene);
    TimelineResponse {
        interacting: false,
        playhead_moved: state.playhead != playhead_before,
    }
}

fn axis(state: &TimelineState, areas: &Areas) -> TimeAxis {
    TimeAxis {
        origin: areas.rows.left(),
        scroll: state.scroll,
        pixels_per_second: state.pixels_per_second,
    }
}

/// Wheel scrolls time, Ctrl+wheel zooms around the pointer;
/// Alt+wheel or wheel over the track names scrolls the rows vertically.
fn navigate(
    ui: &Ui,
    state: &mut TimelineState,
    project: &Project,
    areas: &Areas,
    content_height: f32,
) {
    if ui.rect_contains_pointer(areas.all())
        && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
    {
        let (zoom, delta, alt) =
            ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.alt));
        if zoom != 1.0 {
            let zoomed = axis(state, areas).zoomed(pos.x.max(areas.rows.left()), zoom);
            state.pixels_per_second = zoomed.pixels_per_second;
            state.scroll = zoomed.scroll;
        }
        if delta != Vec2::ZERO {
            let axis = axis(state, areas);
            if alt || areas.header.contains(pos) {
                state.scroll_y -= delta.y;
                state.scroll -= axis.seconds(delta.x);
            } else {
                state.scroll -= axis.seconds(delta.x + delta.y);
            }
            ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
        }
    }
    let max_scroll_y = (content_height - areas.rows.height()).max(0.0);
    state.scroll_y = state.scroll_y.clamp(0.0, max_scroll_y);
    // Keep at least half a view of content (or the playhead) reachable, but no further.
    let half_view = axis(state, areas).seconds(areas.rows.width()) / 2.0;
    let end = project.duration().max(state.playhead);
    state.scroll = state.scroll.clamp(0.0, (end - half_view).max(0.0));
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            pixels_per_second: 20.0,
            scroll: 0.0,
            playhead: 0.0,
            selection: None,
            selected_clip: None,
            scroll_y: 0.0,
        }
    }
}
