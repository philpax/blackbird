//! Lightweight HTTP mock server for exercising the Subsonic client request
//! paths against canned responses.
//!
//! The client has no HTTP mocking infrastructure, so this small harness spins
//! up a `TcpListener` that serves hand-crafted `subsonic-response` JSON. Each
//! test registers the responses to serve (keyed by endpoint with a
//! last-registered-wins map), points a [`Client`] at the listener, and
//! inspects both the parsed result and the recorded request path.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
};

use blackbird_subsonic::Client;

/// Serves canned JSON responses on a background thread.
pub struct MockServer {
    responses: Arc<Mutex<HashMap<String, String>>>,
    /// Endpoints hit, in order, with their full query strings.
    pub requests: Arc<Mutex<Vec<(String, String)>>>,
    pub base_url: String,
}

impl MockServer {
    /// Spawns a server that returns a Subsonic error for any unregistered endpoint.
    pub fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind test listener");
        let address = listener.local_addr().unwrap().to_string();
        let base_url = format!("http://{address}");
        let responses: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let requests: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));

        let responses_thread = responses.clone();
        let requests_thread = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    break;
                };
                let responses = responses_thread.clone();
                let requests = requests_thread.clone();
                std::thread::spawn(move || {
                    handle_connection(&mut stream, &responses, &requests);
                });
            }
        });

        Self {
            responses,
            requests,
            base_url,
        }
    }

    /// Registers the JSON body to serve for a given endpoint (e.g. `ping`).
    /// The response body must be the inner `subsonicResponse` object.
    pub fn respond(&self, endpoint: &str, subsonic_response_json: &str) {
        self.responses
            .lock()
            .unwrap()
            .insert(endpoint.to_string(), wrap_response(subsonic_response_json));
    }

    /// Returns a client pointed at this server.
    pub fn client(&self) -> Client {
        Client::new(
            self.base_url.clone(),
            "user".to_string(),
            "password".to_string(),
            "test".to_string(),
        )
    }

    /// Returns the endpoints that were hit, in order.
    #[allow(dead_code)]
    pub fn hit_endpoints(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(e, _)| e.clone())
            .collect()
    }
}

fn wrap_response(inner: &str) -> String {
    if inner.is_empty() {
        r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#.to_string()
    } else {
        format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{inner}}}}}"#)
    }
}

/// An error body used when no canned response exists for an endpoint.
fn default_error_body() -> String {
    r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":0,"message":"not stubbed"}}}"#.to_string()
}

fn handle_connection(
    stream: &mut TcpStream,
    responses: &Mutex<HashMap<String, String>>,
    requests: &Mutex<Vec<(String, String)>>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let Some(path) = request_line.split_whitespace().nth(1) else {
        return;
    };
    let path = path.trim_start_matches('/');
    // Client requests look like `/rest/<endpoint>?params`.
    let path = path.strip_prefix("rest/").unwrap_or(path);
    let (endpoint, query) = match path.split_once('?') {
        Some((e, q)) => (e.to_string(), q.to_string()),
        None => (path.to_string(), String::new()),
    };

    // Consume headers so the client doesn't get a broken pipe.
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
    }

    requests.lock().unwrap().push((endpoint.clone(), query));

    let body = responses
        .lock()
        .unwrap()
        .get(&endpoint)
        .cloned()
        .unwrap_or_else(default_error_body);

    let http_response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(http_response.as_bytes());
    let _ = stream.flush();
}
