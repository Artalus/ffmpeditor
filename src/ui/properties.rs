use egui::{Color32, Popup, Response, Ui, color_picker};

use crate::model::{Clip, Content, TextProps, Transform};

const PANEL_WIDTH: f32 = 240.0;
const SIZE_RANGE: std::ops::RangeInclusive<f64> = 4.0..=512.0;
const PERCENT_RANGE: std::ops::RangeInclusive<f64> = 0.0..=100.0;
const SCALE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=400.0;
const TEXT_ANCHORS: [f64; 3] = [10.0, 50.0, 90.0];
const ANCHOR_LABELS: [[&str; 3]; 3] = [["↖", "↑", "↗"], ["←", "•", "→"], ["↙", "↓", "↘"]];

/// Side panel for a video-track clip. `fit` is the share of the frame width and height that
/// media covers at 100 % scale, so presets keep it inside the frame; unused for text.
/// Returns true while a field is being edited, so the app snapshots one undo step per edit
/// rather than per keystroke.
pub fn show(ui: &mut Ui, clip: &mut Clip, fit: (f64, f64)) -> bool {
    let mut editing = false;
    let mut track = |r: Response| editing |= r.has_focus() || r.dragged();
    let scalable = clip.scalable();
    let transform = &mut clip.transform;
    egui::Panel::right("properties")
        .default_size(PANEL_WIDTH)
        .show(ui, |ui| {
            if let Content::Text { text } = &mut clip.content {
                text_section(ui, text, &mut track);
                ui.separator();
            }
            ui.heading("Transform");
            ui.horizontal(|ui| {
                for (label, value) in [("X", &mut transform.x), ("Y", &mut transform.y)] {
                    ui.label(label);
                    track(percent(ui, value, PERCENT_RANGE));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Opacity");
                track(percent(ui, &mut transform.opacity, PERCENT_RANGE));
            });
            if scalable {
                ui.horizontal(|ui| {
                    ui.label("Scale");
                    track(percent(ui, &mut transform.scale, SCALE_RANGE));
                });
            }
            let anchors = if !scalable {
                [TEXT_ANCHORS; 2]
            } else {
                let half = |fit: f64| (fit * transform.scale / 2.0).min(50.0);
                [fit.0, fit.1].map(|fit| [half(fit), 50.0, 100.0 - half(fit)])
            };
            egui::Grid::new("anchors").show(ui, |ui| {
                for (labels, y) in ANCHOR_LABELS.iter().zip(anchors[1]) {
                    for (label, x) in labels.iter().zip(anchors[0]) {
                        if ui.button(*label).clicked() {
                            (transform.x, transform.y) = (x, y);
                        }
                    }
                    ui.end_row();
                }
            });
            if ui.button("Reset").clicked() {
                *transform = Transform::default();
            }
        });
    editing || Popup::is_any_open(ui.ctx())
}

fn text_section(ui: &mut Ui, text: &mut TextProps, track: &mut impl FnMut(Response)) {
    ui.heading("Text");
    track(
        ui.add(
            egui::TextEdit::multiline(&mut text.text)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        ),
    );
    ui.horizontal(|ui| {
        ui.label("Size");
        track(
            ui.add(
                egui::DragValue::new(&mut text.size)
                    .range(SIZE_RANGE)
                    .suffix(" pt"),
            ),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Color");
        track(color_button(ui, &mut text.color));
        ui.checkbox(&mut text.outline, "Outline");
        ui.add_enabled_ui(text.outline, |ui| {
            track(color_button(ui, &mut text.outline_color));
        });
    });
}

fn percent(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>) -> Response {
    ui.add(egui::DragValue::new(value).range(range).suffix(" %"))
}

/// Color picker button editing a `#rrggbbaa` string in place.
fn color_button(ui: &mut Ui, hex: &mut String) -> Response {
    let mut color = parse_color(hex);
    let response =
        color_picker::color_edit_button_srgba(ui, &mut color, color_picker::Alpha::OnlyBlend);
    if response.changed() {
        let [r, g, b, a] = color.to_srgba_unmultiplied();
        *hex = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
    }
    response
}

/// `#rrggbbaa`; anything malformed shows as opaque white.
fn parse_color(hex: &str) -> Color32 {
    let channel = |i: usize| {
        hex.get(1 + 2 * i..3 + 2 * i)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
    };
    match (channel(0), channel(1), channel(2), channel(3)) {
        (Some(r), Some(g), Some(b), Some(a)) => Color32::from_rgba_unmultiplied(r, g, b, a),
        _ => Color32::WHITE,
    }
}
