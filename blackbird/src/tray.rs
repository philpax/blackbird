use blackbird_core as bc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};

pub struct TrayMenu {
    current_track_item: MenuItem,
    prev_item: MenuItem,
    next_item: MenuItem,
    playback_mode_items: Vec<(bc::PlaybackMode, CheckMenuItem)>,
    quit_item: MenuItem,
    last_track_display: Option<String>,
    last_playback_mode: bc::PlaybackMode,
}

impl TrayMenu {
    pub fn new(
        icon: image::RgbaImage,
        current_playback_mode: bc::PlaybackMode,
    ) -> (tray_icon::TrayIcon, Self) {
        let menu = Menu::new();

        // Current track (disabled, non-clickable)
        let current_track_item = MenuItem::new("Not playing", false, None);
        menu.append(&current_track_item).unwrap();

        // Separator
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Previous
        let prev_item = MenuItem::new("Previous", true, None);
        menu.append(&prev_item).unwrap();

        // Next
        let next_item = MenuItem::new("Next", true, None);
        menu.append(&next_item).unwrap();

        // Separator
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Playback modes - using an array instead of individual fields
        let playback_modes = [
            bc::PlaybackMode::Sequential,
            bc::PlaybackMode::RepeatOne,
            bc::PlaybackMode::GroupRepeat,
            bc::PlaybackMode::Shuffle,
            bc::PlaybackMode::GroupShuffle,
        ];

        let playback_mode_items: Vec<(bc::PlaybackMode, CheckMenuItem)> = playback_modes
            .iter()
            .map(|&mode| {
                let item =
                    CheckMenuItem::new(mode.as_str(), true, mode == current_playback_mode, None);
                menu.append(&item).unwrap();
                (mode, item)
            })
            .collect();

        // Separator
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Quit
        let quit_item = MenuItem::new("Quit", true, None);
        menu.append(&quit_item).unwrap();

        let tray_menu = Self {
            current_track_item,
            prev_item,
            next_item,
            playback_mode_items,
            quit_item,
            last_track_display: None,
            last_playback_mode: current_playback_mode,
        };

        let tray_icon = Self::build_tray_icon(icon, &menu);

        (tray_icon, tray_menu)
    }

    fn build_tray_icon(icon: image::RgbaImage, menu: &Menu) -> tray_icon::TrayIcon {
        let (icon_width, icon_height) = icon.dimensions();
        tray_icon::TrayIconBuilder::new()
            .with_tooltip(Self::build_tooltip(None))
            .with_icon(
                tray_icon::Icon::from_rgba(icon.into_vec(), icon_width, icon_height).unwrap(),
            )
            .with_menu(Box::new(menu.clone()))
            .with_menu_on_left_click(false)
            .build()
            .unwrap()
    }

    fn build_tooltip(track_display_details: Option<&bc::TrackDisplayDetails>) -> String {
        track_display_details.map_or_else(|| "Not playing".to_string(), |d| d.to_string())
    }

    /// Handle menu events
    pub fn handle_events(&self, logic: &bc::Logic, ctx: &egui::Context) {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.prev_item.id() {
                logic.previous();
                ctx.request_repaint();
            } else if event.id == self.next_item.id() {
                logic.next();
                ctx.request_repaint();
            } else if event.id == self.quit_item.id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                // Check if it's a playback mode item
                for (mode, item) in &self.playback_mode_items {
                    if event.id == item.id() {
                        logic.set_playback_mode(*mode);
                        ctx.request_repaint();
                        break;
                    }
                }
            }
        }
    }

    /// Update the menu state based on current playback state
    pub fn update(&mut self, logic: &bc::Logic, tray_icon: &tray_icon::TrayIcon) {
        // Update tooltip
        tray_icon
            .set_tooltip(Some(Self::build_tooltip(
                logic.get_track_display_details().as_ref(),
            )))
            .ok();

        // Update menu current track display
        let track_display = logic
            .get_track_display_details()
            .map(|details| details.to_string());
        if track_display != self.last_track_display {
            let text = track_display
                .as_deref()
                .unwrap_or("Not playing")
                .to_string();
            self.current_track_item.set_text(text);
            self.last_track_display = track_display;
        }

        // Update menu playback mode checkmarks
        let current_mode = logic.get_playback_mode();
        if current_mode != self.last_playback_mode {
            for (mode, item) in &self.playback_mode_items {
                item.set_checked(*mode == current_mode);
            }
            self.last_playback_mode = current_mode;
        }
    }
}
