use std::time::{Duration, Instant};

use blackbird_core::{self as bc, PlaybackToLogicMessage};
use blackbird_shared::config::ConfigFile as _;

use crate::{
    config::Config,
    cover_art::CoverArtCache,
    keys,
    log_buffer::LogBuffer,
    ui::{
        album_art_overlay::AlbumArtOverlay,
        library::LibraryState,
        logs::LogsState,
        lyrics::LyricsViewState,
        queue::QueueState,
        search::SearchState,
        settings::SettingsState,
        sidebar::{QueueSidebarState, SidebarState, SimilarSongsState},
    },
};

/// Frame interval for smooth motion (~60 FPS), used by inertia scrolling.
pub const SMOOTH_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Floor on the event loop's frame interval, so a hand-edited `tick_rate_ms`
/// of zero can't turn the loop into a busy spin. Matches the lower bound the
/// settings UI enforces on the field.
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(10);

/// The frame intervals requested by whatever is currently animating.
///
/// Animations are driven by wall-clock time (see [`App::elapsed`]), so a
/// component only has to say how often it wants to be redrawn; the event loop
/// runs at the shortest live request, or the configured tick rate when nothing
/// is animating.
///
/// Requests are tracked per producer because the two producers run at
/// different points in the event loop: `App::tick` for state-driven animation
/// (inertia scrolling) and rendering for view-driven animation (the loading
/// screen). Each producer replaces only its own request, so neither can
/// clobber or outlive the other's.
#[derive(Default)]
struct AnimationRequests {
    /// The shortest interval requested during the most recent `App::tick`.
    from_tick: Option<Duration>,
    /// The shortest interval requested during the most recent render.
    from_render: Option<Duration>,
}

impl AnimationRequests {
    /// The shortest live request, or `None` if nothing is animating.
    fn interval(&self) -> Option<Duration> {
        self.from_tick.into_iter().chain(self.from_render).min()
    }

    /// Narrows `slot` to `interval`, so the shortest request of a frame wins.
    fn request_into(slot: &mut Option<Duration>, interval: Duration) {
        *slot = Some(slot.map_or(interval, |current| current.min(interval)));
    }
}

/// Which panel/mode the UI is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Library,
    Search,
    Lyrics,
    Logs,
    Queue,
    Settings,
}

pub struct App {
    // Core infrastructure (shared across views)
    pub logic: bc::Logic,
    pub config: Config,
    pub cover_art_cache: CoverArtCache,
    pub playback_to_logic_rx: bc::PlaybackToLogicRx,
    pub lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
    pub similar_songs_loaded_rx: std::sync::mpsc::Receiver<bc::SimilarSongsData>,
    pub library_populated_rx: std::sync::mpsc::Receiver<()>,
    pub track_updated_rx: std::sync::mpsc::Receiver<()>,

    // Global UI orchestration
    pub focused_panel: FocusedPanel,
    pub volume_editing: bool,
    pub quit_confirming: bool,
    pub should_quit: bool,
    pub needs_redraw: bool,
    pub mouse_position: Option<(u16, u16)>,
    pub album_art_overlay: Option<AlbumArtOverlay>,
    /// Whether the playback mode dropdown is open.
    pub playback_mode_dropdown: bool,
    /// Clickable regions in the help bar: (x_start, x_end, action).
    pub help_bar_items: Vec<(u16, u16, keys::Action)>,
    /// Whether the user is dragging the scrub bar or volume slider.
    pub scrub_dragging: bool,
    /// Preview seek ratio while dragging the scrub bar (0.0–1.0).
    pub scrub_preview_ratio: Option<f32>,
    /// Whether the user is dragging the lyrics sidebar border to resize it.
    pub lyrics_sidebar_dragging: bool,
    /// The index of the sidebar component boundary being drag-adjusted
    /// (between component `i` and `i+1`), or `None`.
    pub sidebar_component_drag: Option<usize>,
    /// Whether the user is dragging the settings sidebar border to resize it.
    pub settings_sidebar_dragging: bool,
    /// Whether the inline lyrics overlay mode is active. Set when the user is
    /// in inline display mode, independent of sidebar presence/visibility.
    pub inline_lyrics_mode: bool,

    /// The frame intervals requested by whatever is currently animating.
    animations: AnimationRequests,
    /// The zero point for time-based animations.
    started_at: Instant,

