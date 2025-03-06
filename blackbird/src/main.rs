use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use blackbird_subsonic as bs;

mod style;
mod util;

mod config;
use config::*;

mod album;
use album::*;
use song::{Song, SongId, SongMap};

mod song;

fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "blackbird",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
    .unwrap();
}

struct App {
    config: Config,
    last_config_update: std::time::Instant,
    logic: AppLogic,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        config.save();

        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.style_mut(|style| {
            style.visuals.panel_fill = config.style.background();
            style.visuals.override_text_color = Some(config.style.text());
        });

        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

        cc.egui_ctx.set_fonts(fonts);

        egui_extras::install_image_loaders(&cc.egui_ctx);

        let logic = AppLogic::new(
            bs::Client::new(
                config.general.base_url.clone(),
                config.general.username.clone(),
                config.general.password.clone(),
                "blackbird".to_string(),
            ),
            cc.egui_ctx.clone(),
        );

        App {
            config,
            last_config_update: std::time::Instant::now(),
            logic,
        }
    }

    fn poll_for_config_updates(&mut self) {
        if self.last_config_update.elapsed() > std::time::Duration::from_secs(1) {
            let new_config = Config::load();
            if new_config != self.config {
                self.config = new_config;
                self.config.save();
            }
            self.last_config_update = std::time::Instant::now();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_for_config_updates();

        if let Some(error) = self.logic.get_error() {
            let mut open = true;
            egui::Window::new("Error").open(&mut open).show(ctx, |ui| {
                ui.label(&error);
            });
            if !open {
                self.logic.clear_error();
            }
        }

        let margin = 8;
        let scroll_margin = 4;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .inner_margin(egui::Margin {
                        left: margin,
                        right: scroll_margin,
                        top: margin,
                        bottom: margin,
                    })
                    .fill(self.config.style.background()),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.style_mut().spacing.item_spacing = egui::Vec2::ZERO;
                            if let Some(pi) = self.logic.get_playing_info() {
                                ui.horizontal(|ui| {
                                    if let Some(artist) =
                                        pi.song_artist.as_ref().filter(|a| **a != pi.album_artist)
                                    {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(artist)
                                                    .color(style::string_to_colour(artist)),
                                            )
                                            .selectable(false),
                                        );
                                        ui.add(egui::Label::new(" - ").selectable(false));
                                    }
                                    ui.add(egui::Label::new(&pi.song_title).selectable(false));
                                });
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&pi.album_name)
                                                .color(self.config.style.album()),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add(egui::Label::new(" by ").selectable(false));
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&pi.album_artist)
                                                .color(style::string_to_colour(&pi.album_artist)),
                                        )
                                        .selectable(false),
                                    );
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    let percent_loaded = self.logic.get_loaded_0_to_1();
                                    ui.add(
                                        egui::Label::new(format!(
                                            "Nothing playing | {:0.1}% loaded",
                                            percent_loaded * 100.0
                                        ))
                                        .selectable(false),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::Label::new("Double-click a song to play it!")
                                            .selectable(false),
                                    );
                                });
                            }
                        });
                    });

                    if self.logic.is_song_loaded() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.style_mut().visuals.override_text_color = None;
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(egui_phosphor::regular::STOP)
                                            .size(32.0),
                                    )
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                self.logic.stop_playback();
                            }
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(egui_phosphor::regular::PLAY_PAUSE)
                                            .size(32.0),
                                    )
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                self.logic.toggle_playback();
                            }
                        });
                    }
                });

                ui.separator();

                ui.scope(|ui| {
                    // Make the scroll bar solid, and hide its background. Ideally, we'd set the opacity
                    // to 0, but egui doesn't allow that for solid scroll bars.
                    ui.style_mut().spacing.scroll = egui::style::ScrollStyle {
                        bar_inner_margin: scroll_margin.into(),
                        ..egui::style::ScrollStyle::solid()
                    };
                    ui.style_mut().visuals.extreme_bg_color = self.config.style.background();

                    let row_height = ui.text_style_height(&egui::TextStyle::Body);
                    let album_margin_bottom_row_count = 1;

                    // Get album data for rendering
                    let num_rows = self
                        .logic
                        .calculate_total_rows(album_margin_bottom_row_count);

                    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                        ui,
                        row_height,
                        num_rows,
                        |ui, visible_row_range| {
                            // Calculate which albums are in view
                            let visible_albums = self.logic.get_visible_albums(
                                visible_row_range.clone(),
                                album_margin_bottom_row_count,
                            );

                            let playing_song_id = self.logic.get_playing_song_id();

                            let mut current_row = visible_albums.start_row;

                            for album in visible_albums.albums {
                                let album_lines =
                                    album.line_count() + album_margin_bottom_row_count;

                                // If the album needs to be loaded
                                if album.songs.is_none() {
                                    self.logic.fetch_album(&album.id);
                                }

                                // Handle cover art if enabled
                                if self.config.general.album_art_enabled {
                                    if let Some(cover_art_id) = &album.cover_art_id {
                                        if !self.logic.has_cover_art(cover_art_id) {
                                            self.logic.fetch_cover_art(cover_art_id);
                                        }
                                    }
                                }

                                // Compute the visible portion of the album's rows, rebased to the album
                                let local_start =
                                    visible_row_range.start.saturating_sub(current_row);
                                let local_end = visible_row_range
                                    .end
                                    .saturating_sub(current_row)
                                    .min(album_lines - album_margin_bottom_row_count);

                                // Ensure we have a valid range (start <= end)
                                let local_visible_range = local_start..local_end.max(local_start);

                                // Get cover art if needed
                                let cover_art = if self.config.general.album_art_enabled {
                                    album
                                        .cover_art_id
                                        .as_deref()
                                        .and_then(|id| self.logic.get_cover_art(id))
                                } else {
                                    None
                                };

                                // Display the album
                                let clicked_song_id = album.ui(
                                    ui,
                                    &self.config.style,
                                    local_visible_range,
                                    cover_art,
                                    self.config.general.album_art_enabled,
                                    &self.logic.read_state().song_map,
                                    playing_song_id.as_ref(),
                                );

                                // Handle song selection
                                if let Some(song_id) = clicked_song_id {
                                    self.logic.play_song(song_id);
                                }

                                ui.add_space(row_height * album_margin_bottom_row_count as f32);
                                current_row += album_lines;
                            }
                        },
                    );
                });
            });
    }
}

