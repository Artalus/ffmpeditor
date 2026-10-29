mod edit;
mod layout;
mod paint;

use egui::{CursorIcon, PointerButton, Pos2, Response, Sense, Ui, Vec2};

use self::edit::{
    ClipDrag, SNAP_DISTANCE, Snapping, Zone, hit_clip, opposite_edge, range_selection,
};
use self::layout::{Areas, TimeAxis, TrackCounts, View, content_height, layout_lanes};
use crate::media::MediaCache;
use crate::model::{ClipId, Project, Span};

pub struct TimelineState {
    pixels_per_second: f32,
    /// Timeline time at the left edge of the view, seconds.
    scroll: f64,
    pub playhead: f64,
    /// Export / preview range picked on the ruler.
    pub selection: Option<Span>,
    pub selected_clip: Option<ClipId>,
    /// Vertical scroll of the track rows, points.
    scroll_y: f32,
    drag: Option<Drag>,
}

pub struct TimelineResponse {
    /// A drag is in progress; the app holds off undo snapshots until it ends.
    pub interacting: bool,
    pub playhead_moved: bool,
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Clip {
        edit: ClipDrag,
        /// Rows are frozen while dragging, with a spare empty track of the clip's kind to
        /// drop onto; otherwise it would keep reappearing under the pointer and pull the
        /// clip ever higher.
        tracks: TrackCounts,
    },
    Range {
        anchor: f64,
    },
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
    let ruler = ui.interact(
        areas.ruler,
        ui.make_persistent_id("timeline_ruler"),
        Sense::click_and_drag(),
    );
    let rows = ui.interact(
        areas.rows,
        ui.make_persistent_id("timeline_rows"),
        Sense::click_and_drag(),
    );

    let lanes = layout_lanes(visible_tracks(state, project));
    navigate(ui, state, project, &areas, content_height(&lanes));
    let view = View {
        axis: axis(state, &areas),
        lanes,
        rows: areas.rows,
        content_top: areas.rows.top() - state.scroll_y,
    };
    edit_range(ui, &ruler, state, project, &view);
    edit_clips(ui, &rows, state, project, media, &view);
    if state.drag.is_some() && !ruler.dragged() && !rows.dragged() {
        state.drag = None;
        // Rows were frozen during the drag; drop the spare track right away.
        ui.ctx().request_repaint();
    }
    set_cursor(ui, &ruler, &rows, state, project, &view);

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
        interacting: state.drag.is_some(),
        playhead_moved: state.playhead != playhead_before,
    }
}

fn visible_tracks(state: &TimelineState, project: &Project) -> TrackCounts {
    match state.drag {
        Some(Drag::Clip { tracks, .. }) => tracks,
        _ => TrackCounts::of(project),
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
    state.scroll = state.scroll.max(0.0);
    // Not while dragging: trimming the last clip would yank the view.
    if state.drag.is_none() {
        // Stop once the content end (or the playhead) reaches mid-view.
        let half_view = axis(state, areas).seconds(areas.rows.width()) / 2.0;
        let end = project.duration().max(state.playhead);
        state.scroll = state.scroll.min((end - half_view).max(0.0));
    }
}

/// Click sets the playhead and clears the range; drag selects a range.
/// Near a range edge, click keeps the range and drag moves that edge.
fn edit_range(
    ui: &Ui,
    response: &Response,
    state: &mut TimelineState,
    project: &Project,
    view: &View,
) {
    let axis = &view.axis;
    let selection = state.selection;
    let grabbed = |x| selection.and_then(|s| opposite_edge(axis, s, x));
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.playhead = axis.time(pos.x).max(0.0);
        if grabbed(pos.x).is_none() {
            state.selection = None;
        }
    }
    if response.drag_started_by(PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        state.drag = Some(Drag::Range {
            anchor: grabbed(origin.x).unwrap_or_else(|| axis.time(origin.x)),
        });
    }
    if response.dragged()
        && let Some(Drag::Range { anchor }) = state.drag
        && let Some(pos) = response.interact_pointer_pos()
    {
        let distance = axis.seconds(SNAP_DISTANCE);
        let span = range_selection(project, anchor, axis.time(pos.x), distance);
        state.selection = (!span.is_empty()).then_some(span);
        state.playhead = span.start;
    }
}

