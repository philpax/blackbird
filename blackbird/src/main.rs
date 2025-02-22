use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use blackbird_subsonic as bs;

mod util;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    base_url: String,
    username: String,
    password: String,
}

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
    client_thread: ClientThread,

    albums: Vec<Album>,
    album_id_to_idx: HashMap<AlbumId, usize>,
    pending_album_request_ids: HashSet<AlbumId>,

    error: Option<String>,
}
impl App {
    const MAX_CONCURRENT_ALBUM_REQUESTS: usize = 10;

    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = toml::from_str::<Config>(
            &std::fs::read_to_string("config.toml").expect("Failed to read config.toml"),
        )
        .expect("Failed to parse config.toml");
        let client = bs::Client::new(
            config.base_url,
            config.username,
            config.password,
            "blackbird".to_string(),
        );

        let client_thread = ClientThread::new(client);
        client_thread.request(ClientThreadRequest::Ping);
        client_thread.request(ClientThreadRequest::FetchAlbums);
        App {
            client_thread,

            albums: vec![],
            album_id_to_idx: HashMap::new(),

            pending_album_request_ids: HashSet::new(),

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
}
impl eframe::App for App {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
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
                    tracing::info!(
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
            }
        }

        if let Some(error) = &self.error {
            let mut open = true;
            egui::Window::new("Error").open(&mut open).show(ctx, |ui| {
                ui.label(error);
            });
            if !open {
                self.error = None;
            }
        }

        let mut fetch_set = HashSet::new();

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

                        // Compute the visible portion of the album's rows, rebased to the album.
                        let local_start = visible_row_range.start.saturating_sub(current_row);
                        let local_end = (visible_row_range.end - current_row).min(album_lines);
                        let local_visible_range = local_start..local_end;

