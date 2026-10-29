//! Drawing of ruler, track headers, lanes with clips, selection and playhead.

use egui::{
    Align2, Color32, FontId, Mesh, Painter, Pos2, Rangef, Rect, Shape, Stroke, StrokeKind, Visuals,
    pos2, vec2,
};

use super::TimelineState;
use super::layout::{Areas, Lane, LaneKind, Ticks, TimeAxis, View, content_height, format_time};
use crate::media::MediaCache;
use crate::model::{Clip, Project};

const CLIP_ROUNDING: f32 = 3.0;
const VIDEO_CLIP: Color32 = Color32::from_rgb(50, 80, 135);
const AUDIO_CLIP: Color32 = Color32::from_rgb(35, 100, 80);
const ERROR_CLIP: Color32 = Color32::from_rgb(140, 35, 35);
const CLIP_BORDER: Color32 = Color32::from_black_alpha(160);
const SELECTED_BORDER: Color32 = Color32::WHITE;
const WAVEFORM: Color32 = Color32::from_rgb(140, 225, 185);
const WAVEFORM_MUTED: Color32 = Color32::from_rgb(85, 125, 110);
const MUTE_SHADE: Color32 = Color32::from_rgba_unmultiplied_const(110, 0, 0, 90);
const FADE_SHADE: Color32 = Color32::from_black_alpha(120);
const FADE_LINE: Color32 = Color32::from_gray(230);
const CROSSFADE: Color32 = Color32::from_rgb(255, 205, 80);
const LABEL_BG: Color32 = Color32::from_black_alpha(150);
const LABEL_TEXT: Color32 = Color32::from_gray(235);
const ERROR_TEXT: Color32 = Color32::from_rgb(255, 150, 150);
const PLAYHEAD: Color32 = Color32::from_rgb(255, 80, 60);
const FULL_UV: Rect = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));

pub struct Scene<'a> {
    pub areas: &'a Areas,
    pub view: &'a View,
    pub state: &'a TimelineState,
    pub project: &'a Project,
    pub media: &'a MediaCache,
    pub visuals: &'a Visuals,
}

pub fn timeline(painter: &Painter, scene: &Scene) {
    let areas = scene.areas;
    lanes(&painter.with_clip_rect(areas.rows), scene);
    scrollbar(painter, scene);
    header(&painter.with_clip_rect(areas.header), scene);
    ruler(&painter.with_clip_rect(areas.ruler), scene);
    corner(&painter.with_clip_rect(areas.corner), scene);
    markers(
        &painter.with_clip_rect(areas.ruler.union(areas.rows)),
        scene,
    );
}

fn lanes(painter: &Painter, scene: &Scene) {
    painter.rect_filled(scene.areas.rows, 0.0, scene.visuals.extreme_bg_color);
    for lane in &scene.view.lanes {
        let rect = scene.view.lane_rect(lane);
        if !painter.clip_rect().intersects(rect) {
            continue;
        }
        painter.rect_filled(rect, 0.0, lane_fill(lane, scene.visuals));
        let clips = scene.view.visible_clips(scene.project, lane.track);
        for clip in &clips {
            clip_in_lane(painter, scene, lane, rect, clip);
        }
        for overlap in scene.project.crossfades(lane.track) {
            let axis = &scene.view.axis;
            cross_mark(painter, axis.x(overlap.start), axis.x(overlap.end), rect);
        }
    }
}

fn lane_fill(lane: &Lane, visuals: &Visuals) -> Color32 {
    if lane.is_primary() {
        visuals.panel_fill
    } else {
        visuals
            .panel_fill
            .lerp_to_gamma(visuals.extreme_bg_color, 0.4)
    }
}