/// Click selects a clip (or deselects on empty space) and sets the playhead;
/// drag moves a clip or trims it by an edge.
fn edit_clips(
    ui: &Ui,
    response: &Response,
    state: &mut TimelineState,
    project: &mut Project,
    media: &MediaCache,
    view: &View,
) {
    let axis = &view.axis;
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.selected_clip = hit(view, project, pos).map(|(clip, _)| clip);
        state.playhead = axis.time(pos.x).max(0.0);
    }
    if response.drag_started_by(PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Some((clip, zone)) = hit(view, project, origin)
        && let Some(edit) = ClipDrag::begin(project, clip, zone, axis.time(origin.x))
        && let Some(video) = project.clip(clip).map(|c| c.track.is_video())
    {
        state.selected_clip = Some(clip);
        state.drag = Some(Drag::Clip {
            edit,
            tracks: TrackCounts::of(project).with_spare(video),
        });
    }
    if response.dragged()
        && let Some(Drag::Clip { edit, .. }) = state.drag
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(clip) = project.clip(edit.clip)
    {
        let track = view.nearest_track(pos.y, clip.track.is_video());
        let source_duration = media
            .info(clip.source)
            .map_or(f64::INFINITY, |i| i.duration);
        let snapping = Snapping {
            playhead: state.playhead,
            distance: axis.seconds(SNAP_DISTANCE),
        };
        edit.apply(project, axis.time(pos.x), track, source_duration, snapping);
    }
}

fn hit(view: &View, project: &Project, pos: Pos2) -> Option<(ClipId, Zone)> {
    let lane = view.lane_at(pos)?;
    let clips: Vec<_> = view
        .visible_clips(project, lane.track)
        .into_iter()
        .map(|c| (c.id, view.axis.x(c.start), view.axis.x(c.end())))
        .collect();
    hit_clip(&clips, pos.x)
}

fn set_cursor(
    ui: &Ui,
    ruler: &Response,
    rows: &Response,
    state: &TimelineState,
    project: &Project,
    view: &View,
) {
    let icon = match state.drag {
        Some(Drag::Clip { edit, .. }) => Some(zone_cursor(edit.zone)),
        Some(Drag::Range { .. }) => Some(CursorIcon::ResizeHorizontal),
        None => hover_cursor(ruler, rows, state, project, view),
    };
    if let Some(icon) = icon {
        ui.ctx().set_cursor_icon(icon);
    }
}

/// Text cursor on the ruler and empty lane space;
/// resize near range and clip edges, default over clip bodies.
fn hover_cursor(
    ruler: &Response,
    rows: &Response,
    state: &TimelineState,
    project: &Project,
    view: &View,
) -> Option<CursorIcon> {
    if let Some(pos) = ruler.hover_pos() {
        let on_edge = state
            .selection
            .is_some_and(|s| opposite_edge(&view.axis, s, pos.x).is_some());
        return Some(if on_edge {
            CursorIcon::ResizeHorizontal
        } else {
            CursorIcon::Text
        });
    }
    match hit(view, project, rows.hover_pos()?) {
        None => Some(CursorIcon::Text),
        Some((_, Zone::Body)) => None,
        Some((_, zone)) => Some(zone_cursor(zone)),
    }
}

fn zone_cursor(zone: Zone) -> CursorIcon {
    match zone {
        Zone::Start | Zone::End => CursorIcon::ResizeHorizontal,
        Zone::Body => CursorIcon::Grabbing,
    }
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
            drag: None,
        }
    }
}
