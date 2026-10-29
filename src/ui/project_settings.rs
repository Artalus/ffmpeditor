use egui::{ComboBox, Context, DragValue, Grid, TextEdit};

use crate::model::ProjectSettings;
use crate::ui::dialogs::{Outcome, confirm_buttons, modal};

pub struct Preset {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Suggested rate, roughly what streaming sites recommend for uploads.
    pub video_bitrate_kbps: u32,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "720p30",
        width: 1280,
        height: 720,
        fps: 30.0,
        video_bitrate_kbps: 5_000,
    },
    Preset {
        name: "1080p60",
        width: 1920,
        height: 1080,
        fps: 60.0,
        video_bitrate_kbps: 12_000,
    },
    Preset {
        name: "1440p60",
        width: 2560,
        height: 1440,
        fps: 60.0,
        video_bitrate_kbps: 24_000,
    },
    Preset {
        name: "4K60",
        width: 3840,
        height: 2160,
        fps: 60.0,
        video_bitrate_kbps: 53_000,
    },
];

const VIDEO_CODECS: &[&str] = &[
    "libx264",
    "libx265",
    "h264_nvenc",
    "hevc_nvenc",
    "av1_nvenc",
    "libvpx-vp9",
];
const AUDIO_CODECS: &[&str] = &["aac", "libopus", "libmp3lame"];
const FORMATS: &[&str] = &["mp4", "mkv", "mov", "webm"];

/// Edits a copy; the project only changes on OK, so the whole edit is one undo step.
pub struct ProjectSettingsDialog {
    settings: ProjectSettings,
}

impl ProjectSettingsDialog {
    pub fn new(settings: &ProjectSettings) -> Self {
        Self {
            settings: settings.clone(),
        }
    }

    pub fn show(&mut self, ctx: &Context) -> Outcome<ProjectSettings> {
        modal(ctx, "Project settings", |ui| {
            let s = &mut self.settings;
            Grid::new("project_settings")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Preset");
                    preset_combo(ui, s);
                    ui.end_row();

                    ui.label("Resolution");
                    ui.horizontal(|ui| {
                        ui.add(DragValue::new(&mut s.width).range(16..=8192));
                        ui.label("×");
                        ui.add(DragValue::new(&mut s.height).range(16..=8192));
                    });
                    ui.end_row();

                    ui.label("Frame rate");
                    ui.add(
                        DragValue::new(&mut s.fps)
                            .range(1.0..=240.0)
                            .speed(0.1)
                            .max_decimals(3)
                            .suffix(" fps"),
                    );
                    ui.end_row();

                    ui.label("Video codec");
                    editable_combo(ui, &mut s.video_codec, VIDEO_CODECS);
                    ui.end_row();

                    ui.label("Video bitrate");
                    bitrate(ui, &mut s.video_bitrate_kbps);
                    ui.end_row();

                    ui.label("Audio codec");
                    combo(ui, "audio_codec", &mut s.audio_codec, AUDIO_CODECS);
                    ui.end_row();

                    ui.label("Audio bitrate");
                    bitrate(ui, &mut s.audio_bitrate_kbps);
                    ui.end_row();

                    ui.label("Format");
                    combo(ui, "format", &mut s.format, FORMATS);
                    ui.end_row();
                });
            if !s.width.is_multiple_of(2) || !s.height.is_multiple_of(2) {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Most codecs need even width and height.",
                );
            }
            // Codecs reach ffmpeg verbatim, so stray spaces would break the command.
            let accepted = (!s.video_codec.trim().is_empty()).then(|| ProjectSettings {
                video_codec: s.video_codec.trim().to_owned(),
                audio_codec: s.audio_codec.trim().to_owned(),
                ..s.clone()
            });
            confirm_buttons(ui, [("OK", accepted)])
        })
    }
}

pub fn matching_preset(settings: &ProjectSettings) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.matches(settings))
}

fn preset_combo(ui: &mut egui::Ui, settings: &mut ProjectSettings) {
    let current = matching_preset(settings).map_or("Custom", |p| p.name);
    ComboBox::from_id_salt("preset")
        .selected_text(current)
        .show_ui(ui, |ui| {
            for preset in PRESETS {
                if ui
                    .selectable_label(preset.name == current, preset.name)
                    .clicked()
                {
                    preset.apply(settings);
                }
            }
        });
}

/// Free text with a list of common values, for encoders the list does not know.
fn editable_combo(ui: &mut egui::Ui, value: &mut String, options: &[&str]) {
    ui.horizontal(|ui| {
        ui.add(TextEdit::singleline(value).desired_width(120.0));
        ui.menu_button("⏷", |ui| {
            for option in options {
                if ui
                    .selectable_label(value.as_str() == *option, *option)
                    .clicked()
                {
                    *value = (*option).to_owned();
                }
            }
        });
    });
}

fn bitrate(ui: &mut egui::Ui, kbps: &mut u32) {
    ui.add(
        DragValue::new(kbps)
            .range(8..=500_000)
            .speed(10.0)
            .suffix(" kbps"),
    );
}

fn combo(ui: &mut egui::Ui, id: &str, value: &mut String, options: &[&str]) {
    ComboBox::from_id_salt(id)
        .selected_text(value.as_str())
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(value, (*option).to_owned(), *option);
            }
        });
}

impl Preset {
    fn matches(&self, settings: &ProjectSettings) -> bool {
        settings.width == self.width && settings.height == self.height && settings.fps == self.fps
    }

    fn apply(&self, settings: &mut ProjectSettings) {
        settings.width = self.width;
        settings.height = self.height;
        settings.fps = self.fps;
        settings.video_bitrate_kbps = self.video_bitrate_kbps;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_1080p60() {
        let preset = matching_preset(&ProjectSettings::default()).unwrap();
        assert_eq!(preset.name, "1080p60");
    }

    #[test]
    fn applied_preset_matches_and_keeps_codecs() {
        let mut settings = ProjectSettings {
            video_codec: "hevc_nvenc".into(),
            ..Default::default()
        };
        let preset = PRESETS.iter().find(|p| p.name == "720p30").unwrap();
        preset.apply(&mut settings);
        assert_eq!(matching_preset(&settings).unwrap().name, "720p30");
        assert_eq!(
            (settings.width, settings.height, settings.fps),
            (1280, 720, 30.0)
        );
        assert_eq!(settings.video_codec, "hevc_nvenc");
    }

    #[test]
    fn bitrate_does_not_affect_match() {
        let settings = ProjectSettings {
            video_bitrate_kbps: 1,
            ..Default::default()
        };
        assert_eq!(matching_preset(&settings).unwrap().name, "1080p60");
    }

    #[test]
    fn odd_settings_are_custom() {
        let settings = ProjectSettings {
            fps: 59.94,
            ..Default::default()
        };
        assert!(matching_preset(&settings).is_none());
    }

    #[test]
    fn preset_names_are_unique() {
        for (i, a) in PRESETS.iter().enumerate() {
            for b in &PRESETS[i + 1..] {
                assert_ne!(a.name, b.name);
            }
        }
    }
}