fn clip_in_lane(painter: &Painter, scene: &Scene, lane: &Lane, lane_rect: Rect, clip: &Clip) {
    let axis = &scene.view.axis;
    let y = lane_rect.y_range().shrink(1.0);
    let rect = Rect::from_x_y_ranges(axis.x(clip.start)..=axis.x(clip.end()), y);
    // Huge offscreen extents lose f32 precision at the visible edge; draw only the visible part.
    let view_x = painter.clip_rect().x_range().expand(CLIP_ROUNDING + 2.0);
    let drawn = Rect::from_x_y_ranges(
        rect.left().max(view_x.min)..=rect.right().min(view_x.max),
        y,
    );
    let error = scene.media.error(clip.source);
    let selected = scene.state.selected_clip == Some(clip.id);
    let base = match (error, lane.kind) {
        (Some(_), _) => ERROR_CLIP,
        (None, LaneKind::Video) => VIDEO_CLIP,
        (None, LaneKind::Audio) => AUDIO_CLIP,
    };
    let fill = if selected {
        base.lerp_to_gamma(Color32::WHITE, 0.2)
    } else {
        base
    };
    painter.rect_filled(drawn, CLIP_ROUNDING, fill);

    let inner = painter.with_clip_rect(rect.shrink(1.0));
    match lane.kind {
        LaneKind::Video => {
            thumbnails(&inner, scene, clip, rect);
            fades(&inner, axis, rect, clip.video.fade_in, clip.video.fade_out);
        }
        LaneKind::Audio => {
            waveform(&inner, scene, clip, rect);
            mute_regions(&inner, axis, rect, clip);
            fades(&inner, axis, rect, clip.audio.fade_in, clip.audio.fade_out);
        }
    }
    if lane.is_primary() {
        label(&inner, scene, clip, error);
    }

    let border = if selected {
        Stroke::new(2.0, SELECTED_BORDER)
    } else {
        Stroke::new(1.0, CLIP_BORDER)
    };
    painter.rect_stroke(drawn, CLIP_ROUNDING, border, StrokeKind::Inside);
}

/// Tiles are anchored at the clip start so they travel with the clip.
fn thumbnails(painter: &Painter, scene: &Scene, clip: &Clip, rect: Rect) {
    let aspect = scene
        .media
        .info(clip.source)
        .and_then(|info| info.video)
        .filter(|video| video.width > 0 && video.height > 0)
        .map_or(16.0 / 9.0, |video| video.width as f32 / video.height as f32);
    let tile = rect.height() * aspect;
    if tile < 1.0 {
        return;
    }
    let visible = painter.clip_rect().x_range();
    let end = visible.max.min(rect.right());
    let first = ((visible.min - rect.left()) / tile).floor().max(0.0);
    let mut x = rect.left() + first * tile;
    while x < end {
        let at = clip
            .timeline_to_source(scene.view.axis.time(x))
            .clamp(clip.source_in, clip.source_out());
        if let Some(texture) = scene.media.thumbnail(clip.source, at) {
            let tile_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(tile, rect.height()));
            painter.image(texture.id(), tile_rect, FULL_UV, Color32::WHITE);
        }
        x += tile;
    }
}

fn waveform(painter: &Painter, scene: &Scene, clip: &Clip, rect: Rect) {
    let mid = rect.center().y;
    let Some(waveform) = scene.media.waveform(clip.source) else {
        painter.hline(rect.x_range(), mid, Stroke::new(1.0, WAVEFORM_MUTED));
        return;
    };
    let axis = &scene.view.axis;
    let half = rect.height() / 2.0 - 2.0;
    let gain = clip.audio.volume as f32;
    let visible = painter.clip_rect().x_range();
    let end = visible.max.min(rect.right());
    let mut mesh = Mesh::default();
    let mut x = visible.min.max(rect.left()).floor();
    while x < end {
        let from = clip.timeline_to_source(axis.time(x));
        let to = clip.timeline_to_source(axis.time(x + 1.0));
        let muted = clip.audio.muted || clip.audio.mute_regions.iter().any(|r| r.contains(from));
        let h = (waveform.peak(from, to) * gain).min(1.0) * half + 0.5;
        let color = if muted { WAVEFORM_MUTED } else { WAVEFORM };
        mesh.add_colored_rect(Rect::from_x_y_ranges(x..=x + 1.0, mid - h..=mid + h), color);
        x += 1.0;
    }
    painter.add(Shape::mesh(mesh));
}

