use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use blackbird_subsonic as bs;

mod style;
mod util;

mod config;
use config::*;

mod album;
use album::*;

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
    client_thread: ClientThread,

    albums: Vec<Album>,
    album_id_to_idx: HashMap<AlbumId, usize>,
    pending_album_request_ids: HashSet<AlbumId>,

    cover_art_cache: HashMap<String, (egui::ImageSource<'static>, std::time::Instant)>,
    pending_cover_art_requests: HashSet<String>,

    error: Option<String>,
}
impl App {
    const MAX_CONCURRENT_ALBUM_REQUESTS: usize = 10;
    const MAX_CONCURRENT_COVER_ART_REQUESTS: usize = 10;
    const MAX_COVER_ART_CACHE_SIZE: usize = 32;

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        config.save();

        let client = bs::Client::new(
            config.general.base_url.clone(),
            config.general.username.clone(),
            config.general.password.clone(),
            "blackbird".to_string(),
        );

        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.style_mut(|style| {
            style.visuals.panel_fill = config.style.background();
            style.visuals.override_text_color = Some(config.style.text());
        });

        egui_extras::install_image_loaders(&cc.egui_ctx);

        let client_thread = ClientThread::new(client);
        client_thread.request(ClientThreadRequest::Ping);
        client_thread.request(ClientThreadRequest::FetchAlbums);
        App {
            config,
            last_config_update: std::time::Instant::now(),
            client_thread,

            albums: vec![],
            album_id_to_idx: HashMap::new(),
            pending_album_request_ids: HashSet::new(),

            cover_art_cache: HashMap::new(),
            pending_cover_art_requests: HashSet::new(),

            error: None,
        }
    }

    fn fetch_album(&mut self, album_id: AlbumId) {
        if self.pending_album_request_ids.len() >= Self::MAX_CONCURRENT_ALBUM_REQUESTS {
            return;
        }
        self.client_thread
            .request(ClientThreadRequest::FetchAlbum(album_id.clone()));
        self.pending_album_request_ids.insert(album_id);
    }

    fn does_album_need_fetching(&self, album_id: &AlbumId) -> bool {
        !self.pending_album_request_ids.contains(album_id)
            && self.albums[self.album_id_to_idx[album_id]].songs.is_none()
    }

    fn fetch_cover_art(&mut self, cover_art_id: String) {
        if self.pending_cover_art_requests.len() >= Self::MAX_CONCURRENT_COVER_ART_REQUESTS {
            return;
        }
        self.client_thread
            .request(ClientThreadRequest::FetchCoverArt(cover_art_id.clone()));
        self.pending_cover_art_requests.insert(cover_art_id);
    }

    fn does_cover_art_need_fetching(&self, cover_art_id: &String) -> bool {
        !self.pending_cover_art_requests.contains(cover_art_id)
            && !self.cover_art_cache.contains_key(cover_art_id)
    }
    fn process_responses(&mut self) {
        for response in self.client_thread.recv_iter() {
            match response {
                ClientThreadResponse::Ping => {
                    tracing::info!("successfully pinged Subsonic server");
                }
                ClientThreadResponse::Albums(albums) => {
                    tracing::info!("fetched {} albums", albums.len());
                    self.albums = albums.into_iter().map(|a| a.into()).collect();
                    self.albums.sort();
                    self.album_id_to_idx = self
                        .albums
                        .iter()
                        .enumerate()
                        .map(|(i, a)| (a.id.clone(), i))
                        .collect();
                }
                ClientThreadResponse::Album(album) => {
                    tracing::trace!(
                        "fetched album {} - {} ({})",
                        album.album.artist.as_deref().unwrap_or("Unknown Artist"),
                        album.album.name,
                        album.album.id
                    );
                    let id = AlbumId(album.album.id.clone());
                    let idx = self
                        .album_id_to_idx
                        .get(&id)
                        .unwrap_or_else(|| panic!("Album ID `{id}` not found in album list"));
                    self.albums[*idx].songs =
                        Some(album.song.into_iter().map(|s| s.into()).collect());
                    self.pending_album_request_ids.remove(&id);
                }
                ClientThreadResponse::Error(error) => {
                    tracing::error!("client thread error: {error}");
                    self.error = Some(error)
                }
                ClientThreadResponse::CoverArt(cover_art_id, cover_art) => {
                    tracing::info!("fetched cover art {cover_art_id}");
                    if self.cover_art_cache.len() == Self::MAX_COVER_ART_CACHE_SIZE {
                        let oldest_id = self
                            .cover_art_cache
                            .iter()
                            .min_by_key(|(_, (_, time))| *time)
                            .map(|(id, _)| id.clone())
                            .expect("cover art cache not empty");

                        self.cover_art_cache.remove(&oldest_id);
                        tracing::info!("evicted cover art {oldest_id}");
                    }

                    let uri = format!("bytes://{cover_art_id}");
                    self.pending_cover_art_requests.remove(&cover_art_id);
                    self.cover_art_cache.insert(
                        cover_art_id,
                        (
                            egui::ImageSource::Bytes {
                                uri: uri.into(),
                                bytes: cover_art.into(),
                            },
                            std::time::Instant::now(),
                        ),
                    );
                }
            }
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
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.process_responses();
        self.poll_for_config_updates();

        let mut fetch_set = HashSet::new();
        let mut cover_art_fetch_set = HashSet::new();

        if let Some(error) = &self.error {
            let mut open = true;
            egui::Window::new("Error").open(&mut open).show(ctx, |ui| {
                ui.label(error);
            });
            if !open {
                self.error = None;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let row_height = ui.text_style_height(&egui::TextStyle::Body);
            let album_margin_bottom_row_count = 1;
            let num_rows = self.albums.iter().map(|album| album.line_count()).sum();

            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                row_height,
                num_rows,
                |ui, visible_row_range| {
                    let mut current_row = 0;
                    for album in &self.albums {
                        let album_lines = album.line_count();
                        let album_range = current_row..(current_row + album_lines);

                        // If this album starts after the visible range, we can break out.
                        if album_range.start >= visible_row_range.end {
                            break;
                        }

                        // If this album is completely above the visible range, skip it.
                        if album_range.end <= visible_row_range.start {
                            current_row += album_lines;
                            continue;
                        }

                        if self.does_album_need_fetching(&album.id) {
                            fetch_set.insert(album.id.clone());
                        }

                        if let Some(cover_art_id) = &album.cover_art_id {
                            if self.does_cover_art_need_fetching(cover_art_id) {
                                cover_art_fetch_set.insert(cover_art_id.clone());
                            }
                        }

                        // Compute the visible portion of the album's rows, rebased to the album.
                        let local_start = visible_row_range.start.saturating_sub(current_row);
                        let local_end = (visible_row_range.end - current_row).min(album_lines);
                        let local_visible_range = local_start..local_end;

                        album.ui(
                            ui,
                            &self.config.style,
                            local_visible_range,
                            album.cover_art_id.as_deref().and_then(|id| {
                                self.cover_art_cache.get(id).map(|(img, _)| img).cloned()
                            }),
                        );

                        ui.add_space(row_height * album_margin_bottom_row_count as f32);

                        current_row += album_lines + album_margin_bottom_row_count;
                    }
                },
            );
        });

        // pad fetch_set up to MAX_CONCURRENT_ALBUM_REQUESTS
        // by adding album IDs in order until we have enough
        for album in &self.albums {
            if fetch_set.len() >= Self::MAX_CONCURRENT_ALBUM_REQUESTS {
                break;
            }
            if self.does_album_need_fetching(&album.id) {
                fetch_set.insert(album.id.clone());
            }
        }

        for album_id in fetch_set {
            self.fetch_album(album_id);
        }

        for cover_art_id in cover_art_fetch_set {
            self.fetch_cover_art(cover_art_id);
        }

        ctx.request_repaint_after_secs(0.05);
    }
}

