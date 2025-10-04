use egui::{Align2, Rect, Sense, TextStyle, Ui, pos2, vec2};

/// Calculate the total spacing between tracks (base egui spacing + extra spacing)
pub fn track_spacing(ui: &Ui) -> f32 {
    ui.spacing().item_spacing.y
}

/// Calculate the row height including spacing for virtual rendering
pub fn spaced_row_height(ui: &Ui) -> f32 {
    let row_height = ui.text_style_height(&TextStyle::Body);
    row_height + track_spacing(ui)
}

/// Draw a clickable heart.
pub fn draw_heart(
    ui: &mut Ui,
    font: egui::FontId,
    pos_x: f32,
    pos_y: f32,
    active: bool,
    right_aligned: bool,
) -> (egui::Response, f32) {
    let size = ui.fonts(|f| f.row_height(&font));

    let pos_x = if right_aligned { pos_x - size } else { pos_x };

    let rect = Rect::from_min_size(pos2(pos_x, pos_y), vec2(size, size));
    let response = ui.allocate_rect(rect, Sense::click());

    let hovered = response.hovered();

    // If:
    // - unstarred, unhovered: invisible
    // - unstarred, hovered: unfilled, red
    // - starred, unhovered: filled, red
    // - starred, hovered: unfilled, white
    let visible = active || hovered;
    let filled = active && !hovered;
    let is_red = (!active && hovered) || (active && !hovered);

    // For some reason, the heart is slightly lower than the text when filled
    let y_offset = if filled { -2.0 } else { 0.0 };

    if visible {
        ui.painter().text(
            pos2(pos_x, pos_y + y_offset),
            Align2::LEFT_TOP,
            egui_phosphor::variants::regular::HEART,
            if filled {
                egui::FontId::new(font.size, egui::FontFamily::Name("phosphor-fill".into()))
            } else {
                font
            },
            if is_red {
                egui::Color32::RED
            } else {
                egui::Color32::WHITE
            },
        );
    }

    (response, size)
}
