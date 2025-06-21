use std::{
    io::Write,
    path::{Path, PathBuf},
};

use blackbird_subsonic as bs;
use serde::Deserialize;

use crate::common::{Albums, Ndjson as _, Tracks};

mod common;
mod spotify;

#[derive(Deserialize)]
pub struct Config {
    server: blackbird_shared::config::Server,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    tracing::info!("Loading configuration from config.toml...");
    let config = toml::from_str::<Config>(&std::fs::read_to_string("config.toml")?)?;

    let spotify_data_path = std::env::args().nth(1).map(PathBuf::from);
    let output_dir = Path::new("spotcheck-output");
    let albums_path = output_dir.join("albums.ndjson");
    let tracks_path = output_dir.join("tracks.ndjson");

    let (albums, _tracks) = if let Some(spotify_data_path) = spotify_data_path {
        tracing::info!("Parsing Spotify data from: {:?}", spotify_data_path);
        let (albums, tracks) =
            tokio::task::block_in_place(|| spotify::parse_and_collate_data(&spotify_data_path))?;
        tracing::info!("Saving parsed data to output directory...");
        albums.save(&albums_path)?;
        tracks.save(&tracks_path)?;
        (albums, tracks)
    } else {
        tracing::info!("Loading existing parsed data from output directory...");
        (Albums::load(&albums_path)?, Tracks::load(&tracks_path)?)
    };

    tracing::info!("Sorting albums by play count...");
    let mut albums_vec = albums.0.into_values().collect::<Vec<_>>();
    albums_vec.sort_by_key(|album| -(album.play_count as i32));

    tracing::info!("Generating top albums report...");
    let mut output = std::fs::File::create(output_dir.join("top-albums.md"))?;
    writeln!(output, "# Top Albums")?;
    for (i, album) in albums_vec.iter().enumerate() {
        writeln!(
            output,
            "{}: {} - {} ({} plays)",
            i + 1,
            album.album_id.artist,
            album.album_id.album,
            album.play_count
        )?;
    }

    tracing::info!("Connecting to Subsonic server...");
    let client = blackbird_subsonic::Client::new(
        config.server.base_url,
        config.server.username,
        config.server.password,
        "blackbird-spotcheck",
    );

    tracing::info!("Fetching all albums from Subsonic...");
    let all_albums = fetch_all_albums(&client).await?;
    tracing::info!("Found {} albums in Subsonic", all_albums.len());

    // Create a more efficient lookup structure
    // 1. Exact matches for fast lookup
    let mut exact_album_matches: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // 2. Normalized artist -> albums mapping for fuzzy matching
    let mut normalized_artist_albums: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();

    for album in &all_albums {
        let artist = album.artist.as_deref().unwrap_or_default();
        let album_name = &album.name;

        // Store exact match for fast lookup
        let exact_key = format!("{} - {}", artist.to_lowercase(), album_name.to_lowercase());
        exact_album_matches.insert(exact_key);

        // Store normalized version for fuzzy matching
        let normalized_artist = normalize_artist_name(artist);
        normalized_artist_albums
            .entry(normalized_artist)
            .or_default()
            .push((artist.to_string(), album_name.to_string()));
    }

    // Pre-compute normalized Subsonic artist names for faster lookup
    let normalized_subsonic_artists: Vec<String> =
        normalized_artist_albums.keys().cloned().collect();

    tracing::info!("Generating missing albums report...");
    let mut output = std::fs::File::create(output_dir.join("top-missing-albums.md"))?;
    writeln!(output, "# Top Missing Albums")?;

    // Also create a report for found albums
    let mut found_output = std::fs::File::create(output_dir.join("top-found-albums.md"))?;
    writeln!(found_output, "# Top Found Albums")?;

    // Process albums in parallel chunks
    let chunk_size = 100;
    let mut all_results = Vec::new();

    for (chunk_idx, chunk) in albums_vec.chunks(chunk_size).enumerate() {
        tracing::info!(
            "Processing chunk {} of {}",
            chunk_idx + 1,
            albums_vec.len().div_ceil(chunk_size)
        );

        let tasks: Vec<_> = chunk
            .iter()
            .enumerate()
            .map(|(i, album)| {
                let exact_album_matches = exact_album_matches.clone();
                let normalized_artist_albums = normalized_artist_albums.clone();
                let normalized_subsonic_artists = normalized_subsonic_artists.clone();
                let album = album.clone();
                let global_idx = chunk_idx * chunk_size + i;

                tokio::task::spawn_blocking(move || {
                    let spotify_artist = &album.album_id.artist;
                    let spotify_album = &album.album_id.album;

                    // First try exact match (fastest)
                    let exact_key = format!(
                        "{} - {}",
                        spotify_artist.to_lowercase(),
                        spotify_album.to_lowercase()
                    );
                    if exact_album_matches.contains(&exact_key) {
                        return (global_idx, album, Some("exact"));
                    }

                    // If no exact match, try fuzzy matching (CPU-intensive work)
                    let normalized_spotify_artist = normalize_artist_name(spotify_artist);

                    // Look for similar artists
                    for subsonic_artist in &normalized_subsonic_artists {
                        if fuzzy_match(&normalized_spotify_artist, subsonic_artist) > 0.8 {
                            // Found a similar artist, now check their albums
                            if let Some(albums) = normalized_artist_albums.get(subsonic_artist) {
                                for (_, subsonic_album_name) in albums {
                                    let album_similarity =
                                        fuzzy_match(spotify_album, subsonic_album_name);
                                    if album_similarity > 0.8 {
                                        return (global_idx, album, Some("fuzzy"));
                                    }
                                }
                            }
                        }
                    }

                    (global_idx, album, None)
                })
            })
            .collect();

        let chunk_results = futures::future::join_all(tasks).await;
        for result in chunk_results {
            all_results.push(result?);
        }
    }

