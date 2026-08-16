//! Regression tests for response parsing against servers that add extra
//! envelope fields (Navidrome adds `type`, `serverVersion`, `openSubsonic`).

mod common;
mod mock_server;

use common::block_on;
use mock_server::MockServer;

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