fn mute_regions(painter: &Painter, axis: &TimeAxis, rect: Rect, clip: &Clip) {
    for region in &clip.audio.mute_regions {
        let x0 = axis
            .x(clip.source_to_timeline(region.start))
            .max(rect.left());
        let x1 = axis
            .x(clip.source_to_timeline(region.end))
            .min(rect.right());
        if x1 > x0 {
            painter.rect_filled(
                Rect::from_x_y_ranges(x0..=x1, rect.y_range()),
                0.0,
                MUTE_SHADE,
            );
        }
    }
}

fn fades(painter: &Painter, axis: &TimeAxis, rect: Rect, fade_in: f64, fade_out: f64) {
    let width = |fade: f64| (axis.pixels_per_second as f64 * fade) as f32;
    if fade_in > 0.0 {
        let x = (rect.left() + width(fade_in)).min(rect.right());
        let top = pos2(x, rect.top());
        fade_shape(
            painter,
            [rect.left_top(), top, rect.left_bottom()],
            [rect.left_bottom(), top],
        );
    }
    if fade_out > 0.0 {
        let x = (rect.right() - width(fade_out)).max(rect.left());
        let top = pos2(x, rect.top());
        fade_shape(
            painter,
            [top, rect.right_top(), rect.right_bottom()],
            [top, rect.right_bottom()],
        );
    }
}

fn fade_shape(painter: &Painter, shade: [Pos2; 3], curve: [Pos2; 2]) {
    painter.add(Shape::convex_polygon(
        shade.to_vec(),
        FADE_SHADE,
        Stroke::NONE,
    ));
    painter.line_segment(curve, Stroke::new(1.5, FADE_LINE));
}

fn cross_mark(painter: &Painter, x0: f32, x1: f32, lane_rect: Rect) {
    let y = lane_rect.y_range().shrink(3.0);
    let stroke = Stroke::new(1.5, CROSSFADE);
    painter.line_segment([pos2(x0, y.min), pos2(x1, y.max)], stroke);
    painter.line_segment([pos2(x0, y.max), pos2(x1, y.min)], stroke);
}

/// Pinned to the visible part of the clip so the name stays readable when the start is scrolled off.
fn label(painter: &Painter, scene: &Scene, clip: &Clip, error: Option<&str>) {
    let name = scene
        .project
        .source(clip.source)
        .and_then(|s| s.path.file_stem())
        .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
    let lines =
        std::iter::once((name, LABEL_TEXT)).chain(error.map(|e| (e.to_owned(), ERROR_TEXT)));
    let mut pos = painter.clip_rect().left_top() + vec2(3.0, 2.0);
    for (text, color) in lines {
        let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), color);
        let background = Rect::from_min_size(pos, galley.size()).expand(1.0);
        painter.rect_filled(background, 2.0, LABEL_BG);
        painter.galley(pos, galley, color);
        pos.y = background.bottom() + 2.0;
    }
}

fn scrollbar(painter: &Painter, scene: &Scene) {
    let rows = scene.areas.rows;
    let content = content_height(&scene.view.lanes);
    if content <= rows.height() {
        return;
    }
    let height = rows.height() * rows.height() / content;
    let top = rows.top() + scene.state.scroll_y / content * rows.height();
    let thumb = Rect::from_x_y_ranges(rows.right() - 5.0..=rows.right() - 1.0, top..=top + height);
    painter.rect_filled(
        thumb,
        2.0,
        scene.visuals.weak_text_color().gamma_multiply(0.6),
    );
}