    // Config auto-reload
    last_config_check: Instant,

    // Per-view state (owned by their respective modules)
    pub library: LibraryState,
    pub search: SearchState,
    pub lyrics: LyricsViewState,
    pub similar_songs: SimilarSongsState,
    /// The sidebar component order/focus wrapper.
    pub sidebar: SidebarState,
    pub logs: LogsState,
    pub queue: QueueState,
    /// State for the queue sidebar component.
    pub queue_sidebar: QueueSidebarState,
    pub settings: SettingsState,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Config,
        logic: bc::Logic,
        playback_to_logic_rx: bc::PlaybackToLogicRx,
        cover_art_cache: CoverArtCache,
        lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
        similar_songs_loaded_rx: std::sync::mpsc::Receiver<bc::SimilarSongsData>,
        library_populated_rx: std::sync::mpsc::Receiver<()>,
        track_updated_rx: std::sync::mpsc::Receiver<()>,
        log_buffer: LogBuffer,
    ) -> Self {
        // Compute derived flags before `config` is moved into the struct.
        let inline_lyrics_mode = config.layout.show_inline_lyrics;
        let sidebar = SidebarState::from_config(&config);
        Self {
            logic,
            config,
            cover_art_cache,
            playback_to_logic_rx,
            lyrics_loaded_rx,
            similar_songs_loaded_rx,
            library_populated_rx,
            track_updated_rx,

            last_config_check: Instant::now(),

            focused_panel: FocusedPanel::Library,
            volume_editing: false,
            quit_confirming: false,
            should_quit: false,
            needs_redraw: true,
            mouse_position: None,
            album_art_overlay: None,
            playback_mode_dropdown: false,
            help_bar_items: Vec::new(),
            scrub_dragging: false,
            scrub_preview_ratio: None,
            lyrics_sidebar_dragging: false,
            sidebar_component_drag: None,
            settings_sidebar_dragging: false,
            inline_lyrics_mode,
            animations: AnimationRequests::default(),
            started_at: Instant::now(),

            library: LibraryState::new(),
            search: SearchState::new(),
            lyrics: LyricsViewState::new(),
            similar_songs: SimilarSongsState::new(),
            sidebar,
            logs: LogsState::new(log_buffer),
            queue: QueueState::new(),
            queue_sidebar: QueueSidebarState::new(),
            settings: SettingsState::new(),
        }
    }

    /// The time since startup, which is the clock for time-phased animations.
    ///
    /// An animation that reads this plays at the same speed regardless of the
    /// tick rate, which varies with the configuration and with what else is
    /// animating. (Inertia scrolling does not: its velocity is in lines per
    /// tick, so its distance still follows the tick rate.)
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Requests, from view code during a render, that the UI keep animating at
    /// `interval` or faster for as long as the view keeps asking.
    ///
    /// This also marks the UI as needing a redraw, so an animating view
    /// sustains itself: each frame it draws re-requests the next one, and the
    /// animation stops on its own once the view stops asking.
    pub fn request_render_animation(&mut self, interval: Duration) {
        AnimationRequests::request_into(&mut self.animations.from_render, interval);
        self.needs_redraw = true;
    }

    /// Requests, from state updates during [`App::tick`], that the UI keep
    /// animating at `interval` or faster until the state stops asking.
    ///
    /// A state animation can be started by an event rather than by a tick — a
    /// scroll fling begins on mouse release — so its first request lands on
    /// the tick after that event. The event loop ticks on input for exactly
    /// this reason, which keeps `tick` the only thing that decides which
    /// viewports it is responsible for.
    fn request_tick_animation(&mut self, interval: Duration) {
        AnimationRequests::request_into(&mut self.animations.from_tick, interval);
        self.needs_redraw = true;
    }

    /// The interval the event loop should run at: the shortest live animation
    /// request, clamped to at most the configured tick rate and at least
    /// [`MIN_FRAME_INTERVAL`].
    pub fn frame_interval(&self) -> Duration {
        let tick_rate = Duration::from_millis(self.config.general.tick_rate_ms);
        let interval = match self.animations.interval() {
            Some(animation) => animation.min(tick_rate),
            None => tick_rate,
        };
        interval.max(MIN_FRAME_INTERVAL)
    }

    /// Prepares for the render that is about to happen: the redraw flag is
    /// cleared and the render-side animation requests are dropped, so the
    /// frame restates both for itself. Called by the event loop immediately
    /// before drawing, never after — a view that animates requests its next
    /// frame while drawing.
    pub fn begin_render(&mut self) {
        self.needs_redraw = false;
        self.animations.from_render = None;
    }

    pub fn tick(&mut self) {
        // Drop the tick-side animation requests; this tick restates them.
        self.animations.from_tick = None;

        // Keep the runtime sidebar order in sync with the config. The order
        // is a snapshot of the config's component list; recomputing a
        // 4-element SmallVec each tick is cheap and picks up both live
        // settings edits and external config reloads.
        self.sidebar.update_from_config(&mut self.config);
        // The inline-lyrics flag tracks the `show_inline_lyrics` setting,
        // independent of sidebar state.
        self.inline_lyrics_mode = self.config.layout.show_inline_lyrics;

        // Keep ReplayGain settings in sync with the config. Cheap: the
        // setters are no-ops when the value is unchanged.
        self.logic
            .set_apply_replaygain(self.config.playback.apply_replaygain);
        self.logic
            .set_replaygain_preamp_db(self.config.playback.replaygain_preamp_db);

        let mut changed = false;

        changed |= self.logic.update();
        changed |= self.cover_art_cache.update(&self.logic);

        // Process playback events.
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            changed = true;
            if let PlaybackToLogicMessage::TrackStarted(tap) = event {
                // Scroll to the new track unless it is already visible.
                let visible = {
                    let state = self.logic.get_state();
                    let state = state.read().unwrap();
                    self.library
                        .find_flat_index_for_track(&state, &tap.track_id)
                        .is_some_and(|idx| self.library.is_index_visible(idx))
                };
                if !visible {
                    self.library.scroll_to_track = Some(tap.track_id.clone());
                }
                self.library.needs_scroll_to_playing = false;

                // Reset all lyrics view state for the new track.
                self.lyrics.reset_view();

                // Request lyrics if they will be displayed for this track:
                // the lyrics panel is open, lyrics are an enabled sidebar
                // component, or inline lyrics are on. The inline-mode flag is
                // independent of sidebar presence/visibility.
                let panel_open = self.focused_panel == FocusedPanel::Lyrics;
                let lyrics_in_sidebar = self
                    .config
                    .layout
                    .base
                    .sidebar
                    .components
                    .contains(&blackbird_client_shared::config::SidebarComponent::Lyrics)
                    && self.config.layout.base.sidebar.enabled;
                let request_lyrics = panel_open || lyrics_in_sidebar || self.inline_lyrics_mode;
                if self
                    .lyrics
                    .shared
                    .on_track_started(&tap.track_id, request_lyrics)
                {
                    self.logic.request_lyrics(&tap.track_id);
                }

                // Request similar songs for the new track if the component is
                // enabled (respecting the panel-open/visible policy: similar
                // songs are fetched if the sidebar would show them, or the
                // lyrics/side panel is open so they may be viewed).
                self.similar_songs.reset();

                // Reset the queue sidebar state for the new track; the draw function
                // will center the viewport on the current track position.
                self.queue_sidebar.reset();

                let similar_enabled = self
                    .config
                    .layout
                    .base
                    .sidebar
                    .components
                    .contains(&blackbird_client_shared::config::SidebarComponent::SimilarSongs)
                    && self.config.layout.base.sidebar.enabled;
                if similar_enabled || panel_open {
                    self.similar_songs.on_fetch_started(&tap.track_id);
                    // A new track wipes any stale similar/extension fetch error.
                    self.logic.clear_similar_errors();
                    self.logic.request_similar_songs(
                        &tap.track_id,
                        self.config.layout.base.sidebar.similar_songs_count,
                    );
                }
            }
        }

        // Process lyrics data. Like the similar-songs drain below, each delivery is
        // handled immediately so stale in-flight responses can't interleave.
        while let Ok(lyrics_data) = self.lyrics_loaded_rx.try_recv() {
            changed = true;
            self.lyrics.shared.on_lyrics_loaded(&lyrics_data);
        }

        // Process similar-songs data. Only accept results for the current
        // track, so a stale response from a previous track is ignored.
        while let Ok(similar_data) = self.similar_songs_loaded_rx.try_recv() {
            changed = true;
            if self
                .similar_songs
                .track_id
                .as_ref()
                .is_some_and(|id| id == &similar_data.track_id)
            {
                self.similar_songs.on_loaded(&similar_data);
                // A successful delivery (non-empty result, meaning the server
                // responded) clears any stale similar/extension fetch error.
                if !similar_data.similar.is_empty() {
                    self.logic.clear_similar_errors();
                }
            }
        }

        // Process library population.
        while let Ok(()) = self.library_populated_rx.try_recv() {
            changed = true;
            self.library.mark_dirty();
            if self.library.needs_scroll_to_playing
                && let Some(track_id) = self.logic.get_playing_track_id()
            {
                self.library.scroll_to_track = Some(track_id);
            }
            // Ensure selection is on a track, not a group header.
            self.library.ensure_selection_on_track(&self.logic);

            // Populate the background art prefetch queue with all album cover art IDs.
            let state = self.logic.get_state();
            let state = state.read().unwrap();
            let ids: Vec<_> = state
                .library
                .groups
                .iter()
                .filter_map(|g| g.cover_art_id.clone())
                .collect();
            drop(state);
            self.cover_art_cache.populate_prefetch_queue(ids);
        }

        // Process track updates (e.g. play count changes after scrobble).
        while let Ok(()) = self.track_updated_rx.try_recv() {
            changed = true;
            self.library.mark_dirty();
        }

        // Handle scroll-to-track.
        if let Some(track_id) = self.library.scroll_to_track.take() {
            let state = self.logic.get_state();
            let state = state.read().unwrap();
            if let Some(index) = self.library.find_flat_index_for_track(&state, &track_id) {
                self.library.selected_index = index;
                self.library.center_viewport_on_selection();
                changed = true;
            } else {
                // Track not in library yet, re-queue.
                self.library.scroll_to_track = Some(track_id);
            }
        }

        // Reload config from disk if changed (check once per second).
        // Skip while settings is open or the sidebar is being dragged —
        // in-memory changes haven't been saved yet and would be overwritten.
        if self.focused_panel != FocusedPanel::Settings
            && !self.lyrics_sidebar_dragging
            && self.sidebar_component_drag.is_none()
            && !self.settings_sidebar_dragging
            && self.last_config_check.elapsed() >= Duration::from_secs(1)
        {
            self.last_config_check = Instant::now();
            let new_config = Config::load();
            if new_config != self.config {
                self.config = new_config;
                self.config.save();
                // The runtime sidebar order and inline-mode flag are synced at
                // the top of the next tick.
                changed = true;
            }
        }

        // Apply inertia scrolling when the focused panel has an active drag.
        // A viewport is only polled for its inertia in the same branch that
        // ticks it, so a viewport left mid-inertia by a focus change can't
        // hold the event loop at the fast frame rate.
        let mut inertia_active = false;
        if self.focused_panel == FocusedPanel::Library {
            changed |= self.library.tick_inertia(&self.logic);
            inertia_active |= self.library.viewport.inertia_active();
        }
        if self.focused_panel == FocusedPanel::Search {
            changed |= self.search.tick_inertia();
            inertia_active |= self.search.viewport.inertia_active();
        }
        // The similar-songs component has its own viewport inertia, active
        // whenever a sidebar is visible (not just when focused).
        if self
            .sidebar
            .order
            .contains(&crate::ui::sidebar::SidebarComponentId::SimilarSongs)
            || self.focused_panel == FocusedPanel::Lyrics
        {
            changed |= self.similar_songs.tick_inertia();
            inertia_active |= self.similar_songs.viewport.inertia_active();
        }
        // The queue sidebar component also has viewport inertia.
        if self
            .sidebar
            .order
            .contains(&crate::ui::sidebar::SidebarComponentId::Queue)
        {
            let (before, current, after) =
                self.logic.get_queue_window(crate::ui::queue::QUEUE_RADIUS);
            let total_items = before.len() + usize::from(current.is_some()) + after.len();
            changed |= self.queue_sidebar.tick_inertia(total_items);
            inertia_active |= self.queue_sidebar.viewport.inertia_active();
        }
        if inertia_active {
            self.request_tick_animation(SMOOTH_FRAME_INTERVAL);
        }

        if self.logic.should_shutdown() {
            self.should_quit = true;
        }

        // Redraw when the scrub bar position is advancing during playback.
        if self.logic.get_playback_state() == bc::PlaybackState::Playing {
            changed = true;
        }

        if changed {
            self.needs_redraw = true;
        }
    }

    pub fn toggle_search(&mut self) {
        if self.focused_panel == FocusedPanel::Search {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Search;
        }
        self.search.reset();
    }

    pub fn toggle_lyrics(&mut self) {
        let sidebar_visible = self.config.layout.base.sidebar.enabled;
        if sidebar_visible {
            // When a sidebar is visible, the lyrics keybinding toggles focus
            // to/from the sidebar rather than opening the full panel.
            if self.focused_panel == FocusedPanel::Lyrics {
                self.focused_panel = FocusedPanel::Library;
            } else {
                // Do not reset the view — the sidebar's scroll position
                // should be preserved when focusing, as the user may be
                // reading at a scrolled position.
                self.focus_lyrics_panel(false);
            }
        } else {
            // No sidebar: the keybinding opens/closes the full panel.
            if self.focused_panel == FocusedPanel::Lyrics {
                self.focused_panel = FocusedPanel::Library;
            } else {
                self.focus_lyrics_panel(true);
            }
        }
    }

    /// Toggle the sidebar's runtime visibility (`sidebar.enabled`), saving the
    /// config. The effective sidebar position is preserved (position is always
    /// Left or Right; `enabled` controls visibility).
    pub fn toggle_sidebar(&mut self) {
        self.config.layout.base.sidebar.enabled = !self.config.layout.base.sidebar.enabled;
        // If hiding the sidebar while the sidebar has focus, return focus to
        // the library.
        if !self.config.layout.base.sidebar.enabled && self.focused_panel == FocusedPanel::Lyrics {
            self.focused_panel = FocusedPanel::Library;
        }
        self.config.save();
    }

    /// Focus the lyrics panel and request lyrics if not already loaded.
    /// When `reset` is true, resets the view state (used for the full panel).
    pub(crate) fn focus_lyrics_panel(&mut self, reset: bool) {
        self.focused_panel = FocusedPanel::Lyrics;
        if reset {
            self.lyrics.reset_view();
        }
        // Request lyrics if not already loaded for the current track.
        let playing_id = self.logic.get_playing_track_id();
        if self.lyrics.shared.on_panel_opened(playing_id.as_ref())
            && let Some(track_id) = playing_id.as_ref()
        {
            self.logic.request_lyrics(track_id);
        }
        // Request similar songs if not already loaded/loading for the current
        // track, so the panel is populated even without a visible sidebar.
        if let Some(track_id) = playing_id.as_ref()
            && self.similar_songs.track_id.as_ref() != Some(track_id)
        {
            self.similar_songs.on_fetch_started(track_id);
            self.logic.request_similar_songs(
                track_id,
                self.config.layout.base.sidebar.similar_songs_count,
            );
        }
    }

    pub fn toggle_logs(&mut self) {
        if self.focused_panel == FocusedPanel::Logs {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Logs;
            self.logs.scroll_to_end();
        }
    }

    pub fn toggle_queue(&mut self) {
        if self.focused_panel == FocusedPanel::Queue {
            self.focused_panel = FocusedPanel::Library;
        } else {
            // Block opening the queue overlay when the queue sidebar is active.
            if self.sidebar.queue_enabled() {
                return;
            }
            self.focused_panel = FocusedPanel::Queue;
            self.queue.reset();
        }
    }

    pub fn toggle_settings(&mut self) {
        if self.focused_panel == FocusedPanel::Settings {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Settings;
            self.settings.reset();
        }
    }

    pub fn cycle_playback_mode(&mut self, direction: blackbird_client_shared::Direction) {
        let next = blackbird_client_shared::cycle(
            &bc::PlaybackMode::ALL,
            self.logic.get_playback_mode(),
            direction,
        );
        self.logic.set_playback_mode(next);
    }

    pub fn save_state(&self) {
        let mut config = self.config.clone();
        config.general.volume = self.logic.get_volume();
        if let Some(tap) = self.logic.get_playing_track_and_position() {
            config.last_playback.track_id = Some(tap.track_id);
            config.last_playback.track_position_secs = tap.position.as_secs_f64();
        }
        config.last_playback.playback_mode = self.logic.get_playback_mode();
        config.last_playback.sort_order = self.logic.get_sort_order();
        config.save();
    }

    pub fn adjust_volume(&mut self, delta: f32) {
        let vol = (self.logic.get_volume() + delta).clamp(0.0, 1.0);
        self.logic.set_volume(vol);
    }

    pub fn seek_relative(&mut self, seconds: i64) {
        if let Some(details) = self.logic.get_track_display_details() {
            let current = details.track_position;
            let delta = Duration::from_secs(seconds.unsigned_abs());
            let new_pos = if seconds > 0 {
                current + delta
            } else {
                current.saturating_sub(delta)
            };
            self.logic.seek_current(new_pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ratatui::{Terminal, backend::TestBackend};

    use super::{MIN_FRAME_INTERVAL, SMOOTH_FRAME_INTERVAL};
    use crate::ui::layout::tests::test_app;

    /// Sets whether the library reports itself as fully loaded, which is what
    /// selects the animated loading screen over the track list.
    fn set_loaded(app: &mut crate::app::App, loaded: bool) {
        app.logic
            .get_state()
            .write()
            .unwrap()
            .library
            .has_loaded_all_tracks = loaded;
    }

    /// Renders one frame the way the event loop does, and reports whether the
    /// frame asked to be animated again.
    fn render_frame(app: &mut crate::app::App) -> Option<Duration> {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        app.begin_render();
        terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
        app.animations.from_render
    }

    #[test]
    fn frame_interval_is_the_tick_rate_when_nothing_animates() {
        let mut app = test_app();
        app.config.general.tick_rate_ms = 100;
        assert_eq!(app.frame_interval(), Duration::from_millis(100));
    }

    #[test]
    fn frame_interval_takes_the_shortest_request() {
        let mut app = test_app();
        app.config.general.tick_rate_ms = 100;

        app.request_tick_animation(SMOOTH_FRAME_INTERVAL);
        app.request_render_animation(Duration::from_millis(50));
        assert_eq!(app.frame_interval(), SMOOTH_FRAME_INTERVAL);
    }

    #[test]
    fn a_request_slower_than_the_tick_rate_does_not_slow_the_loop() {
        let mut app = test_app();
        app.config.general.tick_rate_ms = 100;

        app.request_render_animation(Duration::from_secs(1));
        assert_eq!(app.frame_interval(), Duration::from_millis(100));
    }

    #[test]
    fn frame_interval_is_floored() {
        let mut app = test_app();
        app.config.general.tick_rate_ms = 0;
        assert_eq!(app.frame_interval(), MIN_FRAME_INTERVAL);
    }

    #[test]
    fn each_producer_only_clears_its_own_request() {
        let mut app = test_app();
        app.request_tick_animation(SMOOTH_FRAME_INTERVAL);

        // A render leaves the tick-side request standing.
        app.begin_render();
        assert_eq!(app.animations.interval(), Some(SMOOTH_FRAME_INTERVAL));

        // A tick with nothing animating drops it. Reset the config-reload
        // timer first: `tick` would otherwise be free to reload and re-save
        // the developer's real config file from a unit test.
        app.last_config_check = Instant::now();
        app.tick();
        assert_eq!(app.animations.interval(), None);
    }

    #[test]
    fn the_loading_screen_sustains_its_own_animation() {
        let mut app = test_app();

        // The test `Logic` has an empty base URL, so its initial fetch fails
        // and the library area would render the connection error instead of
        // the loading screen. That failure is one-shot, so waiting for it and
        // clearing it leaves the error slot empty for the rest of the test.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.logic.get_error().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        app.logic.clear_error();

        set_loaded(&mut app, false);

        // Drawing the loading screen requests the next frame, which is what
        // keeps it animating while the library loads and nothing else changes.
        assert_eq!(
            render_frame(&mut app),
            Some(crate::ui::loading::FRAME_INTERVAL)
        );
        assert!(app.needs_redraw);

        // Once the library has loaded, the request stops and the UI settles.
        set_loaded(&mut app, true);
        assert_eq!(render_frame(&mut app), None);
        assert!(!app.needs_redraw);
    }
}
