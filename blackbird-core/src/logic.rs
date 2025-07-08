use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};

use crate::{
    bs,
    queue::{PlaybackMode, Queue, SharedQueue},
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
    queue: SharedQueue,
    transcode: bool,
    cache_size: usize,
    default_shuffle: bool,
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

    pub fn new(
        base_url: String,
        username: String,
        password: String,
        transcode: bool,
        cache_size: usize,
        default_shuffle: bool,
    ) -> Self {
        let state = Arc::new(RwLock::new(AppState {
            songs: vec![],
            groups: vec![],
            albums: HashMap::new(),
            cover_art_cache: HashMap::new(),
            pending_cover_art_requests: HashSet::new(),
            has_loaded_all_songs: false,
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

        // Create the shared queue
        let queue = Arc::new(std::sync::RwLock::new(Queue::new(
            vec![],
            if default_shuffle {
                PlaybackMode::Shuffle
            } else {
                PlaybackMode::Sequential
            },
            cache_size,
        )));

        // Initialize audio output in the playback thread
        let (playback_thread_tx, playback_thread_rx) = std::sync::mpsc::channel();
        let playback_thread_handle = std::thread::spawn({
            let queue = queue.clone();
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

                loop {
                    // Process all available messages without blocking
                    while let Ok(msg) = playback_thread_rx.try_recv() {
                        match msg {
                            LogicThreadMessage::PlaySong(data) => {
                                sink.clear();
                                last_data = Some(data.clone());
                                sink.append(build_decoder(data));
                                sink.play();
                            }
                            LogicThreadMessage::TogglePlayback => {
                                if !sink.is_paused() {
                                    sink.pause();
                                    continue;
                                }
                                if sink.empty() {
                                    if let Some(data) = last_data.clone() {
                                        sink.append(build_decoder(data));
                                    }
                                }
                                sink.play();
                            }
                            LogicThreadMessage::StopPlayback => {
                                sink.clear();
                            }
                            LogicThreadMessage::Seek(position) => {
                                let now = std::time::Instant::now();
                                if now.duration_since(last_seek_time) >= SEEK_DEBOUNCE_DURATION {
                                    last_seek_time = now;
                                    if let Err(e) = sink.try_seek(position) {
                                        // Log error but don't crash - seeking may fail for various reasons
                                        tracing::warn!(
                                            "Failed to seek to position {position:?}: {e}"
                                        );
                                    }
                                }
                                // Drop seeks that come in too soon
                            }
                        }
                    }

                    // Check if we should auto-advance to next track
                    if sink.empty() {
                        if let Some(data) = last_data.clone() {
                            sink.append(build_decoder(data));
                        }
                    }

                    // Update playing position in queue
                    {
                        let mut queue_guard = queue.write().unwrap();
                        queue_guard.update_playing_position(sink.get_pos());
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
            queue: queue.clone(),
            transcode,
            cache_size,
            default_shuffle,
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
        self.playback_thread_tx
            .send(LogicThreadMessage::TogglePlayback)
            .unwrap();
    }

    pub fn stop_playback(&self) {
        self.queue.write().unwrap().stop_playing();
        self.playback_thread_tx
            .send(LogicThreadMessage::StopPlayback)
            .unwrap();
    }

    pub fn seek(&self, position: Duration) {
        self.playback_thread_tx
            .send(LogicThreadMessage::Seek(position))
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
        self.queue
            .read()
            .unwrap()
            .get_playing_song()
            .map(|(song_id, _)| song_id)
    }

    pub fn is_song_loaded(&self) -> bool {
        self.queue.read().unwrap().is_playing()
    }

    pub fn is_song_loading(&self) -> bool {
        self.queue.read().unwrap().is_loading_song()
    }

    pub fn get_playing_info(&self) -> Option<PlayingInfo> {
        let song_map = self.song_map.read().unwrap();
        let (song_id, song_position) = self.queue.read().unwrap().get_playing_song()?;
        let song = song_map.get(&song_id)?;
        let state = self.read_state();
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

    pub fn get_song_map(&self) -> RwLockReadGuard<SongMap> {
        self.song_map.read().unwrap()
    }

    pub fn clear_error(&self) {
        self.write_state().error = None;
    }

    // Queue and playback management methods
    pub fn set_playback_mode(&self, mode: PlaybackMode) {
        let mut queue = self.queue.write().unwrap();
        queue.set_mode(mode);
    }

    pub fn get_playback_mode(&self) -> PlaybackMode {
        self.queue.read().unwrap().get_mode()
    }

    pub fn next_track(&self) {
        let next_song_id = {
            let mut queue = self.queue.write().unwrap();
            queue.advance_to_next().cloned()
        };

        if let Some(song_id) = next_song_id {
            self.play_song_from_cache(&song_id);
        }
    }

    pub fn previous_track(&self) {
        let prev_song_id = {
            let mut queue = self.queue.write().unwrap();
            queue.advance_to_previous().cloned()
        };

        if let Some(song_id) = prev_song_id {
            self.play_song_from_cache(&song_id);
        }
    }

    pub fn play_song(&self, song_id: &SongId) {
        // Initialize queue with all songs when a song is selected
        let songs = self.read_state().songs.clone();

        {
            let mut queue = self.queue.write().unwrap();

            // Use default shuffle mode when starting fresh, otherwise preserve current mode
            let mode = if queue.is_empty() {
                if self.default_shuffle {
                    PlaybackMode::Shuffle
                } else {
                    PlaybackMode::Sequential
                }
            } else {
                queue.get_mode()
            };

            *queue = Queue::new(songs, mode, self.cache_size);
            queue.jump_to_song(song_id);
        }

        // Start cache maintenance
        self.update_track_cache();

        // Play the song
        self.play_song_from_cache(song_id);
    }

    fn play_song_from_cache(&self, song_id: &SongId) {
        // Try to start playing from cache using the queue
        let cached_data = {
            let mut queue = self.queue.write().unwrap();
            queue.start_playing(song_id)
        };

        if let Some(cached_data) = cached_data {
            // Send cached data to playback thread - clear sink first
            self.playback_thread_tx
                .send(LogicThreadMessage::StopPlayback)
                .unwrap();
            self.playback_thread_tx
                .send(LogicThreadMessage::PlaySong(cached_data))
                .unwrap();

            // Update cache window
            self.update_track_cache();
            return;
        }

        // Song not cached, load it normally
        let client = self.client.clone();
        let state = self.state.clone();
        let queue = self.queue.clone();
        let song_id = song_id.clone();
        let logic_tx = self.playback_thread_tx.clone();
        let transcode = self.transcode;

        self.spawn(async move {
            queue.write().unwrap().start_loading_song(song_id.clone());
            let response = client
                .stream(&song_id.0, transcode.then(|| "mp3".to_string()), None)
                .await;

            match response {
                Ok(data) => {
                    // Finish loading and start playing
                    {
                        let mut queue_guard = queue.write().unwrap();
                        queue_guard.finish_loading_song();
                        queue_guard.cache_track(song_id.clone(), data.clone());
                        queue_guard.start_playing(&song_id);
                    }

                    // Clear sink and send the data to the logic thread to play
                    logic_tx.send(LogicThreadMessage::StopPlayback).unwrap();
                    logic_tx.send(LogicThreadMessage::PlaySong(data)).unwrap();
                }
                Err(e) => {
                    queue.write().unwrap().finish_loading_song();
                    let mut state = state.write().unwrap();
                    state.error = Some(e.to_string());
                }
            }
        });
    }

    fn update_track_cache(&self) {
        // Get songs that need caching from the queue
        let songs_needing_cache = {
            let queue = self.queue.read().unwrap();
            if queue.can_start_more_requests() {
                queue.get_songs_needing_cache()
            } else {
                Vec::new()
            }
        };

        // Start loading tracks that aren't cached yet
        for song_id in songs_needing_cache {
            self.cache_track(song_id);
        }
    }

    fn cache_track(&self, song_id: SongId) {
        // Mark as pending in the queue
        self.queue
            .write()
            .unwrap()
            .mark_track_pending(song_id.clone());

        self.spawn({
            let client = self.client.clone();
            let queue = self.queue.clone();
            let transcode = self.transcode;

            async move {
                match client
                    .stream(&song_id.0, transcode.then(|| "mp3".to_string()), None)
                    .await
                {
                    Ok(data) => {
                        queue.write().unwrap().cache_track(song_id.clone(), data);
                    }
                    Err(e) => {
                        queue.write().unwrap().remove_pending_request(&song_id);
                        // Don't set error for cache failures, just log
                        tracing::warn!("Failed to cache track {}: {}", song_id, e);
                    }
                }
            }
        });
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

            // Fetch all albums.
            match Album::fetch_all(&client).await {
                Ok(albums) => {
                    let mut state = state.write().unwrap();
                    state.albums = albums.into_iter().map(|a| (a.id.clone(), a)).collect();
                }
                Err(e) => {
                    state.write().unwrap().error = Some(e.to_string());
                }
            };

            // Fetch all songs.
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

            // Rename all [Unknown Album] single-track albums to be the song title.
            {
                let song_map = song_map.read().unwrap();
                let mut state = state.write().unwrap();

                let mut albums_to_rewrite = HashSet::new();
                for album in state.albums.values_mut() {
                    if album.name == "[Unknown Album]" && album.song_count == 1 {
                        albums_to_rewrite.insert(album.id.clone());
                    }
                }

                for song in song_map.values() {
                    let Some(album_id) = song.album_id.as_ref() else {
                        continue;
                    };
                    if albums_to_rewrite.contains(album_id) {
                        if let Some(album) = state.albums.get_mut(album_id) {
                            album.name = song.title.clone();
                        }
                    }
                }
            }

            // Build groups.
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
                                panic!("Album ID not found in song: {song:?}");
                            });
                            let album = state.albums.get(album_id).unwrap_or_else(|| {
                                panic!("Album not found in state: {album_id:?}");
                            });
                            (
                                id.clone(),
                                (
                                    album.artist.to_lowercase(),
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
                        panic!("Album ID not found in song: {song:?}");
                    });
                    let album = state.albums.get(album_id).unwrap_or_else(|| {
                        panic!("Album not found in state: {album_id:?}");
                    });

                    if !current_group.as_ref().is_some_and(|group| {
                        group.artist == album.artist
                            && group.album == album.name
                            && group.year == album.year
                    }) {
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

    error: Option<String>,
}

enum LogicThreadMessage {
    PlaySong(Vec<u8>),
    TogglePlayback,
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