fn header(painter: &Painter, scene: &Scene) {
    let visuals = scene.visuals;
    let area = scene.areas.header;
    painter.rect_filled(area, 0.0, visuals.extreme_bg_color);
    for lane in &scene.view.lanes {
        let rect = Rect::from_x_y_ranges(area.x_range(), scene.view.lane_rect(lane).y_range());
        painter.rect_filled(rect, 0.0, lane_fill(lane, visuals));
        let (text, color) = if lane.is_primary() {
            (lane.track.to_string(), visuals.strong_text_color())
        } else {
            ("audio".to_owned(), visuals.weak_text_color())
        };
        let pos = pos2(rect.left() + 8.0, rect.center().y);
        painter.text(
            pos,
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(12.0),
            color,
        );
    }
    let separator = visuals.widgets.noninteractive.bg_stroke;
    painter.vline(area.right() - 0.5, area.y_range(), separator);
}

fn ruler(painter: &Painter, scene: &Scene) {
    let visuals = scene.visuals;
    let rect = scene.areas.ruler;
    let axis = &scene.view.axis;
    painter.rect_filled(rect, 0.0, visuals.panel_fill);
    if let Some(x) = selection_x(painter, scene) {
        let band = Rect::from_x_y_ranges(x, rect.y_range());
        painter.rect_filled(band, 0.0, visuals.selection.bg_fill.gamma_multiply(0.6));
    }
    let stroke = Stroke::new(1.0, visuals.weak_text_color());
    let ticks = Ticks::for_zoom(axis.pixels_per_second);
    for (t, major) in ticks.within(axis.time(rect.left()), axis.time(rect.right())) {
        let x = axis.x(t);
        let height = rect.height() * if major { 0.5 } else { 0.2 };
        painter.vline(x, rect.bottom() - height..=rect.bottom(), stroke);
        if major {
            let text = format_time(t, ticks.major);
            let pos = pos2(x + 3.0, rect.top() + 1.0);
            painter.text(
                pos,
                Align2::LEFT_TOP,
                text,
                FontId::monospace(10.0),
                visuals.text_color(),
            );
        }
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, stroke);
}

fn corner(painter: &Painter, scene: &Scene) {
    let visuals = scene.visuals;
    let rect = scene.areas.corner;
    painter.rect_filled(rect, 0.0, visuals.panel_fill);
    let text = format_time(scene.state.playhead, 0.01);
    let font = FontId::monospace(11.0);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font,
        visuals.strong_text_color(),
    );
}

/// The ruler draws its part of the selection band itself, beneath the tick labels.
fn markers(painter: &Painter, scene: &Scene) {
    let ruler = scene.areas.ruler;
    let rows = scene.areas.rows;
    if let Some(x) = selection_x(painter, scene) {
        let band = Rect::from_x_y_ranges(x, rows.y_range());
        let fill = scene.visuals.selection.bg_fill.gamma_multiply(0.2);
        painter.rect_filled(band, 0.0, fill);
    }
    let x = scene.view.axis.x(scene.state.playhead);
    if painter.clip_rect().x_range().expand(2.0).contains(x) {
        painter.vline(x, ruler.top()..=rows.bottom(), Stroke::new(1.5, PLAYHEAD));
        let head = vec![
            pos2(x - 5.0, ruler.top()),
            pos2(x + 5.0, ruler.top()),
            pos2(x, ruler.top() + 7.0),
        ];
        painter.add(Shape::convex_polygon(head, PLAYHEAD, Stroke::NONE));
    }
}

fn selection_x(painter: &Painter, scene: &Scene) -> Option<Rangef> {
    let selection = scene.state.selection?;
    let view_x = painter.clip_rect().x_range().expand(2.0);
    let axis = &scene.view.axis;
    let x = Rangef::new(
        axis.x(selection.start).max(view_x.min),
        axis.x(selection.end).min(view_x.max),
    );
    (x.span() > 0.0).then_some(x)
}
