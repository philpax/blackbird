use egui::{Align2, Context, Pos2, Rect, Sense, TextStyle, Ui, Vec2, ViewportBuilder, pos2, vec2};

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
pub enum HeartPlacement {
    Position { pos: Pos2, right_aligned: bool },
    Space,
}
pub fn draw_heart(
    ui: &mut Ui,
    font: egui::FontId,
    placement: HeartPlacement,
    active: bool,
    show_outline_when_inactive: bool,
) -> (egui::Response, f32) {
    let size = ui.fonts(|f| f.row_height(&font));

    let rect = if let HeartPlacement::Position { pos, right_aligned } = placement {
        let pos_x = if right_aligned { pos.x - size } else { pos.x };
        Rect::from_min_size(pos2(pos_x, pos.y), vec2(size, size))
    } else {
        ui.allocate_space(vec2(size, size)).1
    };
    let response = ui.allocate_rect(rect, Sense::click());

    let hovered = response.hovered();

    // If:
    // - unstarred, unhovered: invisible (or white outline if show_outline_when_inactive)
    // - unstarred, hovered: unfilled, red
    // - starred, unhovered: filled, red
    // - starred, hovered: unfilled, white
    let visible = active || hovered || show_outline_when_inactive;
    let filled = active && !hovered;
    let is_red = (!active && hovered) || (active && !hovered);

    // For some reason, the heart is slightly lower than the text when filled
    let y_offset = if filled { -2.0 } else { 0.0 };

    if visible {
        ui.painter().text(
            pos2(rect.left(), rect.top() + y_offset),
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
