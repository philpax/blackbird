use egui::{TextStyle, Ui};

/// Calculate the total spacing between tracks (base egui spacing + extra spacing)
pub fn track_spacing(ui: &Ui) -> f32 {
    ui.spacing().item_spacing.y
}

/// Calculate the row height including spacing for virtual rendering
pub fn spaced_row_height(ui: &Ui) -> f32 {
    let row_height = ui.text_style_height(&TextStyle::Body);
    row_height + track_spacing(ui)
}