struct AppState {
    albums: Vec<Album>,
    album_id_to_idx: HashMap<AlbumId, usize>,
    pending_album_request_ids: HashSet<AlbumId>,
    song_map: SongMap,

    cover_art_cache: HashMap<String, (egui::ImageSource<'static>, std::time::Instant)>,
    pending_cover_art_requests: HashSet<String>,

    playing_song: Option<SongId>,
    error: Option<String>,
}
enum LogicThreadMessage {
    PlaySong(Vec<u8>),
    TogglePlayback,
    StopPlayback,
}
struct VisibleAlbumSet {
    albums: Vec<Album>,
    start_row: usize,
}

#[derive(Clone)]
struct TokioHandle(tokio::sync::mpsc::Sender<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>);
impl TokioHandle {
    fn spawn(&self, task: impl Future<Output = ()> + Send + Sync + 'static) {
        self.0.blocking_send(Box::pin(task)).unwrap();
    }
}

struct AppLogic {
    tokio: TokioHandle,
    _tokio_thread_handle: std::thread::JoinHandle<()>,
    state: Arc<RwLock<AppState>>,
    client: Arc<bs::Client>,
    ctx: egui::Context,
    logic_thread_tx: std::sync::mpsc::Sender<LogicThreadMessage>,
    _logic_thread_handle: std::thread::JoinHandle<()>,
}

impl AppLogic {
    const MAX_CONCURRENT_ALBUM_REQUESTS: usize = 100;
    const MAX_CONCURRENT_COVER_ART_REQUESTS: usize = 10;
    const MAX_COVER_ART_CACHE_SIZE: usize = 32;

    fn new(client: bs::Client, ctx: egui::Context) -> Self {
        // Create the logic thread for audio playback
        let (logic_tx, logic_rx) = std::sync::mpsc::channel();

        // Initialize audio output in the logic thread
        let logic_thread_handle = std::thread::spawn(move || {
            let (_output_stream, output_stream_handle) =
                rodio::OutputStream::try_default().unwrap();
            let sink = rodio::Sink::try_new(&output_stream_handle).unwrap();
            sink.set_volume(1.0);

            let mut last_data = None;
            while let Ok(msg) = logic_rx.recv() {
                match msg {
                    LogicThreadMessage::PlaySong(data) => {
                        sink.clear();
                        last_data = Some(data.clone());
                        sink.append(
                            rodio::Decoder::new_looped(std::io::Cursor::new(data)).unwrap(),
                        );
                        sink.play();
                    }
                    LogicThreadMessage::TogglePlayback => {
                        if !sink.is_paused() {
                            sink.pause();
                            continue;
                        }
                        if sink.empty() {
                            if let Some(data) = last_data.clone() {
                                sink.append(
                                    rodio::Decoder::new_looped(std::io::Cursor::new(data)).unwrap(),
                                );
                            }
                        }
                        sink.play();
                    }
                    LogicThreadMessage::StopPlayback => {
                        sink.clear();
                    }
                }
            }
        });

        let state = Arc::new(RwLock::new(AppState {
            albums: vec![],
            album_id_to_idx: HashMap::new(),
            pending_album_request_ids: HashSet::new(),
            song_map: HashMap::new(),
            cover_art_cache: HashMap::new(),
            pending_cover_art_requests: HashSet::new(),
            playing_song: None,
            error: None,
        }));

        let client = Arc::new(client);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel(100);

        // Create a thread for background processing
        let tokio_thread_handle = std::thread::spawn(move || {
            runtime.block_on(async {
                while let Some(task) = tokio_rx.recv().await {
                    tokio::spawn(task);
                }
            });
        });

        let logic = AppLogic {
            tokio: TokioHandle(tokio_tx),
            _tokio_thread_handle: tokio_thread_handle,
            state,
            client,
            ctx,
            logic_thread_tx: logic_tx,
            _logic_thread_handle: logic_thread_handle,
        };
        logic.initial_fetch();
        logic
    }

