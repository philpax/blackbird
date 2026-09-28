//! Connecting to the server and loading the library on startup (or after the
//! server settings change).
//!
//! The sequence is:
//!
//! 1. If a track is being restored, claim it as the queue target, so that the
//!    queue is built around it however the rest of the sequence interleaves.
//! 2. Load the library from the on-disk cache, if there is one, and show it.
//! 3. Ping the server, then start the playback thread and restore the track.
//! 4. Re-fetch the library from the server. If it differs from the cache,
//!    replace the library with it and update the cache.
//!
//! Each run belongs to a generation of the server settings. If the settings
//! change mid-run (see [`Logic::reload_library`](crate::Logic::reload_library)),
//! the stale run stops touching the shared state.
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use blackbird_state::{FetchAllOutput, RawLibrary, TrackId};

use crate::{
    AppState, AppStateError, Library, bs,
    library_cache::{self, LibraryCache},
    playback_thread::{PlaybackThread, PlaybackThreadSendHandle, PlaybackToLogicMessage},
    queue,
};

/// Creates a playback thread from the initial volume, whether to apply
/// ReplayGain, the ReplayGain preamp, and the channel for playback events.
pub(crate) type NewPlaybackThread =
    fn(f32, bool, f32, tokio::sync::broadcast::Sender<PlaybackToLogicMessage>) -> PlaybackThread;

/// The generation of the server settings that a task belongs to.
#[derive(Clone)]
pub(crate) struct Generation {
    generation: u64,
    current: Arc<AtomicU64>,
}
impl Generation {
    /// Returns the current generation of `current`.
    pub fn current(current: &Arc<AtomicU64>) -> Self {
        Self {
            generation: current.load(Ordering::SeqCst),
            current: current.clone(),
        }
    }

    /// Whether the server settings are unchanged since this generation. Tasks
    /// should check this while holding the lock on whatever they're about to
    /// modify, so that a reload can't interleave.
    pub fn is_current(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.generation
    }
}

/// Everything a run of the initial fetch needs.
pub(crate) struct InitialFetch {
    pub client: Arc<bs::Client>,
    pub state: Arc<RwLock<AppState>>,
    pub cache: Option<LibraryCache>,
    /// The generation this run belongs to; the run stops touching the state
    /// once it is stale.
    pub generation: Generation,
    pub transcode: bool,
    pub library_populated_tx: std::sync::mpsc::Sender<()>,
    pub playback_event_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
    pub new_playback_thread: NewPlaybackThread,
    /// Where the playback thread is deposited for `Logic::update` to pick up.
    pub playback_thread_slot: Arc<Mutex<Option<PlaybackThread>>>,
    /// Set whenever the library is replaced, so that `Logic::update` can
    /// refresh what depends on the queue (the audio prefetch window, and the
    /// gapless next track).
    pub library_replaced: Arc<AtomicBool>,
}

/// A track restore whose queue target has been claimed.
struct ClaimedRestore {
    track_id: TrackId,
    position: Duration,
    request_id: u64,
}

impl InitialFetch {
    pub async fn run(self, restore_track: Option<(TrackId, Duration)>) {
        let restore =
            restore_track.map(|(track_id, position)| self.claim_restore(track_id, position));

        let cached_encoded = self.load_cache().await;
        let has_library = cached_encoded.is_some();

        if let Err(error) = self.client.ping().await {
            if let Some(restore) = &restore {
                self.release_restore(restore);
            }
            self.report_unreachable(error, has_library);
            return;
        }

        let Some(playback_tx) = self.start_playback_thread() else {
            return;
        };
        if let Some(restore) = restore {
            tokio::spawn(restore_last_track(
                self.client.clone(),
                self.state.clone(),
                self.generation.clone(),
                playback_tx,
                restore,
                self.transcode,
            ));
        }

        let ((), result) = tokio::join!(
            self.fetch_extensions(),
            self.refresh_library(cached_encoded)
        );
        if let Err(error) = result {
            self.report_failure(error, has_library);
        }
    }

