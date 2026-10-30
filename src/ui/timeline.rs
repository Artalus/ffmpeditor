mod edit;
mod layout;
mod paint;

use egui::{CursorIcon, PointerButton, Pos2, Rect, Response, Sense, Ui, UiBuilder, Vec2};

use self::edit::{
    ClipDrag, SNAP_DISTANCE, Snapping, Zone, hit_clip, hit_fade, hit_region_edge, opposite_edge,
    range_selection,
};
use self::layout::{Areas, LaneKind, TimeAxis, TrackCounts, View, content_height, layout_lanes};
use crate::media::MediaCache;
use crate::model::{ClipId, Project, Span, Track};

pub struct TimelineState {
    pixels_per_second: f32,
    /// Timeline time at the left edge of the view, seconds.
    scroll: f64,
    pub playhead: f64,
    /// Export / preview range picked on the ruler.
    pub selection: Option<Span>,
    pub selected_clip: Option<ClipId>,
    /// Clip under the last right-click; the context menu acts on it.
    menu_clip: Option<ClipId>,
    /// Lane kind of that right-click: fade reset targets the video or audio part.
    menu_video: bool,
    /// Vertical scroll of the track rows, points.
    scroll_y: f32,
    drag: Option<Drag>,
    /// Per track index: it held a clip at some point this session, so an empty one hints at deletion.
    used_video: Vec<bool>,
    used_audio: Vec<bool>,
}

pub struct TimelineResponse {
    /// A drag is in progress; the app holds off undo snapshots until it ends.
    pub interacting: bool,
    pub playhead_moved: bool,
    /// Non-empty track the user asked to delete; the app confirms first.
    pub delete_track: Option<Track>,
    /// Mute region the user dragged down to nothing; the app asks whether to drop it.
    pub empty_mute: Option<EmptyMute>,
}

#[derive(Clone, Copy, Debug)]
pub struct EmptyMute {
    pub clip: ClipId,
    pub index: usize,
    /// The region before the drag, in source time.
    pub original: Span,
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Clip(ClipDrag),
    Range { anchor: f64 },
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

    state.note_used_tracks(project);
    let lanes = layout_lanes(TrackCounts::of(project));
    navigate(ui, state, project, &areas, content_height(&lanes));
    let view = View {
        axis: axis(state, &areas),
        lanes,
        rows: areas.rows,
        content_top: areas.rows.top() - state.scroll_y,
    };
    edit_range(ui, &ruler, state, project, &view);
    let empty_mute = edit_clips(ui, &rows, state, project, media, &view);
    clip_menu(&rows, state, project);
    if state.drag.is_some() && !ruler.dragged() && !rows.dragged() {
        state.drag = None;
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
    let delete_track = track_controls(ui, state, project, &areas, &view);
    TimelineResponse {
        interacting: state.drag.is_some(),
        playhead_moved: state.playhead != playhead_before,
        delete_track,
        empty_mute,
    }
}

fn axis(state: &TimelineState, areas: &Areas) -> TimeAxis {
    TimeAxis {
        origin: areas.rows.left(),
        scroll: state.scroll,
        pixels_per_second: state.pixels_per_second,
    }
}

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

fn edit_clips(
    ui: &Ui,
    response: &Response,
    state: &mut TimelineState,
    project: &mut Project,
    media: &MediaCache,
    view: &View,
) -> Option<EmptyMute> {
    let axis = &view.axis;
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.selected_clip = hit(view, project, pos).map(|(clip, _)| clip);
        state.playhead = axis.time(pos.x).max(0.0);
    }
    if response.secondary_clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.menu_clip = hit(view, project, pos).map(|(clip, _)| clip);
        state.menu_video = view.lane_at(pos).is_some_and(|l| l.kind == LaneKind::Video);
        if state.menu_clip.is_some() {
            state.selected_clip = state.menu_clip;
        }
    }
    if response.drag_started_by(PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Some((clip, zone)) = hit(view, project, origin)
        && let Some(edit) = ClipDrag::begin(project, clip, zone, axis.time(origin.x))
    {
        state.selected_clip = Some(clip);
        state.drag = Some(Drag::Clip(edit));
    }
    if response.dragged()
        && let Some(Drag::Clip(edit)) = state.drag
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
    if response.drag_stopped()
        && let Some(Drag::Clip(edit)) = state.drag
    {
        return finish_region_drag(project, edit, axis.seconds(1.0));
    }
    None
}

