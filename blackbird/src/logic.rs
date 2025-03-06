use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use crate::{
    album::{Album, AlbumId},
    bs,
    song::{Song, SongId, SongMap},
};

pub struct Logic {
    tokio: TokioHandle,
    _tokio_thread_handle: std::thread::JoinHandle<()>,
    state: Arc<RwLock<AppState>>,
    song_map: Arc<RwLock<SongMap>>,
    client: Arc<bs::Client>,
    ctx: egui::Context,
    logic_thread_tx: std::sync::mpsc::Sender<LogicThreadMessage>,
    _logic_thread_handle: std::thread::JoinHandle<()>,
}

pub struct PlayingInfo {
    pub album_name: String,
    pub album_artist: String,
    pub song_title: String,
    pub song_artist: Option<String>,
}

pub struct VisibleAlbumSet {
    pub albums: Vec<Arc<Album>>,
    pub start_row: usize,
}

impl Logic {
    const MAX_CONCURRENT_ALBUM_REQUESTS: usize = 100;
    const MAX_CONCURRENT_COVER_ART_REQUESTS: usize = 10;
    const MAX_COVER_ART_CACHE_SIZE: usize = 32;

    pub fn new(client: bs::Client, ctx: egui::Context) -> Self {
        // Create the logic thread for audio playback
        let (logic_tx, logic_rx) = std::sync::mpsc::channel();

        let state = Arc::new(RwLock::new(AppState {
            albums: vec![],
            album_id_to_idx: HashMap::new(),
            pending_album_request_ids: HashSet::new(),
            cover_art_cache: HashMap::new(),
            pending_cover_art_requests: HashSet::new(),
            playing_song: None,
            error: None,
        }));
        let song_map = Arc::new(RwLock::new(SongMap::new()));

        let client = Arc::new(client);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel(100);
        let tokio = TokioHandle(tokio_tx);

        // Create a thread for background processing
        let tokio_thread_handle = std::thread::spawn(move || {
            runtime.block_on(async {
                while let Some(task) = tokio_rx.recv().await {
                    tokio::spawn(task);
                }
            });
        });

        // Initialize audio output in the logic thread
        let logic_thread_handle = std::thread::spawn({
            let state = state.clone();
            let song_map = song_map.clone();
            let client = client.clone();
            let ctx = ctx.clone();
            let tokio = tokio.clone();
            move || {
                let (_output_stream, output_stream_handle) =
                    rodio::OutputStream::try_default().unwrap();
                let sink = rodio::Sink::try_new(&output_stream_handle).unwrap();
                sink.set_volume(1.0);

                let mut last_data = None;
                loop {
                    // Process all available messages without blocking
                    while let Ok(msg) = logic_rx.try_recv() {
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
                                            rodio::Decoder::new_looped(std::io::Cursor::new(data))
                                                .unwrap(),
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

                    // Fetch unloaded albums up to MAX_CONCURRENT_ALBUM_REQUESTS
                    {
                        let pending_count = state.read().unwrap().pending_album_request_ids.len();

                        if pending_count < Self::MAX_CONCURRENT_ALBUM_REQUESTS {
                            // Find albums that need to be loaded
                            let unloaded_albums: Vec<AlbumId> = {
                                let state_read = state.read().unwrap();
                                state_read
                                    .albums
                                    .iter()
                                    .filter(|album| album.songs.is_none())
                                    .filter(|album| {
                                        !state_read.pending_album_request_ids.contains(&album.id)
                                    })
                                    .take(Self::MAX_CONCURRENT_ALBUM_REQUESTS - pending_count)
                                    .map(|album| album.id.clone())
                                    .collect()
                            };

                            // Fetch each album
                            for album_id in unloaded_albums {
                                Self::fetch_album_impl(
                                    &tokio, &client, &state, &song_map, &ctx, &album_id,
                                );
                            }
                        }
                    }

                    // Sleep for 10ms between iterations
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        });

        let logic = Logic {
            tokio,
            _tokio_thread_handle: tokio_thread_handle,
            state,
            song_map,
            client,
            ctx,
            logic_thread_tx: logic_tx,
            _logic_thread_handle: logic_thread_handle,
        };
        logic.initial_fetch();
        logic
    }

    pub fn calculate_total_rows(
        &self,
        album_margin_bottom_row_count: usize,
        album_line_count_getter: impl Fn(&Album) -> usize,
    ) -> usize {
        self.read_state()
            .albums
            .iter()
            .map(|album| album_line_count_getter(album) + album_margin_bottom_row_count)
            .sum()
    }

    pub fn get_visible_albums(
        &self,
        visible_row_range: std::ops::Range<usize>,
        album_margin_bottom_row_count: usize,
        album_line_count_getter: impl Fn(&Album) -> usize,
    ) -> VisibleAlbumSet {
        let state = self.read_state();
        let mut current_row = 0;
        let mut visible_albums = Vec::new();
        // We'll set this to the actual start row of first visible album
        let mut start_row = 0;
        let mut first_visible_found = false;

        for album in &state.albums {
            let album_lines = album_line_count_getter(album) + album_margin_bottom_row_count;
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

    pub fn fetch_album(&self, album_id: &AlbumId) {
        Self::fetch_album_impl(
            &self.tokio,
            &self.client,
            &self.state,
            &self.song_map,
            &self.ctx,
            album_id,
        );
    }

    pub fn fetch_cover_art(&self, cover_art_id: &str) {
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

    pub fn play_song(&self, song_id: &SongId) {
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

    pub fn toggle_playback(&self) {
        self.logic_thread_tx
            .send(LogicThreadMessage::TogglePlayback)
            .unwrap();
    }

    pub fn stop_playback(&self) {
        self.logic_thread_tx
            .send(LogicThreadMessage::StopPlayback)
            .unwrap();
    }

    pub fn get_cover_art(&self, id: &str) -> Option<egui::ImageSource<'static>> {
        self.read_state()
            .cover_art_cache
            .get(id)
            .map(|(img, _)| img.clone())
    }

    pub fn has_cover_art(&self, id: &str) -> bool {
        let state = self.read_state();
        state.cover_art_cache.contains_key(id) || state.pending_cover_art_requests.contains(id)
    }

    pub fn get_playing_song_id(&self) -> Option<SongId> {
        self.read_state().playing_song.clone()
    }

    pub fn is_song_loaded(&self) -> bool {
        self.read_state().playing_song.is_some()
    }

    pub fn get_playing_info(&self) -> Option<PlayingInfo> {
        let state = self.read_state();
        let song_map = self.song_map.read().unwrap();
        let song = song_map.get(state.playing_song.as_ref()?)?;
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

    pub fn get_loaded_0_to_1(&self) -> f32 {
        let state = self.read_state();
        let total_albums = state.albums.len();
        let loaded_albums = state.albums.iter().filter(|a| a.songs.is_some()).count();
        loaded_albums as f32 / total_albums as f32
    }

    pub fn get_error(&self) -> Option<String> {
        self.read_state().error.clone()
    }

    pub fn get_song_map(&self) -> RwLockReadGuard<SongMap> {
        self.song_map.read().unwrap()
    }

    pub fn clear_error(&self) {
        self.write_state().error = None;
    }
}
impl Logic {
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

    fn write_state(&self) -> RwLockWriteGuard<AppState> {
        self.state.write().unwrap()
    }

    fn read_state(&self) -> RwLockReadGuard<AppState> {
        self.state.read().unwrap()
    }

    // Shared implementation for fetching an album
    fn fetch_album_impl(
        tokio: &TokioHandle,
        client: &Arc<bs::Client>,
        state: &Arc<RwLock<AppState>>,
        song_map: &Arc<RwLock<SongMap>>,
        ctx: &egui::Context,
        album_id: &AlbumId,
    ) {
        // Mark album as pending
        {
            let mut state = state.write().unwrap();
            if state.pending_album_request_ids.len() >= Self::MAX_CONCURRENT_ALBUM_REQUESTS {
                return;
            }
            if state.pending_album_request_ids.contains(album_id) {
                return;
            }
            state.pending_album_request_ids.insert(album_id.clone());
        }

        // Clone what we need for the async task
        let client = client.clone();
        let state = state.clone();
        let album_id = album_id.clone();
        let ctx = ctx.clone();
        let song_map = song_map.clone();

        // Create the async task
        let task = async move {
            match client.get_album_with_songs(album_id.0.clone()).await {
                Ok(incoming_album) => {
                    let mut state = state.write().unwrap();
                    let album_idx = state.album_id_to_idx[&album_id];
                    // Replace the Arc in the array
                    state.albums[album_idx] = Arc::new(Album {
                        songs: Some(
                            incoming_album
                                .song
                                .iter()
                                .map(|s| SongId(s.id.clone()))
                                .collect(),
                        ),
                        ..(*state.albums[album_idx]).clone()
                    });
                    {
                        let mut song_map = song_map.write().unwrap();
                        song_map.extend(incoming_album.song.into_iter().map(|s| {
                            let s: Song = s.into();
                            (s.id.clone(), s)
                        }));
                    }
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
        };

        // Get a TokioHandle to spawn the task
        tokio.spawn(task);
    }
}

struct AppState {
    albums: Vec<Arc<Album>>,
    album_id_to_idx: HashMap<AlbumId, usize>,
    pending_album_request_ids: HashSet<AlbumId>,

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

#[derive(Clone)]
struct TokioHandle(tokio::sync::mpsc::Sender<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>);
impl TokioHandle {
    fn spawn(&self, task: impl Future<Output = ()> + Send + Sync + 'static) {
        self.0.blocking_send(Box::pin(task)).unwrap();
    }
}
