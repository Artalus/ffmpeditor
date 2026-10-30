use egui::{Color32, Popup, Response, Ui, color_picker};

use crate::model::{Clip, Content};

const PANEL_WIDTH: f32 = 240.0;
const SIZE_RANGE: std::ops::RangeInclusive<f64> = 4.0..=512.0;
const PERCENT_RANGE: std::ops::RangeInclusive<f64> = 0.0..=100.0;
const ANCHOR_PERCENT: [f64; 3] = [10.0, 50.0, 90.0];
const ANCHOR_LABELS: [[&str; 3]; 3] = [["↖", "↑", "↗"], ["←", "•", "→"], ["↙", "↓", "↘"]];

/// Side panel for a text clip. Returns true while a field is being edited,
/// so the app snapshots one undo step per edit rather than per keystroke.
pub fn show(ui: &mut Ui, clip: &mut Clip) -> bool {
    let Content::Text { text } = &mut clip.content else {
        return false;
    };
    let transform = &mut clip.transform;
    let mut editing = false;
    let mut track = |r: Response| editing |= r.has_focus() || r.dragged();
    egui::Panel::right("properties")
        .default_size(PANEL_WIDTH)
        .show(ui, |ui| {
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
            ui.separator();
            ui.horizontal(|ui| {
                for (label, value) in [("X", &mut transform.x), ("Y", &mut transform.y)] {
                    ui.label(label);
                    track(
                        ui.add(
                            egui::DragValue::new(value)
                                .range(PERCENT_RANGE)
                                .suffix(" %"),
                        ),
                    );
                }
            });
            egui::Grid::new("anchors").show(ui, |ui| {
                for (labels, y) in ANCHOR_LABELS.iter().zip(ANCHOR_PERCENT) {
                    for (label, x) in labels.iter().zip(ANCHOR_PERCENT) {
                        if ui.button(*label).clicked() {
                            (transform.x, transform.y) = (x, y);
                        }
                    }
                    ui.end_row();
                }
            });
        });
    editing || Popup::is_any_open(ui.ctx())
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
