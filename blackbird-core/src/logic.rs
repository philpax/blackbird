use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};

use crate::{
    bs,
    state::{Album, AlbumId, Group, Song, SongId, SongMap},
};

pub struct Logic {
    tokio: TokioHandle,
    _tokio_thread_handle: std::thread::JoinHandle<()>,
    state: Arc<RwLock<AppState>>,
    song_map: Arc<RwLock<SongMap>>,
    client: Arc<bs::Client>,
    playback_thread_tx: std::sync::mpsc::Sender<LogicThreadMessage>,
    _playback_thread_handle: std::thread::JoinHandle<()>,
}

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

impl Logic {
    const MAX_CONCURRENT_COVER_ART_REQUESTS: usize = 10;
    const MAX_COVER_ART_CACHE_SIZE: usize = 32;

    pub fn new(base_url: String, username: String, password: String) -> Self {
        let state = Arc::new(RwLock::new(AppState {
            songs: vec![],
            groups: vec![],
            albums: HashMap::new(),
            cover_art_cache: HashMap::new(),
            pending_cover_art_requests: HashSet::new(),
            has_loaded_all_songs: false,
            is_loading_song: false,
            playing_song: None,
            error: None,
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

        // Initialize audio output in the playback thread
        let (playback_thread_tx, playback_thread_rx) = std::sync::mpsc::channel();
        let playback_thread_handle = std::thread::spawn({
            let state = state.clone();
            move || {
                let (_output_stream, output_stream_handle) =
                    rodio::OutputStream::try_default().unwrap();
                let sink = rodio::Sink::try_new(&output_stream_handle).unwrap();
                sink.set_volume(1.0);

                let mut last_data = None;
                loop {
                    // Process all available messages without blocking
                    while let Ok(msg) = playback_thread_rx.try_recv() {
                        match msg {
                            LogicThreadMessage::PlaySong(data) => {
                                sink.clear();
                                last_data = Some(data.clone());
                                sink.append(
                                    rodio::Decoder::new(std::io::Cursor::new(data)).unwrap(),
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
                                            rodio::Decoder::new(std::io::Cursor::new(data))
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

                    {
                        let mut state = state.write().unwrap();
                        if let Some(playing_song) = state.playing_song.as_mut() {
                            playing_song.position = sink.get_pos();
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
            playback_thread_tx,
            _playback_thread_handle: playback_thread_handle,
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
        let mut visible_groups = Vec::new();
        // We'll set this to the actual start row of first visible album
        let mut start_row = 0;
        let mut first_visible_found = false;

        for group in &state.groups {
            let group_lines = group_line_count_getter(group) + group_margin_bottom_row_count;
            let group_range = current_row..(current_row + group_lines);

            // If this album starts after the visible range, we can break out
            if group_range.start >= visible_row_range.end {
                break;
            }

            // If this album is completely above the visible range, skip it
            if group_range.end <= visible_row_range.start {
                current_row += group_lines;
                continue;
            }

            // Found first visible album - record its starting row
            if !first_visible_found {
                start_row = current_row;
                first_visible_found = true;
            }

            visible_groups.push(group.clone());
            current_row += group_lines;
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

    pub fn play_song(&self, song_id: &SongId) {
        let client = self.client.clone();
        let state = self.state.clone();
        let song_id = song_id.clone();
        let logic_tx = self.playback_thread_tx.clone();

        self.spawn(async move {
            state.write().unwrap().is_loading_song = true;
            match client.download(&song_id.0).await {
                Ok(data) => {
                    // Update the state to reflect the new playing song
                    {
                        let mut state = state.write().unwrap();
                        state.playing_song = Some(PlayingSong {
                            song_id: song_id.clone(),
                            position: Duration::from_secs(0),
                        });
                        state.is_loading_song = false;
                    }

                    // Send the data to the logic thread to play
                    logic_tx.send(LogicThreadMessage::PlaySong(data)).unwrap();
                }
                Err(e) => {
                    let mut state = state.write().unwrap();
                    state.error = Some(e.to_string());
                }
            }
        });
    }

    pub fn toggle_playback(&self) {
        self.playback_thread_tx
            .send(LogicThreadMessage::TogglePlayback)
            .unwrap();
    }

    pub fn stop_playback(&self) {
        self.playback_thread_tx
            .send(LogicThreadMessage::StopPlayback)
            .unwrap();
    }

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
            .map(|s| s.song_id.clone())
    }

    pub fn is_song_loaded(&self) -> bool {
        self.read_state().playing_song.is_some()
    }

    pub fn is_song_loading(&self) -> bool {
        self.read_state().is_loading_song
    }

    pub fn get_playing_info(&self) -> Option<PlayingInfo> {
        let state = self.read_state();
        let song_map = self.song_map.read().unwrap();
        let playing_song = state.playing_song.as_ref()?;
        let song = song_map.get(&playing_song.song_id)?;
        let album = state.albums.get(song.album_id.as_ref()?)?;
        Some(PlayingInfo {
            album_name: album.name.clone(),
            album_artist: album.artist.clone(),
            song_title: song.title.clone(),
            song_artist: song.artist.clone(),
            song_duration: Duration::from_secs(song.duration.unwrap_or(1) as u64),
            song_position: playing_song.position,
        })
    }

    pub fn has_loaded_all_songs(&self) -> bool {
        let state = self.read_state();
        state.has_loaded_all_songs
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
        let song_map = self.song_map.clone();
        self.spawn(async move {
            if let Err(e) = client.ping().await {
                state.write().unwrap().error = Some(e.to_string());
                return;
            }

            match Album::fetch_all(&client).await {
                Ok(albums) => {
                    let mut state = state.write().unwrap();
                    state.albums = albums.into_iter().map(|a| (a.id.clone(), a)).collect();
                }
                Err(e) => {
                    state.write().unwrap().error = Some(e.to_string());
                }
            };

            let mut offset = 0;
            loop {
                let response = client
                    .search3(&bs::Search3Request {
                        query: "".to_string(),
                        artist_count: Some(0),
                        album_count: Some(0),
                        song_count: Some(10000),
                        song_offset: Some(offset),
                        ..Default::default()
                    })
                    .await
                    .unwrap();

                if response.song.is_empty() {
                    break;
                }

                let song_count = response.song.len();
                {
                    let mut song_map = song_map.write().unwrap();
                    for song in response.song {
                        let song = Song::from(song);
                        song_map.insert(song.id.clone(), song);
                    }
                }
                tracing::info!("Fetched {song_count} songs");
                offset += song_count as u32;
            }

            {
                let song_map = song_map.read().unwrap();
                let mut state = state.write().unwrap();
                // Get all song IDs.
                state.songs = song_map.keys().cloned().collect();
                // This is all mad ineffcient but cbf doing it better.
                // Sort songs.
                {
                    let song_data: HashMap<SongId, _> = state
                        .songs
                        .iter()
                        .map(|id| {
                            let song = song_map.get(id).unwrap_or_else(|| {
                                panic!("Song not found in song map: {id}");
                            });
                            let album_id = song.album_id.as_ref().unwrap_or_else(|| {
                                panic!("Album ID not found in song: {:?}", song);
                            });
                            let album = state.albums.get(album_id).unwrap_or_else(|| {
                                panic!("Album not found in state: {:?}", album_id);
                            });
                            (
                                id.clone(),
                                (
                                    album.artist.clone(),
                                    album.year.unwrap_or_default(),
                                    album.name.clone(),
                                    song.disc_number.unwrap_or_default(),
                                    song.track.unwrap_or_default(),
                                    song.title.clone(),
                                ),
                            )
                        })
                        .collect();
                    state
                        .songs
                        .sort_by_cached_key(|id| song_data.get(id).unwrap());
                }
                // Build groups.
                let mut new_groups = vec![];
                let mut current_group: Option<Group> = None;
                for song_id in &state.songs {
                    let song = song_map.get(song_id).unwrap_or_else(|| {
                        panic!("Song not found in song map: {song_id}");
                    });
                    let album_id = song.album_id.as_ref().unwrap_or_else(|| {
                        panic!("Album ID not found in song: {:?}", song);
                    });
                    let album = state.albums.get(album_id).unwrap_or_else(|| {
                        panic!("Album not found in state: {:?}", album_id);
                    });

                    if current_group.is_none() || matches!(&current_group, Some(group) if !(group.artist == album.artist && group.album == album.name && group.year == album.year)) {
                        if let Some(group) = current_group.take() {
                            new_groups.push(Arc::new(group));
                        }

                        current_group = Some(Group {
                            artist: album.artist.clone(),
                            album: album.name.clone(),
                            year: album.year,
                            duration: album.duration,
                            songs: vec![],
                            cover_art_id: album.cover_art_id.clone(),
                        });
                    }

                    current_group.as_mut().unwrap().songs.push(song_id.clone());
                }
                if let Some(group) = current_group.take() {
                    new_groups.push(Arc::new(group));
                }
                state.groups = new_groups;

                state.has_loaded_all_songs = true;
            }
        })
    }

    fn write_state(&self) -> RwLockWriteGuard<AppState> {
        self.state.write().unwrap()
    }

    fn read_state(&self) -> RwLockReadGuard<AppState> {
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

    is_loading_song: bool,
    playing_song: Option<PlayingSong>,
    error: Option<String>,
}

struct PlayingSong {
    song_id: SongId,
    position: Duration,
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
