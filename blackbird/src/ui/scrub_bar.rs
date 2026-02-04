use std::time::Duration;

use blackbird_core::util::seconds_to_hms_string;
use egui::{Align, Label, Layout, RichText, Slider, Ui, style::HandleShape};

use crate::{bc, config::Config, ui::style::StyleExt};

pub fn ui(ui: &mut Ui, logic: &mut bc::Logic, config: &Config) {
    ui.horizontal(|ui| {
        let (position_secs, duration_secs) = logic
            .get_track_display_details()
            .map(|pi| {
                (
                    pi.track_position.as_secs_f32(),
                    pi.track_duration.as_secs_f32(),
                )
            })
            .unwrap_or_default();

        // Position/duration text
        let [position_hms, duration_hms] =
            [position_secs, duration_secs].map(|s| seconds_to_hms_string(s as u32, true));
        ui.add(
            Label::new(
                RichText::new(format!("{position_hms} / {duration_hms}"))
                    .color(config.style.track_duration_color32()),
            )
            .selectable(false),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Volume slider
            ui.add_space(ui.style().spacing.window_margin.right as f32);
            let mut volume = logic.get_volume();
            let volume_response = ui.add_sized(
                [ui.available_width().min(80.0), ui.available_height()],
                Slider::new(&mut volume, 0.0..=1.0)
                    .show_value(false)
                    .handle_shape(HandleShape::Rect { aspect_ratio: 0.75 }),
            );
            if volume_response.changed() {
                logic.set_volume(volume);
            }
            ui.label(egui_phosphor::regular::SPEAKER_HIGH);

            // Separator
            ui.separator();

            // Scrub bar
            let mut slider_position = position_secs;
            let slider_duration = duration_secs.max(1.0);
            ui.style_mut().spacing.slider_width = ui.available_width();
            let slider_response = ui.add(
                Slider::new(&mut slider_position, 0.0..=slider_duration)
                    .show_value(false)
                    .handle_shape(HandleShape::Rect { aspect_ratio: 2.0 }),
            );
            if slider_response.changed() {
                let seek_position = Duration::from_secs_f32(slider_position);
                logic.seek_current(seek_position);
            }
        });
    });
}