/// Merges the dragged region into its neighbours, unless it shrank below `min_len`:
/// that one is left alone for the app to ask about.
fn finish_region_drag(project: &mut Project, edit: ClipDrag, min_len: f64) -> Option<EmptyMute> {
    let (Zone::Region { index, .. }, Some(original)) = (edit.zone, edit.original) else {
        return None;
    };
    let region = *project.clip(edit.clip)?.audio.mute_regions.get(index)?;
    if region.len() < min_len {
        return Some(EmptyMute {
            clip: edit.clip,
            index,
            original,
        });
    }
    project.merge_mute_regions(edit.clip);
    None
}

fn clip_menu(rows: &Response, state: &mut TimelineState, project: &mut Project) {
    let Some(id) = state.menu_clip else { return };
    rows.context_menu(|ui| {
        let Some(clip) = project.clip(id) else {
            return;
        };
        let muted = clip.audio.muted;
        let video = state.menu_video;
        let (fade_in, fade_out) = clip.fades(video);
        let selection = state
            .selection
            .filter(|s| s.intersect(clip.span()).is_some());
        let label = if muted { "Unmute clip" } else { "Mute clip" };
        if ui.button(label).clicked() {
            project.set_muted(id, !muted);
            ui.close();
        }
        let enabled = selection.is_some();
        let range = selection.unwrap_or(Span::new(0.0, 0.0));
        if ui
            .add_enabled(enabled, egui::Button::new("Mute selection"))
            .clicked()
        {
            project.mute_range(id, range);
            ui.close();
        }
        if ui
            .add_enabled(enabled, egui::Button::new("Unmute selection"))
            .clicked()
        {
            project.unmute_range(id, range);
            ui.close();
        }
        ui.menu_button("Reset fade", |ui| {
            for (label, reset_in, reset_out) in [
                ("Fade-in", true, false),
                ("Fade-out", false, true),
                ("Both", true, true),
            ] {
                let active = (reset_in && fade_in > 0.0) || (reset_out && fade_out > 0.0);
                if ui.add_enabled(active, egui::Button::new(label)).clicked() {
                    if reset_in {
                        project.set_fade(id, video, true, 0.0);
                    }
                    if reset_out {
                        project.set_fade(id, video, false, 0.0);
                    }
                    ui.close();
                }
            }
        });
    });
}

fn hit(view: &View, project: &Project, pos: Pos2) -> Option<(ClipId, Zone)> {
    let lane = view.lane_at(pos)?;
    let clips: Vec<_> = view
        .visible_clips(project, lane.track)
        .into_iter()
        .map(|c| (c.id, view.axis.x(c.start), view.axis.x(c.end())))
        .collect();
    let video = lane.kind == LaneKind::Video;
    let handles: Vec<_> = view
        .visible_clips(project, lane.track)
        .into_iter()
        .map(|c| {
            let (fade_in, fade_out) = c.fades(video);
            let x = |t| view.axis.x(t);
            (c.id, x(c.start + fade_in), x(c.end() - fade_out))
        })
        .collect();
    let top = pos.y - view.lane_rect(lane).top();
    if let Some((id, fade_in)) = hit_fade(&handles, pos.x, top) {
        return Some((id, Zone::Fade { video, fade_in }));
    }
    let (id, zone) = hit_clip(&clips, pos.x)?;
    // Fade handles win over trim zones, which win over region edges, which win over the clip body.
    if zone == Zone::Body
        && lane.kind == LaneKind::Audio
        && let Some(clip) = project.clip(id)
    {
        let x = |t| view.axis.x(clip.source_to_timeline(t));
        let edges: Vec<_> = clip
            .audio
            .mute_regions
            .iter()
            .map(|r| (x(r.start), x(r.end)))
            .collect();
        if let Some((index, start)) = hit_region_edge(&edges, pos.x) {
            return Some((id, Zone::Region { index, start }));
        }
    }
    Some((id, zone))
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
        Some(Drag::Clip(edit)) => Some(zone_cursor(edit.zone)),
        Some(Drag::Range { .. }) => Some(CursorIcon::ResizeHorizontal),
        None => hover_cursor(ruler, rows, state, project, view),
    };
    if let Some(icon) = icon {
        ui.ctx().set_cursor_icon(icon);
    }
}

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
        Zone::Start | Zone::End | Zone::Region { .. } | Zone::Fade { .. } => {
            CursorIcon::ResizeHorizontal
        }
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
            menu_clip: None,
            menu_video: false,
            scroll_y: 0.0,
            drag: None,
            used_video: Vec::new(),
            used_audio: Vec::new(),
        }
    }
}