struct ClientThread {
    _thread: std::thread::JoinHandle<()>,
    request_tx: std::sync::mpsc::Sender<ClientThreadRequest>,
    response_rx: std::sync::mpsc::Receiver<ClientThreadResponse>,
}
enum ClientThreadRequest {
    Ping,
    FetchAlbums,
    FetchAlbum(AlbumId),
    FetchCoverArt(String),
}
#[allow(clippy::large_enum_variant /* this is not that important */)]
enum ClientThreadResponse {
    Ping,
    Albums(Vec<bs::AlbumID3>),
    Album(bs::AlbumWithSongsID3),
    CoverArt(String, Vec<u8>),
    Error(String),
}
impl ClientThread {
    fn new(client: bs::Client) -> Self {
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let (response_tx, response_rx) = std::sync::mpsc::channel();
        let client = Arc::new(client);
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();

            fn send_result<T, E, F>(
                response_tx: std::sync::mpsc::Sender<ClientThreadResponse>,
                result: Result<T, E>,
                f: F,
            ) where
                E: std::fmt::Display,
                F: FnOnce(T) -> ClientThreadResponse,
            {
                let response = match result {
                    Ok(value) => f(value),
                    Err(e) => ClientThreadResponse::Error(e.to_string()),
                };
                response_tx.send(response).unwrap();
            }

            loop {
                let request = request_rx.recv().unwrap();
                let response_tx = response_tx.clone();
                let client = client.clone();

                runtime.spawn(async move {
                    match request {
                        ClientThreadRequest::Ping => {
                            send_result(response_tx, client.ping().await, |_| {
                                ClientThreadResponse::Ping
                            });
                        }
                        ClientThreadRequest::FetchAlbums => {
                            let albums = album::fetch_all_raw(&client).await;
                            send_result(response_tx, albums, ClientThreadResponse::Albums);
                        }
                        ClientThreadRequest::FetchAlbum(album_id) => {
                            let album = client.get_album_with_songs(album_id.0).await;
                            send_result(response_tx, album, ClientThreadResponse::Album);
                        }
                        ClientThreadRequest::FetchCoverArt(cover_art_id) => {
                            let cover_art = client.get_cover_art(&cover_art_id).await;
                            send_result(response_tx, cover_art, |cover_art| {
                                ClientThreadResponse::CoverArt(cover_art_id, cover_art)
                            });
                        }
                    }
                });
            }
        });

        ClientThread {
            _thread: thread,
            request_tx,
            response_rx,
        }
    }

    fn request(&self, request: ClientThreadRequest) {
        self.request_tx.send(request).unwrap();
    }

    fn recv_iter(&self) -> impl Iterator<Item = ClientThreadResponse> + use<'_> {
        self.response_rx.try_iter()
    }
}
