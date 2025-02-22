use std::{collections::HashMap, ops::Range};

use serde::{Deserialize, Serialize};

use blackbird_subsonic as bs;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    base_url: String,
    username: String,
    password: String,
}

fn main() {
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
    album_id_to_idx: HashMap<String, usize>,

    error: Option<String>,
}
impl App {
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
        client_thread.request(ClientThreadRequest::FetchAlbums);
        App {
            client_thread,
            albums: vec![],
            album_id_to_idx: HashMap::new(),

            error: None,
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        for response in self.client_thread.recv_iter() {
            match response {
                ClientThreadResponse::Albums(albums) => {
                    self.albums = albums.into_iter().map(|a| a.into()).collect();
                    self.albums.sort();
                    self.album_id_to_idx = self
                        .albums
                        .iter()
                        .enumerate()
                        .map(|(i, a)| (a.id.clone(), i))
                        .collect();
                }
                ClientThreadResponse::Error(error) => self.error = Some(error),
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
    }
}

struct ClientThread {
    _thread: std::thread::JoinHandle<()>,
    request_tx: std::sync::mpsc::Sender<ClientThreadRequest>,
    response_rx: std::sync::mpsc::Receiver<ClientThreadResponse>,
}
enum ClientThreadRequest {
    FetchAlbums,
}
enum ClientThreadResponse {
    Albums(Vec<bs::AlbumID3>),
    Error(String),
}
impl ClientThread {
    fn new(client: bs::Client) -> Self {
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let (response_tx, response_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            loop {
                fn handle_result<T, E, F>(result: Result<T, E>, f: F) -> ClientThreadResponse
                where
                    E: std::fmt::Display,
                    F: FnOnce(T) -> ClientThreadResponse,
                {
                    match result {
                        Ok(value) => f(value),
                        Err(e) => ClientThreadResponse::Error(e.to_string()),
                    }
                }

                let request = request_rx.recv().unwrap();
                match request {
                    ClientThreadRequest::FetchAlbums => {
                        let albums = runtime.block_on(fetch_all_albums(&client));
                        response_tx
                            .send(handle_result(albums, ClientThreadResponse::Albums))
                            .unwrap();
                    }
                }
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

#[derive(Debug)]
/// An album, as `blackbird` cares about it
pub struct Album {
    /// The album ID
    pub id: String,
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
    pub songs: Option<Vec<String>>,
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
            id: album.id,
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
                        ui.label(song);
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
