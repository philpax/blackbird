//! Regression tests for response parsing against servers that add extra
//! envelope fields (Navidrome adds `type`, `serverVersion`, `openSubsonic`).

mod mock_server;

use mock_server::MockServer;

/// Runs an async client future to completion on a current-thread runtime.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

/// `ping` must succeed even when the `subsonic-response` envelope carries
/// extra top-level fields (Navidrome-style), because `ping` uses the unit type
/// for its body.
#[test]
fn ping_with_extra_server_fields_succeeds() {
    let server = MockServer::spawn();
    server.respond(
        "ping",
        r#""type":"navidrome","serverVersion":"0.53.0","openSubsonic":true"#,
    );

    let result = block_on(server.client().ping());
    assert!(
        result.is_ok(),
        "ping failed against an envelope with extra fields: {result:?}"
    );
}

/// A plain ping (no extra fields) must also succeed.
#[test]
fn ping_with_plain_envelope_succeeds() {
    let server = MockServer::spawn();
    server.respond("ping", "");

    let result = block_on(server.client().ping());
    assert!(result.is_ok(), "ping failed: {result:?}");
}
