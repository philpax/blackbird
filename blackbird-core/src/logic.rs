use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};

use crate::{
    bs,
    state::{self, Album, AlbumId, Group, SongId, SongMap},
};

pub struct Logic {
    tokio: TokioHandle,
    _tokio_thread_handle: std::thread::JoinHandle<()>,
    state: Arc<RwLock<AppState>>,
    song_map: Arc<RwLock<SongMap>>,
    client: Arc<bs::Client>,

    logic_to_playback_tx: std::sync::mpsc::Sender<LogicToPlaybackMessage>,
    _playback_thread_handle: std::thread::JoinHandle<()>,
    playback_to_logic_rx: tokio::sync::broadcast::Receiver<PlaybackToLogicMessage>,

    transcode: bool,
}

#[derive(Debug, Clone)]
pub enum PlaybackToLogicMessage {
    TrackStarted(PlayingInfo),
    PlaybackStateChanged(PlaybackState),
    PositionChanged(Duration),
}

#[derive(Debug, Clone)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct PlayingInfo {
    pub album_name: String,
    pub album_artist: String,
    pub song_title: String,
    pub song_artist: Option<String>,
    pub song_duration: Duration,
    pub song_position: Duration,
}

pub struct VisibleGroupSet {
    pub groups: Vec<Arc<Group>>,
    pub start_row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    Sequential,
    Shuffle,
    RepeatOne,
}

impl Logic {
    const MAX_CONCURRENT_COVER_ART_REQUESTS: usize = 10;
    const MAX_COVER_ART_CACHE_SIZE: usize = 32;

