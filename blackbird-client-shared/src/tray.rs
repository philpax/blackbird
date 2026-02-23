//! System tray icon and menu integration.
//!
//! Provides a platform-agnostic tray icon with a context menu for playback
//! control. The menu displays the current track, liked status, navigation
//! controls, and playback mode selection.

use blackbird_core as bc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};

pub use tray_icon::TrayIcon;

/// An action requested by the user through the tray icon or its menu.
pub enum TrayAction {
    /// The user clicked "Quit" in the menu.
    Quit,
    /// The user left-clicked the tray icon, requesting window focus.
    FocusWindow,
    /// A menu action was handled that may have changed state, requesting a repaint.
    Repaint,
}

pub struct TrayMenu {
    current_track_item: MenuItem,
    liked_item: CheckMenuItem,
    prev_item: MenuItem,
    next_item: MenuItem,
    playback_mode_items: Vec<(bc::PlaybackMode, CheckMenuItem)>,
    quit_item: MenuItem,
    last_track_display: Option<String>,
    last_playback_mode: bc::PlaybackMode,
    last_starred: Option<bool>,
}

impl TrayMenu {
    pub fn new(
        icon: image::RgbaImage,
        current_playback_mode: bc::PlaybackMode,
    ) -> (TrayIcon, Self) {
        let menu = Menu::new();

        // Current track (disabled, non-clickable).
        let current_track_item = MenuItem::new("Not playing", false, None);
        menu.append(&current_track_item).unwrap();

        // Liked checkbox.
        let liked_item = CheckMenuItem::new("Liked", true, false, None);
        menu.append(&liked_item).unwrap();

        // Separator.
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Previous.
        let prev_item = MenuItem::new("Previous", true, None);
        menu.append(&prev_item).unwrap();

        // Next.
        let next_item = MenuItem::new("Next", true, None);
        menu.append(&next_item).unwrap();

        // Separator.
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Playback modes.
        let playback_modes = [
            bc::PlaybackMode::Sequential,
            bc::PlaybackMode::RepeatOne,
            bc::PlaybackMode::GroupRepeat,
            bc::PlaybackMode::Shuffle,
            bc::PlaybackMode::LikedShuffle,
            bc::PlaybackMode::GroupShuffle,
            bc::PlaybackMode::LikedGroupShuffle,
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

        // Separator.
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Quit.
        let quit_item = MenuItem::new("Quit", true, None);
        menu.append(&quit_item).unwrap();

        let tray_menu = Self {
            current_track_item,
            liked_item,
            prev_item,
            next_item,
            playback_mode_items,
            quit_item,
            last_track_display: None,
            last_playback_mode: current_playback_mode,
            last_starred: None,
        };

        let tray_icon = Self::build_tray_icon(icon, &menu);

        (tray_icon, tray_menu)
    }

    fn build_tray_icon(icon: image::RgbaImage, menu: &Menu) -> TrayIcon {
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

    /// Drain tray icon click events and return `FocusWindow` on left-click.
    pub fn handle_icon_events(&self) -> Option<TrayAction> {
        let mut action = None;
        while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            if let tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                ..
            } = event
            {
                action = Some(TrayAction::FocusWindow);
            }
        }
        action
    }

    /// Process a single menu event, calling the appropriate logic method.
    /// Returns `Quit` when the quit item is clicked, `Repaint` for actions
    /// that change playback state, or `None` if no event was pending.
    pub fn handle_menu_events(&self, logic: &bc::Logic) -> Option<TrayAction> {
        let event = MenuEvent::receiver().try_recv().ok()?;

        if event.id == self.prev_item.id() {
            logic.previous();
            Some(TrayAction::Repaint)
        } else if event.id == self.next_item.id() {
            logic.next();
            Some(TrayAction::Repaint)
        } else if event.id == self.liked_item.id() {
            if let Some(details) = logic.get_track_display_details() {
                logic.set_track_starred(&details.track_id, !details.starred);
            }
            Some(TrayAction::Repaint)
        } else if event.id == self.quit_item.id() {
            Some(TrayAction::Quit)
        } else {
            // Check if it's a playback mode item.
            for (mode, item) in &self.playback_mode_items {
                if event.id == item.id() {
                    logic.set_playback_mode(*mode);
                    return Some(TrayAction::Repaint);
                }
            }
            None
        }
    }

    /// Update the menu state based on current playback state.
    pub fn update(&mut self, logic: &bc::Logic, tray_icon: &TrayIcon) {
        // Update tooltip.
        tray_icon
            .set_tooltip(Some(Self::build_tooltip(
                logic.get_track_display_details().as_ref(),
            )))
            .ok();

        // Update menu current track display and liked status.
        let track_details = logic.get_track_display_details();
        let track_display = track_details.as_ref().map(|details| details.to_string());
        if track_display != self.last_track_display {
            let text = track_display
                .as_deref()
                .unwrap_or("Not playing")
                .to_string();
            self.current_track_item.set_text(text);
            self.last_track_display = track_display;
        }

        // Update liked checkbox.
        let current_starred = track_details.as_ref().map(|d| d.starred);
        if current_starred != self.last_starred {
            if let Some(starred) = current_starred {
                self.liked_item.set_checked(starred);
                self.liked_item.set_enabled(true);
            } else {
                self.liked_item.set_checked(false);
                self.liked_item.set_enabled(false);
            }
            self.last_starred = current_starred;
        }

        // Update menu playback mode checkmarks.
        let current_mode = logic.get_playback_mode();
        if current_mode != self.last_playback_mode {
            for (mode, item) in &self.playback_mode_items {
                item.set_checked(*mode == current_mode);
            }
            self.last_playback_mode = current_mode;
        }
    }
}

/// Initialize platform-specific requirements for tray icon support.
///
/// On Linux, this initializes GTK on the main thread. On other platforms
/// this is a no-op.
pub fn init_platform() {
    #[cfg(target_os = "linux")]
    {
        gtk::init().expect("failed to initialize gtk");
    }
}

/// Process pending platform events without blocking.
///
/// On Linux, this pumps the GTK event loop (non-blocking). Must be called
/// periodically from the main thread to keep the tray icon responsive.
/// On other platforms this is a no-op.
pub fn pump_platform_events() {
    #[cfg(target_os = "linux")]
    {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}
