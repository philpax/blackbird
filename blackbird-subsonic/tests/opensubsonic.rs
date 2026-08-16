//! End-to-end tests for the OpenSubsonic client additions, exercised against
//! the [`mock_server`] harness.
#![cfg(feature = "opensubsonic")]

mod common;
mod mock_server;

use blackbird_subsonic::{ClientError, OpenSubsonicExtension};
use common::block_on;
use mock_server::MockServer;

/// Parses `getOpenSubsonicExtensions` responses into the extension list.
#[test]
fn test_get_open_subsonic_extensions_parses() {
    let server = MockServer::spawn();
    server.respond(
        "getOpenSubsonicExtensions",
        r#""openSubsonicExtensions":{"extension":[{"name":"sonicSimilarity","version":"1.0.0"},{"name":"lyrics","version":"1.2.0"}]}"#,
    );

    let extensions = block_on(server.client().get_open_subsonic_extensions()).unwrap();

    assert_eq!(
        extensions,
        vec![
            OpenSubsonicExtension {
                name: "sonicSimilarity".to_string(),
                version: "1.0.0".to_string(),
            },
            OpenSubsonicExtension {
                name: "lyrics".to_string(),
                version: "1.2.0".to_string(),
            },
        ]
    );
    assert_eq!(server.hit_endpoints(), vec!["getOpenSubsonicExtensions"]);
}

/// A server that returns an empty extension list yields an empty vec.
#[test]
fn test_get_open_subsonic_extensions_empty() {
    let server = MockServer::spawn();
    server.respond(
        "getOpenSubsonicExtensions",
        r#""openSubsonicExtensions":{"extension":[]}"#,
    );

    let extensions = block_on(server.client().get_open_subsonic_extensions()).unwrap();
    assert!(extensions.is_empty());
}

/// A server that omits the `extension` field entirely also yields an empty vec.
#[test]
fn test_get_open_subsonic_extensions_missing_field_ok() {
    let server = MockServer::spawn();
    server.respond(
        "getOpenSubsonicExtensions",
        r#""openSubsonicExtensions":{}"#,
    );

    let extensions = block_on(server.client().get_open_subsonic_extensions()).unwrap();
    assert!(extensions.is_empty());
}

/// Parses `getSimilarSongs2` responses.
#[test]
fn test_get_similar_songs2_parses() {
    let server = MockServer::spawn();
    server.respond(
        "getSimilarSongs2",
        r#""similarSongs2":{"song":[{"id":"1","isDir":false,"title":"First"},{"id":"2","isDir":false,"title":"Second"}]}"#,
    );

    let songs = block_on(server.client().get_similar_songs2("seed-id", Some(10))).unwrap();

    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].id, "1");
    assert_eq!(songs[0].title, "First");
    assert_eq!(songs[1].id, "2");
    assert_eq!(songs[1].title, "Second");
}

/// `getSonicSimilarTracks` is only invoked when the server advertises the
/// `sonicSimilarity` extension; this pin's the documented response shape.
#[test]
fn test_get_sonic_similar_tracks_parses() {
    let server = MockServer::spawn();
    server.respond(
        "getSonicSimilarTracks",
        r#""sonicSimilarTracks":{"track":[{"id":"a","isDir":false,"title":"Alpha","duration":180,"artist":"Some Artist"}]}"#,
    );

    let tracks = block_on(server.client().get_sonic_similar_tracks("seed-id", Some(5))).unwrap();

    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].id, "a");
    assert_eq!(tracks[0].title, "Alpha");
    assert_eq!(tracks[0].artist.as_deref(), Some("Some Artist"));
}

/// `count` is included in the query string when provided.
#[test]
fn test_similar_songs_count_parameter() {
    let server = MockServer::spawn();
    server.respond("getSimilarSongs2", r#""similarSongs2":{"song":[]}"#);

    let _ = block_on(server.client().get_similar_songs2("seed-id", Some(17))).unwrap();

    let (endpoint, query) = server.requests.lock().unwrap().first().unwrap().clone();
    assert_eq!(endpoint, "getSimilarSongs2");
    assert!(query.contains("id=seed-id"), "query: {query}");
    assert!(query.contains("count=17"), "query: {query}");
}

/// A server error surfaces as `ClientError::SubsonicError` and callers can
/// degrade gracefully (no panic, no partial state).
#[test]
fn test_similar_songs_error_is_subsonic_error() {
    let server = MockServer::spawn();
    // No canned response: the harness returns a "failed" Subsonic response.
    let result = block_on(server.client().get_similar_songs2("seed-id", None));
    // The request still records even in the error case.
    assert_eq!(server.hit_endpoints(), vec!["getSimilarSongs2"]);

    assert!(
        matches!(result, Err(ClientError::SubsonicError { .. })),
        "expected a SubsonicError, got: {result:?}"
    );
}