    fn spawn(&self, task: impl Future<Output = ()> + Send + Sync + 'static) {
        self.tokio.spawn(task);
    }

    fn initial_fetch(&self) {
        let client = self.client.clone();
        let state = self.state.clone();
        let ctx = self.ctx.clone();
        self.spawn(async move {
            if let Err(e) = client.ping().await {
                state.write().unwrap().error = Some(e.to_string());
                return;
            }

            match Album::fetch_all(&client).await {
                Ok(albums) => {
                    let mut state = state.write().unwrap();
                    state.albums = albums;
                    state.albums.sort();
                    state.album_id_to_idx = state
                        .albums
                        .iter()
                        .enumerate()
                        .map(|(i, a)| (a.id.clone(), i))
                        .collect();
                }
                Err(e) => {
                    state.write().unwrap().error = Some(e.to_string());
                    ctx.request_repaint();
                }
            };
        })
    }

    fn calculate_total_rows(&self, album_margin_bottom_row_count: usize) -> usize {
        self.read_state()
            .albums
            .iter()
            .map(|album| album.line_count() + album_margin_bottom_row_count)
            .sum()
    }

    fn get_visible_albums(
        &self,
        visible_row_range: std::ops::Range<usize>,
        album_margin_bottom_row_count: usize,
    ) -> VisibleAlbumSet {
        let state = self.read_state();
        let mut current_row = 0;
        let mut visible_albums = Vec::new();
        // We'll set this to the actual start row of first visible album
        let mut start_row = 0;
        let mut first_visible_found = false;

        for album in &state.albums {
            let album_lines = album.line_count() + album_margin_bottom_row_count;
            let album_range = current_row..(current_row + album_lines);

            // If this album starts after the visible range, we can break out
            if album_range.start >= visible_row_range.end {
                break;
            }

            // If this album is completely above the visible range, skip it
            if album_range.end <= visible_row_range.start {
                current_row += album_lines;
                continue;
            }

            // Found first visible album - record its starting row
            if !first_visible_found {
                start_row = current_row;
                first_visible_found = true;
            }

            visible_albums.push(album.clone());
            current_row += album_lines;
        }

        VisibleAlbumSet {
            albums: visible_albums,
            start_row,
        }
    }

    fn fetch_album(&self, album_id: &AlbumId) {
        {
            let mut state = self.write_state();
            if state.pending_album_request_ids.len() >= Self::MAX_CONCURRENT_ALBUM_REQUESTS {
                return;
            }
            if state.pending_album_request_ids.contains(album_id) {
                return;
            }
            state.pending_album_request_ids.insert(album_id.clone());
        }

        self.spawn({
            let client = self.client.clone();
            let state = self.state.clone();
            let album_id = album_id.clone();
            let ctx = self.ctx.clone();
            async move {
                match client.get_album_with_songs(album_id.0.clone()).await {
                    Ok(incoming_album) => {
                        let mut state = state.write().unwrap();
                        let album_idx = state.album_id_to_idx[&album_id];
                        let album = &mut state.albums[album_idx];
                        album.songs = Some(
                            incoming_album
                                .song
                                .iter()
                                .map(|s| SongId(s.id.clone()))
                                .collect(),
                        );
                        state
                            .song_map
                            .extend(incoming_album.song.into_iter().map(|s| {
                                let s: Song = s.into();
                                (s.id.clone(), s)
                            }));
                        state.pending_album_request_ids.remove(&album_id);
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        let mut state = state.write().unwrap();
                        state.error = Some(e.to_string());
                        state.pending_album_request_ids.remove(&album_id);
                        ctx.request_repaint();
                    }
                }
            }
        });
    }