    pub fn new(base_url: String, username: String, password: String, transcode: bool) -> Self {
        let state = Arc::new(RwLock::new(AppState {
            songs: vec![],
            groups: vec![],
            albums: HashMap::new(),
            cover_art_cache: HashMap::new(),
            pending_cover_art_requests: HashSet::new(),
            has_loaded_all_songs: false,
            error: None,
            playing_song: None,
            next_song: None,
            playback_mode: PlaybackMode::Sequential,
        }));
        let song_map = Arc::new(RwLock::new(SongMap::new()));

        let client = Arc::new(bs::Client::new(
            base_url,
            username,
            password,
            "blackbird".to_string(),
        ));

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

        let (playback_to_logic_tx, playback_to_logic_rx) =
            tokio::sync::broadcast::channel::<PlaybackToLogicMessage>(100);

        // Initialize audio output in the playback thread
        let (logic_to_playback_tx, logic_to_playback_rx) = std::sync::mpsc::channel();
        let playback_thread_handle = std::thread::spawn({
            let playback_to_logic_tx = playback_to_logic_tx.clone();
            let state = state.clone();
            move || {
                let stream_handle = rodio::OutputStreamBuilder::open_default_stream().unwrap();
                let sink = rodio::Sink::connect_new(stream_handle.mixer());
                sink.set_volume(1.0);

                fn build_decoder(
                    data: Vec<u8>,
                ) -> rodio::decoder::Decoder<std::io::Cursor<Vec<u8>>> {
                    rodio::decoder::DecoderBuilder::new()
                        .with_byte_len(data.len() as u64)
                        .with_data(std::io::Cursor::new(data))
                        .build()
                        .unwrap()
                }

                const SEEK_DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

                let mut last_data = None;
                let mut last_seek_time = std::time::Instant::now();
                let mut last_position_update = std::time::Instant::now();

                loop {
                    // Process all available messages without blocking
                    while let Ok(msg) = logic_to_playback_rx.try_recv() {
                        match msg {
                            LogicToPlaybackMessage::PlaySong(data, playing_info) => {
                                sink.clear();
                                last_data = Some(data.clone());
                                sink.append(build_decoder(data));
                                sink.play();
                                let _ = playback_to_logic_tx
                                    .send(PlaybackToLogicMessage::TrackStarted(playing_info));
                                let _ = playback_to_logic_tx.send(
                                    PlaybackToLogicMessage::PlaybackStateChanged(
                                        PlaybackState::Playing,
                                    ),
                                );
                            }
                            LogicToPlaybackMessage::TogglePlayback => {
                                if !sink.is_paused() {
                                    sink.pause();
                                    let _ = playback_to_logic_tx.send(
                                        PlaybackToLogicMessage::PlaybackStateChanged(
                                            PlaybackState::Paused,
                                        ),
                                    );
                                    continue;
                                }
                                if sink.empty()
                                    && let Some(data) = last_data.clone()
                                {
                                    sink.append(build_decoder(data));
                                }
                                sink.play();
                                let _ = playback_to_logic_tx.send(
                                    PlaybackToLogicMessage::PlaybackStateChanged(
                                        PlaybackState::Playing,
                                    ),
                                );
                            }
                            LogicToPlaybackMessage::Play => {
                                if sink.empty()
                                    && let Some(data) = last_data.clone()
                                {
                                    sink.append(build_decoder(data));
                                }
                                sink.play();
                                let _ = playback_to_logic_tx.send(
                                    PlaybackToLogicMessage::PlaybackStateChanged(
                                        PlaybackState::Playing,
                                    ),
                                );
                            }
                            LogicToPlaybackMessage::Pause => {
                                sink.pause();
                                let _ = playback_to_logic_tx.send(
                                    PlaybackToLogicMessage::PlaybackStateChanged(
                                        PlaybackState::Paused,
                                    ),
                                );
                            }
                            LogicToPlaybackMessage::StopPlayback => {
                                sink.clear();
                                let _ = playback_to_logic_tx.send(
                                    PlaybackToLogicMessage::PlaybackStateChanged(
                                        PlaybackState::Stopped,
                                    ),
                                );
                            }
                            LogicToPlaybackMessage::Seek(position) => {
                                let now = std::time::Instant::now();
                                if now.duration_since(last_seek_time) >= SEEK_DEBOUNCE_DURATION {
                                    last_seek_time = now;
                                    if let Err(e) = sink.try_seek(position) {
                                        tracing::warn!(
                                            "Failed to seek to position {position:?}: {e}"
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // Check if we should auto-advance to next track
                    if sink.empty()
                        && let Some(data) = last_data.clone()
                    {
                        sink.append(build_decoder(data));
                    }

                    // Update playing position in queue
                    let current_position = sink.get_pos();
                    if let Some(playing_song) = &mut state.write().unwrap().playing_song {
                        playing_song.position = current_position;
                    }

                    // Send position updates every second
                    let now = std::time::Instant::now();
                    if now.duration_since(last_position_update) >= Duration::from_secs(1) {
                        last_position_update = now;
                        if !sink.empty() && !sink.is_paused() {
                            let _ = playback_to_logic_tx
                                .send(PlaybackToLogicMessage::PositionChanged(current_position));
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
            logic_to_playback_tx,
            _playback_thread_handle: playback_thread_handle,
            playback_to_logic_rx,
            transcode,
        };
        logic.initial_fetch();
        logic
    }

    pub fn calculate_total_rows(
        &self,
        group_margin_bottom_row_count: usize,
        group_line_count_getter: impl Fn(&Group) -> usize,
    ) -> usize {
        self.read_state()
            .groups
            .iter()
            .map(|group| group_line_count_getter(group) + group_margin_bottom_row_count)
            .sum()
    }

    pub fn get_visible_groups(
        &self,
        visible_row_range: std::ops::Range<usize>,
        group_margin_bottom_row_count: usize,
        group_line_count_getter: impl Fn(&Group) -> usize,
    ) -> VisibleGroupSet {
        let state = self.read_state();
        let mut current_row = 0;
        let visible_groups = Vec::new();

        // Add buffer albums before and after visible range
        const BUFFER_ALBUMS: usize = 3;

        // First pass: find albums that intersect with visible range
        let mut intersecting_album_indices = Vec::new();
        for (album_index, group) in state.groups.iter().enumerate() {
            let group_lines = group_line_count_getter(group) + group_margin_bottom_row_count;
            let group_range = current_row..(current_row + group_lines);

            // Check if this album intersects with visible range
            if group_range.start < visible_row_range.end
                && group_range.end > visible_row_range.start
            {
                intersecting_album_indices.push(album_index);
            }

            current_row += group_lines;
        }

        if intersecting_album_indices.is_empty() {
            return VisibleGroupSet {
                groups: visible_groups,
                start_row: 0,
            };
        }

        // Determine the range of albums to include with buffer
        let first_intersecting = intersecting_album_indices[0];
        let last_intersecting = intersecting_album_indices[intersecting_album_indices.len() - 1];

        let start_album_index = first_intersecting.saturating_sub(BUFFER_ALBUMS);
        let end_album_index = (last_intersecting + BUFFER_ALBUMS + 1).min(state.groups.len());

        // Calculate start_row for the first album we'll include
        current_row = 0;
        for i in 0..start_album_index {
            let group = &state.groups[i];
            let group_lines = group_line_count_getter(group) + group_margin_bottom_row_count;
            current_row += group_lines;
        }
        let start_row = current_row;

        // Include the selected range of albums
        let mut visible_groups = Vec::new();
        for i in start_album_index..end_album_index {
            visible_groups.push(state.groups[i].clone());
        }

        VisibleGroupSet {
            groups: visible_groups,
            start_row,
        }
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

                        state.pending_cover_art_requests.remove(&cover_art_id);
                        state
                            .cover_art_cache
                            .insert(cover_art_id, (cover_art, std::time::Instant::now()));
                    }
                    Err(e) => {
                        let mut state = state.write().unwrap();
                        state.error = Some(e.to_string());
                        state.pending_cover_art_requests.remove(&cover_art_id);
                    }
                }
            }
        });
    }

    pub fn toggle_playback(&self) {
        self.logic_to_playback_tx
            .send(LogicToPlaybackMessage::TogglePlayback)
            .unwrap();
    }

    pub fn play(&self) {
        self.logic_to_playback_tx
            .send(LogicToPlaybackMessage::Play)
            .unwrap();
    }

    pub fn pause(&self) {
        self.logic_to_playback_tx
            .send(LogicToPlaybackMessage::Pause)
            .unwrap();
    }

    pub fn stop_playback(&self) {
        self.logic_to_playback_tx
            .send(LogicToPlaybackMessage::StopPlayback)
            .unwrap();
    }

    pub fn seek(&self, position: Duration) {
        self.logic_to_playback_tx
            .send(LogicToPlaybackMessage::Seek(position))
            .unwrap();
    }

    pub fn subscribe_to_track_changes(
        &self,
    ) -> tokio::sync::broadcast::Receiver<PlaybackToLogicMessage> {
        self.playback_to_logic_rx.resubscribe()
    }
}

fn create_playing_info_for_song(
    song_id: &SongId,
    song_map: &Arc<RwLock<SongMap>>,
    state: &Arc<RwLock<AppState>>,
) -> Option<PlayingInfo> {
    let song_map = song_map.read().unwrap();
    let song = song_map.get(song_id)?;
    let state = state.read().unwrap();
    let album = state.albums.get(song.album_id.as_ref()?)?;
    Some(PlayingInfo {
        album_name: album.name.clone(),
        album_artist: album.artist.clone(),
        song_title: song.title.clone(),
        song_artist: song.artist.clone(),
        song_duration: Duration::from_secs(song.duration.unwrap_or(1) as u64),
        song_position: Duration::from_secs(0),
    })
}

impl Logic {
    pub fn get_cover_art(&self, id: &str) -> Option<Vec<u8>> {
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
        self.read_state()
            .playing_song
            .as_ref()
            .map(|playing_song| playing_song.song_id.clone())
    }

    pub fn is_song_loaded(&self) -> bool {
        self.read_state().playing_song.is_some()
    }

    pub fn is_song_loading(&self) -> bool {
        self.read_state().next_song.is_some()
    }

    pub fn get_playing_info(&self) -> Option<PlayingInfo> {
        let song_map = self.song_map.read().unwrap();
        let state = self.read_state();

        let playing_song = state.playing_song.as_ref()?;
        let song = song_map.get(&playing_song.song_id)?;
        let song_position = playing_song.position;
        let album = state.albums.get(song.album_id.as_ref()?)?;

        Some(PlayingInfo {
            album_name: album.name.clone(),
            album_artist: album.artist.clone(),
            song_title: song.title.clone(),
            song_artist: song.artist.clone(),
            song_duration: Duration::from_secs(song.duration.unwrap_or(1) as u64),
            song_position,
        })
    }

    pub fn has_loaded_all_songs(&self) -> bool {
        let state = self.read_state();
        state.has_loaded_all_songs
    }

    pub fn get_error(&self) -> Option<String> {
        self.read_state().error.clone()
    }

    pub fn get_song_map(&'_ self) -> RwLockReadGuard<'_, SongMap> {
        self.song_map.read().unwrap()
    }

    pub fn clear_error(&self) {
        self.write_state().error = None;
    }

    // Queue and playback management methods
    pub fn set_playback_mode(&self, mode: PlaybackMode) {
        self.write_state().playback_mode = mode;
    }

    pub fn get_playback_mode(&self) -> PlaybackMode {
        self.read_state().playback_mode
    }

    pub fn play_song(&self, song_id: &SongId) {
        let client = self.client.clone();
        let state = self.state.clone();
        let song_id = song_id.clone();
        let logic_tx = self.logic_to_playback_tx.clone();
        let transcode = self.transcode;
        let song_map = self.song_map.clone();

        state.write().unwrap().next_song = Some(song_id.clone());

        self.spawn(async move {
            let response = client
                .stream(&song_id.0, transcode.then(|| "mp3".to_string()), None)
                .await;

            match response {
                Ok(data) => {
                    logic_tx.send(LogicToPlaybackMessage::StopPlayback).unwrap();

                    let playing_info = create_playing_info_for_song(&song_id, &song_map, &state)
                        .unwrap_or_else(|| {
                            panic!("Failed to create playing info for song {song_id}")
                        });
                    logic_tx
                        .send(LogicToPlaybackMessage::PlaySong(data, playing_info))
                        .unwrap();
                    state.write().unwrap().playing_song = Some(PlayingSong {
                        song_id,
                        position: Duration::from_secs(0),
                    });
                }
                Err(e) => {
                    let mut state = state.write().unwrap();
                    state.error = Some(e.to_string());
                }
            }
            state.write().unwrap().next_song = None;
        });
    }
}
impl Logic {
    pub fn spawn(&self, task: impl Future<Output = ()> + Send + Sync + 'static) {
        self.tokio.spawn(task);
    }

    fn initial_fetch(&self) {
        let client = self.client.clone();
        let state = self.state.clone();
        let song_map = self.song_map.clone();
        self.spawn(async move {
            let future = {
                let state = state.clone();
                async move {
                    client.ping().await?;

                    let result = state::fetch_all(&client, |batch_count, total_count| {
                        tracing::info!("Fetched {batch_count} songs, total {total_count} songs");
                    })
                    .await?;

                    {
                        let mut state = state.write().unwrap();
                        state.albums = result.albums;
                        state.songs = result.songs.keys().cloned().collect();
                        state.groups = result.groups;
                        state.has_loaded_all_songs = true;
                    }

                    {
                        let mut song_map = song_map.write().unwrap();
                        *song_map = result.songs;
                    }

                    bs::ClientResult::Ok(())
                }
            };

            if let Err(e) = future.await {
                state.write().unwrap().error = Some(e.to_string());
            }
        })
    }

    fn write_state(&'_ self) -> RwLockWriteGuard<'_, AppState> {
        self.state.write().unwrap()
    }

    fn read_state(&'_ self) -> RwLockReadGuard<'_, AppState> {
        self.state.read().unwrap()
    }
}

struct AppState {
    songs: Vec<SongId>,
    groups: Vec<Arc<Group>>,
    albums: HashMap<AlbumId, Album>,
    cover_art_cache: HashMap<String, (Vec<u8>, std::time::Instant)>,
    pending_cover_art_requests: HashSet<String>,
    has_loaded_all_songs: bool,

    playing_song: Option<PlayingSong>,
    next_song: Option<SongId>,
    playback_mode: PlaybackMode,

    error: Option<String>,
}

#[derive(Debug, Clone)]
struct PlayingSong {
    song_id: SongId,
    position: Duration,
}

enum LogicToPlaybackMessage {
    PlaySong(Vec<u8>, PlayingInfo),
    TogglePlayback,
    Play,
    Pause,
    StopPlayback,
    Seek(Duration),
}

#[derive(Clone)]
struct TokioHandle(tokio::sync::mpsc::Sender<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>);
impl TokioHandle {
    fn spawn(&self, task: impl Future<Output = ()> + Send + Sync + 'static) {
        self.0.blocking_send(Box::pin(task)).unwrap();
    }
}
