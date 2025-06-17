use std::path::{Path, PathBuf};

use crate::common::{Albums, Ndjson as _, Tracks};

mod common;
mod spotify;

fn main() -> anyhow::Result<()> {
    let spotify_data_path = std::env::args().nth(1).map(PathBuf::from);
    let output_dir = Path::new("spotcheck-output");
    let albums_path = output_dir.join("albums.ndjson");
    let tracks_path = output_dir.join("tracks.ndjson");

    let (albums, _tracks) = if let Some(spotify_data_path) = spotify_data_path {
        let (albums, tracks) = spotify::parse_and_collate_data(&spotify_data_path)?;
        albums.save(&albums_path)?;
        tracks.save(&tracks_path)?;
        (albums, tracks)
    } else {
        (Albums::load(&albums_path)?, Tracks::load(&tracks_path)?)
    };

    let mut albums_vec = albums.0.values().collect::<Vec<_>>();
    albums_vec.sort_by_key(|album| -(album.play_count as i32));
    for (i, album) in albums_vec[..10].iter().enumerate() {
        println!(
            "{}: {} - {} ({} plays)",
            i + 1,
            album.album_id.artist,
            album.album_id.album,
            album.play_count
        );
    }

    Ok(())
}