    fn fetch_cover_art(&self, cover_art_id: &str) {
        {
            let mut state = self.write_state();
            if state.pending_cover_art_requests.len() >= Self::MAX_CONCURRENT_COVER_ART_REQUESTS {
                return;
            }
            if state.pending_cover_art_requests.contains(cover_art_id) {
                return;
            }
            state
                .pending_cover_art_requests
                .insert(cover_art_id.to_string());
        }

        self.spawn({
            let client = self.client.clone();
            let state = self.state.clone();
            let cover_art_id = cover_art_id.to_string();
            let ctx = self.ctx.clone();
            async move {
                match client.get_cover_art(&cover_art_id).await {
                    Ok(cover_art) => {
                        let mut state = state.write().unwrap();
                        if state.cover_art_cache.len() == Self::MAX_COVER_ART_CACHE_SIZE {
                            let oldest_id = state
                                .cover_art_cache
                                .iter()
                                .min_by_key(|(_, (_, time))| *time)
                                .map(|(id, _)| id.clone())
                                .expect("cover art cache not empty");
                            state.cover_art_cache.remove(&oldest_id);
                        }

                        let uri = format!("bytes://{cover_art_id}");
                        state.pending_cover_art_requests.remove(&cover_art_id);
                        state.cover_art_cache.insert(
                            cover_art_id,
                            (
                                egui::ImageSource::Bytes {
                                    uri: uri.into(),
                                    bytes: cover_art.into(),
                                },
                                std::time::Instant::now(),
                            ),
                        );
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        let mut state = state.write().unwrap();
                        state.error = Some(e.to_string());
                        state.pending_cover_art_requests.remove(&cover_art_id);
                        ctx.request_repaint();
                    }
                }
            }
        });
    }

    fn play_song(&self, song_id: &SongId) {
        let client = self.client.clone();
        let state = self.state.clone();
        let song_id = song_id.clone();
        let ctx = self.ctx.clone();
        let logic_tx = self.logic_thread_tx.clone();

        self.spawn(async move {
            match client.download(&song_id.0).await {
                Ok(data) => {
                    // Update the state to reflect the new playing song
                    {
                        let mut state = state.write().unwrap();
                        state.playing_song = Some(song_id.clone());
                    }

                    // Send the data to the logic thread to play
                    logic_tx.send(LogicThreadMessage::PlaySong(data)).unwrap();
                    ctx.request_repaint();
                }
                Err(e) => {
                    let mut state = state.write().unwrap();
                    state.error = Some(e.to_string());
                    ctx.request_repaint();
                }
            }
        });
    }

    fn toggle_playback(&self) {
        self.logic_thread_tx
            .send(LogicThreadMessage::TogglePlayback)
            .unwrap();
    }

    fn stop_playback(&self) {
        self.logic_thread_tx
            .send(LogicThreadMessage::StopPlayback)
            .unwrap();
    }

    fn get_cover_art(&self, id: &str) -> Option<egui::ImageSource<'static>> {
        self.read_state()
            .cover_art_cache
            .get(id)
            .map(|(img, _)| img.clone())
    }

    fn has_cover_art(&self, id: &str) -> bool {
        let state = self.read_state();
        state.cover_art_cache.contains_key(id) || state.pending_cover_art_requests.contains(id)
    }

    fn get_playing_song_id(&self) -> Option<SongId> {
        self.read_state().playing_song.clone()
    }

    fn is_song_loaded(&self) -> bool {
        self.read_state().playing_song.is_some()
    }

    fn get_playing_info(&self) -> Option<PlayingInfo> {
        let state = self.read_state();
        let song = state.song_map.get(state.playing_song.as_ref()?)?;
        let album = state.albums.get(
            state
                .album_id_to_idx
                .get(song.album_id.as_ref()?)
                .copied()?,
        )?;
        Some(PlayingInfo {
            album_name: album.name.clone(),
            album_artist: album.artist.clone(),
            song_title: song.title.clone(),
            song_artist: song.artist.clone(),
        })
    }

    fn get_loaded_0_to_1(&self) -> f32 {
        let state = self.read_state();
        let total_albums = state.albums.len();
        let loaded_albums = state.albums.iter().filter(|a| a.songs.is_some()).count();
        loaded_albums as f32 / total_albums as f32
    }

    fn get_error(&self) -> Option<String> {
        self.read_state().error.clone()
    }

    fn clear_error(&self) {
        self.write_state().error = None;
    }

    fn write_state(&self) -> RwLockWriteGuard<AppState> {
        self.state.write().unwrap()
    }

    fn read_state(&self) -> RwLockReadGuard<AppState> {
        self.state.read().unwrap()
    }
}

struct PlayingInfo {
    album_name: String,
    album_artist: String,
    song_title: String,
    song_artist: Option<String>,
}
