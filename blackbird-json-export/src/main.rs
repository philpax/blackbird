use std::path::PathBuf;

use anyhow::Context as _;

use blackbird_json_export_types::{OutputGroup, OutputSong};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    server: blackbird_shared::config::Server,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = toml::from_str::<Config>(&std::fs::read_to_string("config.toml")?)?;
    let output_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .context("Output path is required")?;

    let client = blackbird_state::bs::Client::new(
        config.server.base_url,
        config.server.username,
        config.server.password,
        "blackbird-json-export",
    );

    let fetched = blackbird_state::fetch_all(&client, |batch_count, total_count| {
        println!("Fetched {batch_count} tracks, total {total_count} tracks");
    })
    .await?;

    let mut output = vec![];
    for group in fetched.groups {
        output.push(OutputGroup {
            artist: group.artist.clone(),
            album: group.album.clone(),
            year: group.year,
            duration: group.duration,
            songs: group
                .songs
                .iter()
                .map(|song_id| {
                    let song = fetched.songs.get(song_id).unwrap();
                    OutputSong {
                        title: song.title.clone(),
                        artist: song.artist.clone(),
                        track: song.track,
                        year: song.year,
                        duration: song.duration,
                        disc_number: song.disc_number,
                    }
                })
                .collect(),
        });
    }

    std::fs::write(
        &output_path,
        serde_json::to_string_pretty(&output)
            .with_context(|| format!("Failed to write to {output_path:?}"))?,
    )?;

    Ok(())
}
