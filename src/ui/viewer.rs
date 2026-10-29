use egui::{
    Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, UiBuilder, Vec2, pos2, vec2,
};

use crate::render::Previewer;

const ERROR_COLOR: Color32 = Color32::from_rgb(255, 120, 120);

/// Fills the available space with the preview frame, letterboxed on black.
pub fn show(ui: &mut egui::Ui, previewer: &Previewer, placeholder: &str) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    match previewer.texture() {
        Some(texture) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(
                texture.id(),
                fit(rect, texture.size_vec2()),
                uv,
                Color32::WHITE,
            );
        }
        None => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                placeholder,
                FontId::proportional(16.0),
                Color32::GRAY,
            );
        }
    }
    if let Some(error) = previewer.error() {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(8.0))
                .layout(Layout::bottom_up(Align::Min)),
        );
        child.label(RichText::new(error).color(ERROR_COLOR));
    }
    if previewer.is_busy() {
        // Bottom right: the top right is where error messages pop up.
        let spinner = Rect::from_min_size(rect.right_bottom() - vec2(28.0, 28.0), vec2(20.0, 20.0));
        egui::Spinner::new()
            .size(20.0)
            .color(Color32::LIGHT_GRAY)
            .paint_at(ui, spinner);
    }
}

/// Largest rect with the aspect ratio of `image` centred in `area`.
pub fn fit(area: Rect, image: Vec2) -> Rect {
    if image.x <= 0.0 || image.y <= 0.0 {
        return area;
    }
    let scale = (area.width() / image.x).min(area.height() / image.y);
    Rect::from_center_size(area.center(), image * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(w, h))
    }

    #[test]
    fn wide_image_is_letterboxed() {
        let fitted = fit(area(400.0, 400.0), vec2(1920.0, 1080.0));
        assert_eq!(fitted.width(), 400.0);
        assert_eq!(fitted.height(), 225.0);
        assert_eq!(fitted.center(), area(400.0, 400.0).center());
    }

    #[test]
    fn tall_image_is_pillarboxed() {
        let fitted = fit(area(800.0, 300.0), vec2(1080.0, 1920.0));
        assert_eq!(fitted.height(), 300.0);
        assert!((fitted.width() - 168.75).abs() < 1e-3);
    }

    #[test]
    fn small_image_is_scaled_up() {
        let fitted = fit(area(1280.0, 720.0), vec2(640.0, 360.0));
        assert_eq!(fitted, area(1280.0, 720.0));
    }

    #[test]
    fn empty_image_takes_whole_area() {
        assert_eq!(fit(area(100.0, 50.0), Vec2::ZERO), area(100.0, 50.0));
    }
}