impl TimelineState {
    /// Called when the project drops a track, so the flags stay with their tracks.
    pub fn track_removed(&mut self, track: Track) {
        let used = self.used_mut(track.is_video());
        if (track.index() as usize) < used.len() {
            used.remove(track.index() as usize);
        }
    }

    fn note_used_tracks(&mut self, project: &Project) {
        self.used_video.resize(project.tracks.video as usize, false);
        self.used_audio.resize(project.tracks.audio as usize, false);
        for clip in &project.clips {
            self.used_mut(clip.track.is_video())[clip.track.index() as usize] = true;
        }
    }

    fn used_mut(&mut self, video: bool) -> &mut Vec<bool> {
        if video {
            &mut self.used_video
        } else {
            &mut self.used_audio
        }
    }

    /// Hints that a track emptied this session could go; the last track of a kind can't.
    pub(crate) fn shows_trash(&self, project: &Project, track: Track) -> bool {
        let used = if track.is_video() {
            &self.used_video
        } else {
            &self.used_audio
        };
        used.get(track.index() as usize).copied().unwrap_or(false)
            && project.tracks.count(track.is_video()) > 1
            && project.clips_on(track).next().is_none()
    }
}

/// Drawn after the painted timeline so the widgets stay on top of it.
fn track_controls(
    ui: &mut Ui,
    state: &mut TimelineState,
    project: &mut Project,
    areas: &Areas,
    view: &View,
) -> Option<Track> {
    ui.scope_builder(UiBuilder::new().max_rect(areas.corner.shrink(2.0)), |ui| {
        ui.menu_button("+ Track", |ui| {
            for (label, video) in [("Video", true), ("Audio", false)] {
                if ui.button(label).clicked() {
                    project.add_track(video);
                    ui.close();
                }
            }
        });
    });
    let mut delete = None;
    for lane in view.lanes.iter().filter(|l| l.is_primary()) {
        let rect = Rect::from_x_y_ranges(areas.header.x_range(), view.lane_rect(lane).y_range());
        let rect = rect.intersect(areas.header);
        if rect.height() <= 0.0 {
            continue;
        }
        let id = ui.make_persistent_id(("track_header", lane.track));
        ui.interact(rect, id, Sense::click()).context_menu(|ui| {
            let last = project.tracks.count(lane.track.is_video()) <= 1;
            if ui.add_enabled(!last, egui::Button::new("Delete")).clicked() {
                if project.clips_on(lane.track).next().is_some() {
                    delete = Some(lane.track);
                } else if project.remove_track(lane.track) {
                    state.track_removed(lane.track);
                }
                ui.close();
            }
        });
        // egui only hovers a hover-sense widget registered after the clickable one beneath it.
        if state.shows_trash(project, lane.track) {
            let id = ui.make_persistent_id(("track_trash", lane.track));
            ui.interact(paint::trash_rect(rect), id, Sense::hover())
                .on_hover_text("This track has no clips");
        }
    }
    delete
}
