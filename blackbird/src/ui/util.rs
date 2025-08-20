/// Extra spacing added between tracks for better readability
pub const EXTRA_TRACK_SPACING: f32 = 4.0;

/// Calculate the total spacing between tracks (base egui spacing + extra spacing)
pub fn track_spacing(ui: &egui::Ui) -> f32 {
    ui.spacing().item_spacing.y + EXTRA_TRACK_SPACING
}

/// Calculate the row height including spacing for virtual rendering
pub fn spaced_row_height(ui: &egui::Ui) -> f32 {
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    row_height + track_spacing(ui)
}