    fn claim_restore(&self, track_id: TrackId, position: Duration) -> ClaimedRestore {
        let mut st = self.state.write().unwrap();
        st.queue.current_target = Some(track_id.clone());
        st.queue.request_counter = st.queue.request_counter.wrapping_add(1);
        st.queue.current_target_request_id = Some(st.queue.request_counter);
        ClaimedRestore {
            track_id,
            position,
            request_id: st.queue.request_counter,
        }
    }

    fn release_restore(&self, restore: &ClaimedRestore) {
        let mut st = self.state.write().unwrap();
        if self.generation.is_current() {
            queue::release_target(&mut st, &restore.track_id, restore.request_id);
        }
    }

    /// Loads the library from the cache and shows it. Returns the cache's
    /// encoded contents if it was loaded, for comparison with the fresh fetch.
    async fn load_cache(&self) -> Option<Vec<u8>> {
        let cache = self.cache.clone()?;
        let (fetched, encoded) = run_blocking(move || match cache.load() {
            Ok(Some(cached)) => Some((cached.library.build(), cached.encoded)),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("Ignoring the library cache: failed to {e}");
                None
            }
        })
        .await?;

        log_orphans(&fetched, "the library cache");
        let track_count = fetched.track_ids.len();
        if !self.apply(fetched).await {
            return None;
        }
        tracing::info!("Loaded {track_count} tracks from the library cache");
        Some(encoded)
    }

    /// Fetches the server's OpenSubsonic extensions. This deliberately does
    /// not return an error: a server that errors on `getOpenSubsonicExtensions`
    /// must not turn into a failed library fetch — the extensions are an
    /// enhancement, and the error is surfaced non-fatally instead.
    async fn fetch_extensions(&self) {
        let result = self.client.get_open_subsonic_extensions().await;
        let mut st = self.state.write().unwrap();
        if !self.generation.is_current() {
            return;
        }
        match result {
            Ok(extensions) => st.open_subsonic_extensions = extensions,
            Err(e) => {
                tracing::warn!("Failed to fetch OpenSubsonic extensions: {e}");
                st.error = Some(AppStateError::OpenSubsonicExtensionsFetchFailed {
                    error: e.to_string(),
                });
            }
        }
    }

    /// Fetches the library from the server and, if it differs from
    /// `cached_encoded`, replaces the library with it and updates the cache.
    async fn refresh_library(&self, cached_encoded: Option<Vec<u8>>) -> bs::ClientResult<()> {
        self.state.write().unwrap().library.begin_refresh();

        let raw = blackbird_state::fetch_raw(&self.client, |batch_count, total_count| {
            tracing::info!("Fetched {batch_count} tracks, total {total_count} tracks");
        })
        .await?;

        let cache = self.cache.clone();
        let Some(fetched) =
            run_blocking(move || rebuild_if_changed(raw, cached_encoded, cache)).await
        else {
            tracing::info!("The library is unchanged since it was cached");
            return Ok(());
        };

        log_orphans(&fetched, "the server");
        let track_count = fetched.track_ids.len();
        if self.apply(fetched).await {
            tracing::info!("Loaded {track_count} tracks from the server");
        }
        Ok(())
    }

    /// Replaces the library with `fetched`, rebuilding the queue around the
    /// current track. Returns `false` if this run is stale.
    async fn apply(&self, fetched: FetchAllOutput) -> bool {
        // Build the library without holding the lock, as it takes a while;
        // only carrying over the old library's state has to happen under it.
        let sort_order = self.state.read().unwrap().sort_order;
        let mut library = run_blocking(move || Library::from_fetched(fetched, sort_order)).await;

        let previous = {
            let mut st = self.state.write().unwrap();
            // Checked under the lock, so that a reload can't clear the state
            // between this check and the replacement.
            if !self.generation.is_current() {
                return false;
            }

            if st.sort_order != sort_order {
                library.resort(st.sort_order);
            }
            let playing = st
                .current_track_and_position
                .as_ref()
                .map(|t| t.track_id.clone());
            // A track that is being restored hasn't started playing yet, but
            // its details (fetched by the restore) should be kept too.
            let anchor = st.queue.current_target.clone().or(playing);
            library.carry_over_from(&st.library, st.sort_order, anchor.as_ref());
            let previous = std::mem::replace(&mut st.library, library);

            queue::recompute_queue_on_state(&mut st, anchor.as_ref());
            previous
        };
        // Dropping a large library takes a moment, so do it outside the lock.
        drop(previous);

        self.library_replaced.store(true, Ordering::SeqCst);
        let _ = self.library_populated_tx.send(());
        true
    }

    /// Starts the playback thread (opening the audio device) and deposits it
    /// for the main thread to pick up. Returns `None` if this run is stale.
    fn start_playback_thread(&self) -> Option<PlaybackThreadSendHandle> {
        let (volume, apply_replaygain, replaygain_preamp_db) = {
            let st = self.state.read().unwrap();
            (st.volume, st.apply_replaygain, st.replaygain_preamp_db)
        };
        let pt = (self.new_playback_thread)(
            volume,
            apply_replaygain,
            replaygain_preamp_db,
            self.playback_event_tx.clone(),
        );
        let playback_tx = pt.send_handle();

        // Checked while holding the slot, which a reload empties after bumping
        // the generation, so that a stale thread can't be deposited after the
        // reload has emptied the slot.
        let mut slot = self.playback_thread_slot.lock().unwrap();
        if !self.generation.is_current() {
            return None;
        }
        *slot = Some(pt);
        Some(playback_tx)
    }

    /// Reports that the server couldn't be reached at all.
    fn report_unreachable(&self, error: bs::ClientError, has_library: bool) {
        {
            let mut st = self.state.write().unwrap();
            if !self.generation.is_current() {
                return;
            }
            // Without a server, tracks can't be played. Refuse further play
            // requests, and cancel any made before the connection failed.
            st.server_unreachable = Some(error.to_string());
            if st.started_loading_track.take().is_some()
                && let (Some(track_id), Some(request_id)) = (
                    st.queue.current_target.clone(),
                    st.queue.current_target_request_id,
                )
            {
                queue::release_target(&mut st, &track_id, request_id);
            }
        }
        self.report_failure(error, has_library);
    }

    fn report_failure(&self, error: bs::ClientError, has_library: bool) {
        let mut st = self.state.write().unwrap();
        if !self.generation.is_current() {
            return;
        }
        let error = error.to_string();
        if has_library {
            tracing::warn!("Failed to refresh the library: {error}");
            st.error = Some(AppStateError::LibraryRefreshFailed { error });
        } else {
            st.error = Some(AppStateError::InitialFetchFailed { error });
            drop(st);
            // Notify clients so they leave the loading state and render the
            // connection error instead of staying on a frozen loading screen.
            // Nothing else sets `changed` during loading (no playback, no
            // library events), so without this signal the error wouldn't
            // appear until the user interacted.
            let _ = self.library_populated_tx.send(());
        }
    }
}

