use egui::{Align, Layout};

use crate::model::Span;
use crate::ui::format_time;

pub fn show(
    ui: &mut egui::Ui,
    playhead: f64,
    duration: f64,
    selection: Option<Span>,
    ffmpeg: &Result<String, String>,
    export_progress: Option<f32>,
) {
    ui.horizontal(|ui| {
        ui.monospace(timecode(playhead));
        ui.weak("/");
        ui.monospace(timecode(duration));
        ui.separator();
        ui.label(selection_text(selection));
        if let Some(progress) = export_progress {
            ui.separator();
            ui.label(format!("exporting {:.0}%", progress * 100.0));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            match ffmpeg {
                Ok(version) => ui.weak(short_version(version)),
                Err(err) => ui.colored_label(ui.visuals().error_fg_color, format!("ffmpeg: {err}")),
            };
        });
    });
}

/// `h:mm:ss.mmm`
pub fn timecode(seconds: f64) -> String {
    format_time(seconds, 3, true)
}

fn selection_text(selection: Option<Span>) -> String {
    match selection {
        Some(span) => format!(
            "selection {} – {} ({})",
            timecode(span.start),
            timecode(span.end),
            timecode(span.len())
        ),
        None => "no selection".to_owned(),
    }
}

/// Drops the copyright tail of `ffmpeg -version`, which would crowd the status bar.
fn short_version(version: &str) -> &str {
    version.split(" Copyright").next().unwrap_or(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_shows_bounds_and_length() {
        assert_eq!(selection_text(None), "no selection");
        assert_eq!(
            selection_text(Some(Span::new(1.0, 4.5))),
            "selection 0:00:01.000 – 0:00:04.500 (0:00:03.500)"
        );
    }

    #[test]
    fn version_without_copyright() {
        assert_eq!(
            short_version(
                "ffmpeg version 7.1-full_build Copyright (c) 2000-2024 the FFmpeg developers"
            ),
            "ffmpeg version 7.1-full_build"
        );
        assert_eq!(short_version("ffmpeg version n7"), "ffmpeg version n7");
    }
}