    // Sort results by original index to maintain play count order
    all_results.sort_by_key(|(idx, _, _)| *idx);

    // Write results to files
    let mut found_counter = 0;
    let mut missing_counter = 0;

    for (_, album, match_type) in all_results {
        match match_type {
            Some(match_kind) => {
                writeln!(
                    found_output,
                    "{}: {} - {} ({} plays) [{}]",
                    found_counter + 1,
                    album.album_id.artist,
                    album.album_id.album,
                    album.play_count,
                    match_kind
                )?;
                found_counter += 1;
            }
            None => {
                writeln!(
                    output,
                    "{}: {} - {} ({} plays)",
                    missing_counter + 1,
                    album.album_id.artist,
                    album.album_id.album,
                    album.play_count
                )?;
                missing_counter += 1;
            }
        }
    }

    tracing::info!(
        "Found {} albums in Subsonic, {} missing",
        found_counter,
        missing_counter
    );

    tracing::info!("blackbird-spotcheck completed successfully!");
    Ok(())
}

async fn fetch_all_albums(client: &bs::Client) -> bs::ClientResult<Vec<bs::AlbumID3>> {
    let mut all_albums = vec![];
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

fn fuzzy_match(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    // Exact match gets highest score
    if a_lower == b_lower {
        return 1.0;
    }

    // Check if one string contains the other
    if a_lower.contains(&b_lower) || b_lower.contains(&a_lower) {
        return 0.8;
    }

    // Calculate Jaro-Winkler similarity
    let jaro = jaro_similarity(&a_lower, &b_lower);
    let winkler = winkler_similarity(&a_lower, &b_lower, jaro);

    // Also check for word-level matches
    let word_similarity = word_based_similarity(&a_lower, &b_lower);

    // Return the maximum of the different similarity measures
    winkler.max(word_similarity)
}

fn jaro_similarity(s1: &str, s2: &str) -> f64 {
    if s1 == s2 {
        return 1.0;
    }

    let len1 = s1.chars().count();
    let len2 = s2.chars().count();

    if len1 == 0 || len2 == 0 {
        return 0.0;
    }

    let match_distance = (len1.max(len2) / 2) - 1;
    let mut s1_matches = vec![false; len1];
    let mut s2_matches = vec![false; len2];

    let mut matches = 0;

    for (i, c1) in s1.chars().enumerate() {
        let start = i.saturating_sub(match_distance);
        let end = (i + match_distance + 1).min(len2);

        #[allow(clippy::needless_range_loop)]
        for j in start..end {
            if !s2_matches[j] && c1 == s2.chars().nth(j).unwrap() {
                s1_matches[i] = true;
                s2_matches[j] = true;
                matches += 1;
                break;
            }
        }
    }

    if matches == 0 {
        return 0.0;
    }

    let mut transpositions = 0;
    let mut k = 0;

    for (i, matched) in s1_matches.iter().enumerate() {
        if *matched {
            while !s2_matches[k] {
                k += 1;
            }
            if s1.chars().nth(i).unwrap() != s2.chars().nth(k).unwrap() {
                transpositions += 1;
            }
            k += 1;
        }
    }

    let m = matches as f64;
    let t = (transpositions / 2) as f64;

    (m / len1 as f64 + m / len2 as f64 + (m - t) / m) / 3.0
}

fn winkler_similarity(s1: &str, s2: &str, jaro: f64) -> f64 {
    if jaro < 0.7 {
        return jaro;
    }

    let prefix_length = s1
        .chars()
        .zip(s2.chars())
        .take_while(|(a, b)| a == b)
        .count()
        .min(4);

    jaro + 0.1 * prefix_length as f64 * (1.0 - jaro)
}

fn word_based_similarity(s1: &str, s2: &str) -> f64 {
    let words1: std::collections::HashSet<_> = s1.split_whitespace().collect();
    let words2: std::collections::HashSet<_> = s2.split_whitespace().collect();

    if words1.is_empty() && words2.is_empty() {
        return 1.0;
    }

    if words1.is_empty() || words2.is_empty() {
        return 0.0;
    }

    let intersection = words1.intersection(&words2).count();
    let union = words1.union(&words2).count();

    intersection as f64 / union as f64
}

fn normalize_artist_name(artist: &str) -> String {
    artist
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