/// Builds `raw` into a library, unless it is identical to `cached_encoded`, in
/// which case there is nothing to do and `None` is returned. A changed library
/// is written to `cache`.
fn rebuild_if_changed(
    raw: RawLibrary,
    cached_encoded: Option<Vec<u8>>,
    cache: Option<LibraryCache>,
) -> Option<FetchAllOutput> {
    let encoded = library_cache::encode(&raw);
    if cached_encoded.as_deref() == Some(encoded.as_slice()) {
        return None;
    }
    if let Some(cache) = cache
        && let Err(e) = cache.store(&encoded)
    {
        tracing::warn!("Failed to {e}");
    }
    Some(raw.build())
}

/// Logs the tracks that `build` dropped for lacking a known album.
fn log_orphans(fetched: &FetchAllOutput, source: &str) {
    const SHOWN: usize = 10;
    let orphans = &fetched.orphaned_track_ids;
    if !orphans.is_empty() {
        tracing::warn!(
            "Ignoring {} tracks from {source} without a known album, including {:?}",
            orphans.len(),
            &orphans[..orphans.len().min(SHOWN)]
        );
    }
}

/// Restores the last played track, loading it paused at its last position.
///
/// This runs independently of the library fetch. The track's metadata is
/// fetched directly so that the now-playing display and ReplayGain work before
/// the library has loaded; this also confirms that the track still exists on
/// the server.
async fn restore_last_track(
    client: Arc<bs::Client>,
    state: Arc<RwLock<AppState>>,
    generation: Generation,
    playback_tx: PlaybackThreadSendHandle,
    restore: ClaimedRestore,
    transcode: bool,
) {
    let ClaimedRestore {
        track_id,
        position,
        request_id,
    } = restore;

    let song = client.get_song(track_id.0.as_str()).await;
    {
        let mut st = state.write().unwrap();
        if !generation.is_current() {
            return;
        }
        match song {
            Ok(child) => {
                st.library.track_map.insert(track_id.clone(), child.into());
                // Registered so that the audio prefetch doesn't request the
                // track again while it downloads.
                st.queue
                    .pending_audio_requests
                    .insert(track_id.clone(), request_id);
            }
            Err(e) => {
                tracing::warn!("Not restoring last track {}: {e}", track_id.0);
                queue::release_target(&mut st, &track_id, request_id);
                return;
            }
        }
    }

    tracing::info!(
        "Restoring last track {} at {:.1}s",
        track_id.0,
        position.as_secs_f64()
    );
    let response = client
        .stream(
            track_id.0.as_str(),
            transcode.then(|| "mp3".to_string()),
            None,
        )
        .await;
    if !generation.is_current() {
        return;
    }
    queue::handle_load_response(
        response,
        state,
        playback_tx,
        track_id,
        request_id,
        queue::TrackLoadBehavior::Paused(position),
    );
}