                        album.ui(ui, local_visible_range);

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
}
enum ClientThreadResponse {
    Ping,
    Albums(Vec<bs::AlbumID3>),
    Album(bs::AlbumWithSongsID3),
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
                            let albums = fetch_all_albums(&client).await;
                            send_result(response_tx, albums, ClientThreadResponse::Albums);
                        }
                        ClientThreadRequest::FetchAlbum(album_id) => {
                            let album = client.get_album_with_songs(album_id.0).await;
                            send_result(response_tx, album, ClientThreadResponse::Album);
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

async fn fetch_all_albums(client: &bs::Client) -> anyhow::Result<Vec<bs::AlbumID3>> {
    let mut all_albums = Vec::new();
    let mut offset = 0;
    loop {
        let albums = client
            .get_album_list_2(
                bs::AlbumListType::AlphabeticalByArtist,
                Some(500),
                Some(offset),
            )
            .await?;
        let album_count = albums.len();

        offset += album_count;
        all_albums.extend(albums);
        if album_count < 500 {
            break;
        }
    }
    Ok(all_albums)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AlbumId(pub String);
impl std::fmt::Display for AlbumId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
/// An album, as `blackbird` cares about it
pub struct Album {
    /// The album ID
    pub id: AlbumId,
    /// The album name
    pub name: String,
    /// The album artist name
    pub artist: String,
    /// The album artist ID
    pub artist_id: Option<String>,
    /// The album cover art ID
    pub cover_art: Option<String>,
    /// The number of songs in the album
    pub song_count: u32,
    /// The songs in the album
    pub songs: Option<Vec<Song>>,
    /// The total duration of the album in seconds
    pub duration: u32,
    /// The release year of the album
    pub year: Option<i32>,
    /// The genre of the album
    pub genre: Option<String>,
}
impl From<bs::AlbumID3> for Album {
    fn from(album: bs::AlbumID3) -> Self {
        Album {
            id: AlbumId(album.id),
            name: album.name,
            artist: album.artist.unwrap_or_else(|| "Unknown Artist".to_string()),
            artist_id: album.artist_id,
            cover_art: album.cover_art,
            song_count: album.song_count,
            songs: None,
            duration: album.duration,
            year: album.year,
            genre: album.genre,
        }
    }
}
impl PartialEq for Album {
    fn eq(&self, other: &Self) -> bool {
        (self.artist.as_str(), self.year, &self.name)
            == (other.artist.as_str(), other.year, &other.name)
    }
}
impl Eq for Album {}
impl PartialOrd for Album {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Album {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.artist.as_str(), self.year, &self.name).cmp(&(
            other.artist.as_str(),
            other.year,
            &other.name,
        ))
    }
}
impl Album {
    fn ui(&self, ui: &mut egui::Ui, row_range: Range<usize>) {
        // If the first row is visible, draw the artist.
        if row_range.contains(&0) {
            ui.label(&self.artist);
        }
        // If the second row is visible, draw the album title (including release year if available).
        if row_range.contains(&1) {
            let album_title = if let Some(year) = self.year {
                format!("{} ({})", self.name, year)
            } else {
                self.name.clone()
            };
            ui.label(album_title);
        }

        // The first two rows are for headers, so adjust the song row indices by subtracting 2.
        let song_start = row_range.start.saturating_sub(2);
        let song_end = row_range.end.saturating_sub(2);

        if song_start >= song_end {
            return;
        }
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 10,
                ..egui::Margin::ZERO
            })
            .show(ui, |ui| {
                if let Some(songs) = &self.songs {
                    // Clamp the song slice to the actual number of songs.
                    let end = song_end.min(songs.len());
                    for song in &songs[song_start..end] {
                        song.ui(ui, &self.artist);
                    }
                } else {
                    for _ in song_start..song_end {
                        ui.label("[loading...]");
                    }
                }
            });
    }
    fn line_count(&self) -> usize {
        let artist = 1;
        let album = 1;
        let songs = self
            .songs
            .as_ref()
            .map_or(self.song_count as usize, |songs| songs.len());
        artist + album + songs
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SongId(pub String);
impl std::fmt::Display for SongId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
/// A song, as `blackbird` cares about it
pub struct Song {
    /// The song ID
    pub id: SongId,
    /// The song title
    pub title: String,
    /// The song artist
    pub artist: Option<String>,
    /// The track number
    pub track: Option<u32>,
    /// The release year
    pub year: Option<i32>,
    /// The genre
    pub genre: Option<String>,
    /// The duration in seconds
    pub duration: Option<u32>,
    /// The disc number
    pub disc_number: Option<u32>,
    /// The album ID
    pub album_id: Option<AlbumId>,
}
impl From<bs::Child> for Song {
    fn from(child: bs::Child) -> Self {
        Song {
            id: SongId(child.id),
            title: child.title,
            artist: child.artist,
            track: child.track,
            year: child.year,
            genre: child.genre,
            duration: child.duration,
            disc_number: child.disc_number,
            album_id: child.album_id.map(AlbumId),
        }
    }
}
impl PartialEq for Song {
    fn eq(&self, other: &Self) -> bool {
        (self.year, self.disc_number, self.track) == (other.year, other.disc_number, other.track)
    }
}
impl Eq for Song {}
impl PartialOrd for Song {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Song {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.year, self.disc_number, self.track).cmp(&(other.year, other.disc_number, other.track))
    }
}
impl Song {
    pub fn ui(&self, ui: &mut egui::Ui, album_artist: &str) {
        let track = self.track.unwrap_or(0);
        let track_str = if let Some(disc_number) = self.disc_number {
            format!("{disc_number}.{track}")
        } else {
            track.to_string()
        };
        ui.horizontal(|ui| {
            // column 1 left aligned
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(&track_str);
                ui.add_space(4.0);
                ui.label(&self.title);
            });

            // column 2 right-aligned
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(util::seconds_to_hms_string(self.duration.unwrap_or(0)));
                if let Some(artist) = self
                    .artist
                    .as_ref()
                    .filter(|artist| *artist != album_artist)
                {
                    ui.label(artist);
                }
            });
        });
    }
}
