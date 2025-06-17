use std::path::{Path, PathBuf};

use crate::common::{Albums, Ndjson as _, Tracks};

mod common;
mod spotify;

fn main() -> anyhow::Result<()> {
    let spotify_data_path = std::env::args().nth(1).map(PathBuf::from);
    let output_dir = Path::new("spotcheck-output");
    let albums_path = output_dir.join("albums.ndjson");
    let tracks_path = output_dir.join("tracks.ndjson");

    let (_albums, _tracks) = if let Some(spotify_data_path) = spotify_data_path {
        let (albums, tracks) = spotify::parse_and_collate_data(&spotify_data_path)?;
        albums.save(&albums_path)?;
        tracks.save(&tracks_path)?;
        (albums, tracks)
    } else {
        (Albums::load(&albums_path)?, Tracks::load(&tracks_path)?)
    };

    Ok(())
}
