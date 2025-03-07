pub struct RightAlignedWidget<T: egui::Widget>(pub T);
impl<T: egui::Widget> egui::Widget for RightAlignedWidget<T> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            self.0.ui(ui)
        })
        .inner
    }
}