/// Runs CPU- or disk-bound work off the async worker threads.
async fn run_blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    match tokio::task::spawn_blocking(f).await {
        Ok(value) => value,
        // Propagate panics as if the work had run inline. The task can
        // otherwise only fail by being cancelled, which only happens when the
        // runtime shuts down, taking this task with it.
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::mpsc,
    };

    use super::*;
    use crate::library_cache::tests::TestDir;

    /// A canned Subsonic server serving a library of the given tracks, all on
    /// one album. Every other endpoint returns a Subsonic error.
    struct MockServer {
        base_url: String,
    }

    impl MockServer {
        fn spawn(track_ids: &[&str]) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            let songs: Vec<String> = track_ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    format!(
                        r#"{{"id":"{id}","isDir":false,"title":"Track {id}","track":{},"albumId":"a1","duration":180}}"#,
                        i + 1
                    )
                })
                .collect();
            let songs = songs.join(",");
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else {
                        break;
                    };
                    let songs = songs.clone();
                    std::thread::spawn(move || serve(stream, &songs));
                }
            });
            Self { base_url }
        }
    }

    fn serve(mut stream: std::net::TcpStream, songs: &str) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                break;
            }
        }

        let path = request_line.split_whitespace().nth(1).unwrap_or_default();
        let path = path.trim_start_matches('/');
        let path = path.strip_prefix("rest/").unwrap_or(path);
        let (endpoint, query) = path.split_once('?').unwrap_or((path, ""));
        let has_param = |param: &str| query.split('&').any(|p| p == param);

        let inner = match endpoint {
            "ping" => Some(String::new()),
            "getOpenSubsonicExtensions" => Some(r#","openSubsonicExtensions":[]"#.to_string()),
            "getAlbumList2" => Some(
                r#","albumList2":{"album":[{"id":"a1","name":"Album","artist":"Artist","songCount":1,"duration":180,"created":"2024-01-01T00:00:00Z"}]}"#
                    .to_string(),
            ),
            "search3" if has_param("songOffset=0") => {
                Some(format!(r#","searchResult3":{{"song":[{songs}]}}"#))
            }
            "search3" => Some(r#","searchResult3":{}"#.to_string()),
            _ => None,
        };
        let body = match inner {
            Some(inner) => format!(
                r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1"{inner}}}}}"#
            ),
            None => r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":0,"message":"not stubbed"}}}"#.to_string(),
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    }

    /// Returns a base URL that refuses connections.
    fn unreachable_base_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        base_url
    }

    fn test_dir(test_name: &str) -> TestDir {
        TestDir::new("initial-fetch", test_name)
    }

    struct Harness {
        run: InitialFetch,
        state: Arc<RwLock<AppState>>,
        populated_rx: mpsc::Receiver<()>,
        library_replaced: Arc<AtomicBool>,
    }

    fn harness(base_url: &str, cache_dir: &std::path::Path) -> Harness {
        let state = Arc::new(RwLock::new(AppState::default()));
        let (library_populated_tx, populated_rx) = mpsc::channel();
        let (playback_event_tx, _) = tokio::sync::broadcast::channel(16);
        let library_replaced = Arc::new(AtomicBool::new(false));
        let run = InitialFetch {
            client: Arc::new(bs::Client::new(
                base_url.to_string(),
                "user".to_string(),
                "password".to_string(),
                "blackbird-test".to_string(),
            )),
            state: state.clone(),
            cache: Some(LibraryCache::new(cache_dir, base_url, "user")),
            generation: Generation::current(&Arc::new(AtomicU64::new(0))),
            transcode: false,
            library_populated_tx,
            playback_event_tx,
            new_playback_thread: PlaybackThread::for_test,
            playback_thread_slot: Arc::new(Mutex::new(None)),
            library_replaced: library_replaced.clone(),
        };
        Harness {
            run,
            state,
            populated_rx,
            library_replaced,
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn library_track_ids(state: &RwLock<AppState>) -> Vec<String> {
        let st = state.read().unwrap();
        let mut ids: Vec<String> = st
            .library
            .track_ids
            .iter()
            .map(|id| id.0.to_string())
            .collect();
        ids.sort();
        ids
    }

    /// Fetches the library from `server` into the cache in `cache_dir`.
    fn warm_cache(server: &MockServer, cache_dir: &std::path::Path) {
        let h = harness(&server.base_url, cache_dir);
        block_on(h.run.run(None));
        assert_eq!(library_track_ids(&h.state), ["t1", "t2"]);
    }

    #[test]
    fn first_run_fetches_the_library_and_caches_it() {
        let dir = test_dir("first-run");
        let server = MockServer::spawn(&["t1", "t2"]);
        let h = harness(&server.base_url, &dir);
        let cache = h.run.cache.clone().unwrap();

        block_on(h.run.run(None));

        assert_eq!(library_track_ids(&h.state), ["t1", "t2"]);
        assert!(h.state.read().unwrap().error.is_none());
        assert_eq!(h.populated_rx.try_iter().count(), 1);
        assert!(h.library_replaced.load(Ordering::SeqCst));
        let cached = cache.load().unwrap().expect("the library was cached");
        assert_eq!(cached.library.tracks.len(), 2);
    }

    #[test]
    fn cached_library_is_shown_when_the_server_is_unreachable() {
        let dir = test_dir("offline");
        let server = MockServer::spawn(&["t1", "t2"]);
        warm_cache(&server, &dir);

        // The cache is keyed by server, so point the unreachable client at it.
        let mut h = harness(&unreachable_base_url(), &dir);
        h.run.cache = Some(LibraryCache::new(&dir, &server.base_url, "user"));
        block_on(h.run.run(None));

        assert_eq!(library_track_ids(&h.state), ["t1", "t2"]);
        assert!(matches!(
            h.state.read().unwrap().error,
            Some(AppStateError::LibraryRefreshFailed { .. })
        ));
        assert_eq!(h.populated_rx.try_iter().count(), 1);
    }

    #[test]
    fn unreachable_server_without_a_cache_fails_the_initial_fetch() {
        let dir = test_dir("offline-uncached");
        let h = harness(&unreachable_base_url(), &dir);
        block_on(h.run.run(None));

        let st = h.state.read().unwrap();
        assert!(!st.library.has_loaded_all_tracks);
        assert!(matches!(
            st.error,
            Some(AppStateError::InitialFetchFailed { .. })
        ));
        assert_eq!(h.populated_rx.try_iter().count(), 1);
    }

    #[test]
    fn changed_library_replaces_the_cached_one() {
        let dir = test_dir("changed");
        let old_server = MockServer::spawn(&["t1", "t2"]);
        warm_cache(&old_server, &dir);

        // The same cache, but the server now has a track added and one removed.
        let new_server = MockServer::spawn(&["t1", "t3", "t4"]);
        let mut h = harness(&new_server.base_url, &dir);
        let cache = LibraryCache::new(&dir, &old_server.base_url, "user");
        h.run.cache = Some(cache.clone());
        block_on(h.run.run(None));

        assert_eq!(library_track_ids(&h.state), ["t1", "t3", "t4"]);
        // Once for the cache, once for the refresh.
        assert_eq!(h.populated_rx.try_iter().count(), 2);
        let cached = cache.load().unwrap().unwrap();
        let cached_ids: Vec<&str> = cached
            .library
            .tracks
            .keys()
            .map(|id| id.0.as_str())
            .collect();
        assert_eq!(cached_ids, ["t1", "t3", "t4"]);
    }

    #[test]
    fn unchanged_library_is_not_replaced() {
        let dir = test_dir("unchanged");
        let server = MockServer::spawn(&["t1", "t2"]);
        warm_cache(&server, &dir);

        let h = harness(&server.base_url, &dir);
        block_on(h.run.run(None));

        assert_eq!(library_track_ids(&h.state), ["t1", "t2"]);
        // Only the cache load populated the library.
        assert_eq!(h.populated_rx.try_iter().count(), 1);
        assert!(h.state.read().unwrap().error.is_none());
    }

    #[test]
    fn stale_run_leaves_the_state_alone() {
        let dir = test_dir("stale");
        let server = MockServer::spawn(&["t1", "t2"]);
        let h = harness(&server.base_url, &dir);
        // The server settings changed before the run got going.
        h.run.generation.current.fetch_add(1, Ordering::SeqCst);
        let slot = h.run.playback_thread_slot.clone();
        block_on(h.run.run(None));

        let st = h.state.read().unwrap();
        assert!(!st.library.has_loaded_all_tracks);
        assert!(st.error.is_none());
        assert_eq!(h.populated_rx.try_iter().count(), 0);
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn restore_of_a_track_missing_from_the_cache_releases_the_queue() {
        let dir = test_dir("restore-missing");
        let server = MockServer::spawn(&["t1", "t2"]);
        warm_cache(&server, &dir);

        let h = harness(&server.base_url, &dir);
        let state = h.state.clone();
        let run = h.run;
        // `getSong` isn't stubbed, so the restore finds the track missing.
        block_on(async {
            run.run(Some((TrackId("gone".into()), Duration::from_secs(5))))
                .await;
            // The restore runs as its own task; give it a chance to finish.
            for _ in 0..100 {
                if state.read().unwrap().queue.current_target.is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let st = state.read().unwrap();
        assert_eq!(st.queue.current_target, None);
        let queue: Vec<&str> = st
            .queue
            .ordered_tracks
            .iter()
            .map(|id| id.0.as_str())
            .collect();
        assert!(
            !queue.contains(&"gone"),
            "queue still has the missing track: {queue:?}"
        );
    }

    /// Builds the output of a fetch of the given tracks, all on one album.
    fn fetched(track_ids: &[&str]) -> FetchAllOutput {
        let mut raw = RawLibrary::default();
        let album = blackbird_state::Album {
            id: blackbird_state::AlbumId("a1".into()),
            name: "Album".into(),
            artist: "Artist".into(),
            artist_id: None,
            cover_art_id: None,
            track_count: track_ids.len() as u32,
            duration: 0,
            year: None,
            _genre: None,
            starred: false,
            created: "2024-01-01T00:00:00Z".into(),
        };
        raw.albums.insert(album.id.clone(), album);
        for (i, id) in track_ids.iter().enumerate() {
            let track = blackbird_state::Track {
                id: TrackId((*id).into()),
                title: format!("Track {id}").into(),
                artist: None,
                track: Some(i as u32 + 1),
                year: None,
                _genre: None,
                duration: Some(180),
                disc_number: None,
                album_id: Some(blackbird_state::AlbumId("a1".into())),
                starred: false,
                play_count: Some(1),
                replay_gain: None,
            };
            raw.tracks.insert(track.id.clone(), track);
        }
        raw.build()
    }

    #[test]
    fn swap_keeps_the_playing_track_after_its_removal() {
        let dir = test_dir("swap-while-playing");
        let old_server = MockServer::spawn(&["t1", "t2"]);
        warm_cache(&old_server, &dir);

        // The track playing from the cached library is gone from the server.
        let new_server = MockServer::spawn(&["t1"]);
        let mut h = harness(&new_server.base_url, &dir);
        h.run.cache = Some(LibraryCache::new(&dir, &old_server.base_url, "user"));
        h.state.write().unwrap().current_track_and_position = Some(crate::TrackAndPosition {
            track_id: TrackId("t2".into()),
            position: Duration::from_secs(10),
        });
        block_on(h.run.run(None));

        let st = h.state.read().unwrap();
        assert_eq!(st.library.track_ids, [TrackId("t1".into())]);
        // Its details are kept for the now-playing display...
        assert!(st.library.track_map.contains_key(&TrackId("t2".into())));
        // ...and the queue is still anchored on it.
        assert_eq!(
            st.queue.ordered_tracks.get(st.queue.current_index),
            Some(&TrackId("t2".into()))
        );
    }

    #[test]
    fn stale_apply_leaves_the_library_alone() {
        let dir = test_dir("stale-apply");
        let h = harness(&unreachable_base_url(), &dir);
        h.run.generation.current.fetch_add(1, Ordering::SeqCst);

        assert!(!block_on(h.run.apply(fetched(&["t1"]))));

        assert!(!h.state.read().unwrap().library.has_loaded_all_tracks);
        assert_eq!(h.populated_rx.try_iter().count(), 0);
        assert!(!h.library_replaced.load(Ordering::SeqCst));
    }

    #[test]
    fn apply_keeps_edits_made_during_the_refresh() {
        let dir = test_dir("edits-during-refresh");
        let h = harness(&unreachable_base_url(), &dir);
        assert!(block_on(h.run.apply(fetched(&["t1", "t2"]))));

        {
            let mut st = h.state.write().unwrap();
            st.library.begin_refresh();
            st.library.set_track_starred(&TrackId("t1".into()), true);
        }
        // The refresh read the server before the star landed.
        assert!(block_on(h.run.apply(fetched(&["t1", "t2"]))));

        let st = h.state.read().unwrap();
        assert!(st.library.track_map[&TrackId("t1".into())].starred);
        assert!(!st.library.track_map[&TrackId("t2".into())].starred);
    }

    #[test]
    fn unreachable_server_cancels_a_pending_play() {
        let dir = test_dir("offline-pending-play");
        let server = MockServer::spawn(&["t1", "t2"]);
        warm_cache(&server, &dir);

        let mut h = harness(&unreachable_base_url(), &dir);
        h.run.cache = Some(LibraryCache::new(&dir, &server.base_url, "user"));
        {
            // As if the user pressed play on the cached library before the
            // connection failed.
            let mut st = h.state.write().unwrap();
            st.started_loading_track = Some(std::time::Instant::now());
            st.queue.current_target = Some(TrackId("t1".into()));
            st.queue.current_target_request_id = Some(99);
        }
        block_on(h.run.run(None));

        let st = h.state.read().unwrap();
        assert!(st.server_unreachable.is_some());
        assert!(st.started_loading_track.is_none());
        assert!(st.queue.current_target.is_none());
        assert!(matches!(
            st.error,
            Some(AppStateError::LibraryRefreshFailed { .. })
        ));
    }
}
