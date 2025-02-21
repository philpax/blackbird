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
    albums: Vec<bs::AlbumID3>,

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

            error: None,
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        for response in self.client_thread.recv_iter() {
            match response {
                ClientThreadResponse::Albums(albums) => self.albums = albums,
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
            egui::ScrollArea::vertical().show(ui, |ui| {
                for album in &self.albums {
                    ui.label(album.artist.as_deref().unwrap_or("Unknown Artist"));
                    ui.label(if let Some(year) = album.year {
                        format!("{} ({})", album.name, year)
                    } else {
                        album.name.clone()
                    });
                    ui.add_space(10.0);
                }
            });
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
