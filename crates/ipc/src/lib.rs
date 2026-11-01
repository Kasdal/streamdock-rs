//! Loopback HTTP server for the daemon.
//!
//! The server never touches the device. It reads a shared state and sends
//! commands to the daemon thread, which owns the HID session.
//!
//! Safety:
//!
//! 1. The socket binds to `127.0.0.1` only.
//! 2. Every request needs the token from the runtime file.
//! 3. The body is limited to 1 MiB and the header to 64 KiB.
//! 4. A connection is closed after one request.
//!
//! The editor page arrives with the binary, so no file is read from disk and
//! no path can escape a root folder.

pub mod http;

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use streamdock_store::profile::Profile;

use http::{Request, Response};

/// Most connections served at the same time.
pub const MAX_CONNECTIONS: usize = 16;

/// How long a request waits for the daemon to answer.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// The editor page.
const UI: &str = include_str!("ui.html");

/// What the client may read.
#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub connected: bool,
    pub page: usize,
    pub page_name: String,
    pub brightness: Option<u8>,
    pub profile: Profile,
}

impl State {
    /// Build the state for a page index.
    pub fn new(profile: Profile, page: usize) -> Self {
        let page_name = profile
            .pages
            .get(page)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        Self {
            connected: false,
            page,
            page_name,
            brightness: None,
            profile,
        }
    }

    /// Change the page and keep the name in step.
    pub fn set_page(&mut self, page: usize) {
        if let Some(found) = self.profile.pages.get(page) {
            self.page = page;
            self.page_name = found.name.clone();
        }
    }

    /// Replace the profile and keep the page in range.
    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
        if self.page >= self.profile.pages.len() {
            self.page = self.profile.pages.len().saturating_sub(1);
        }
        self.page_name = self
            .profile
            .pages
            .get(self.page)
            .map(|p| p.name.clone())
            .unwrap_or_default();
    }
}

/// State shared between the daemon loop and the HTTP threads.
pub struct Shared {
    inner: Mutex<State>,
}

impl Shared {
    /// Wrap a state for sharing.
    pub fn new(state: State) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(state),
        })
    }

    /// A copy of the state.
    pub fn snapshot(&self) -> State {
        self.inner.lock().expect("state lock").clone()
    }

    /// Change the state.
    pub fn update(&self, change: impl FnOnce(&mut State)) {
        let mut state = self.inner.lock().expect("state lock");
        change(&mut state);
    }
}

/// Answer channel for one command.
pub type Reply = Sender<Result<State, String>>;

/// A change that only the daemon thread can apply.
pub enum Command {
    /// Replace the profile, save it and repaint.
    Save { profile: Profile, reply: Reply },
    /// Show another page by name.
    SwitchPage { target: String, reply: Reply },
    /// Set the panel brightness.
    Brightness { value: u8, reply: Reply },
    /// Read the profile from disk again.
    Reload { reply: Reply },
}

/// A running server.
pub struct Server {
    pub port: u16,
    pub token: String,
}

impl Server {
    /// Bind a free loopback port and start to serve.
    pub fn start(shared: Arc<Shared>, commands: Sender<Command>) -> io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let token = make_token();

        let context = Arc::new(Context {
            shared,
            commands,
            token: token.clone(),
        });

        thread::Builder::new()
            .name("streamdock-http".into())
            .spawn(move || accept_loop(listener, context))?;

        Ok(Self { port, token })
    }

    /// The URL that opens the editor with the token.
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/?token={}",
            self.port, self.token
        )
    }

    /// Write the port and the token to `runtime.json` so a client can find us.
    pub fn write_runtime_file(&self) -> io::Result<PathBuf> {
        let path = runtime_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::json!({ "port": self.port, "token": self.token }).to_string();
        std::fs::write(&path, text)?;
        Ok(path)
    }
}

/// `%LOCALAPPDATA%\StreamDock\runtime.json`.
pub fn runtime_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("StreamDock").join("runtime.json")
}

struct Context {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    token: String,
}

static OPEN: AtomicUsize = AtomicUsize::new(0);

struct OpenGuard;

impl OpenGuard {
    fn take() -> Option<Self> {
        let open = OPEN.fetch_add(1, Ordering::SeqCst);
        if open >= MAX_CONNECTIONS {
            OPEN.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self)
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        OPEN.fetch_sub(1, Ordering::SeqCst);
    }
}

fn accept_loop(listener: TcpListener, context: Arc<Context>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let Some(guard) = OpenGuard::take() else {
            continue;
        };
        let context = Arc::clone(&context);
        thread::spawn(move || {
            let _guard = guard;
            handle_connection(stream, &context);
        });
    }
}

fn handle_connection(mut stream: TcpStream, context: &Context) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let response = match http::read_request(&mut stream) {
        Ok(request) => route(context, &request),
        Err(message) => Response::message(400, message),
    };
    let _ = http::write_response(&mut stream, &response);
}

fn route(context: &Context, request: &Request) -> Response {
    let method = request.method.as_str();
    let path = request.path.as_str();

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            if request.query_value("token") != Some(context.token.as_str()) {
                return Response::message(403, "wrong token");
            }
            Response::html(UI)
        }
        ("GET", "/api/state") => {
            if !api_token_ok(context, request) {
                return Response::message(401, "missing or wrong token");
            }
            json_state(&context.shared.snapshot())
        }
        ("PUT", "/api/profile") => {
            if !api_token_ok(context, request) {
                return Response::message(401, "missing or wrong token");
            }
            let profile: Profile = match serde_json::from_slice(&request.body) {
                Ok(profile) => profile,
                Err(e) => return Response::message(400, format!("bad profile JSON: {e}")),
            };
            roundtrip(context, |reply| Command::Save { profile, reply })
        }
        ("POST", "/api/page") => {
            if !api_token_ok(context, request) {
                return Response::message(401, "missing or wrong token");
            }
            let body: PageRequest = match serde_json::from_slice(&request.body) {
                Ok(body) => body,
                Err(e) => return Response::message(400, format!("bad page request: {e}")),
            };
            let name = match (body.name, body.index) {
                (Some(name), _) => name,
                (None, Some(index)) => {
                    let state = context.shared.snapshot();
                    match state.profile.pages.get(index) {
                        Some(page) => page.name.clone(),
                        None => return Response::message(400, "no such page index"),
                    }
                }
                (None, None) => return Response::message(400, "need name or index"),
            };
            roundtrip(context, |reply| Command::SwitchPage {
                target: name,
                reply,
            })
        }
        ("POST", "/api/brightness") => {
            if !api_token_ok(context, request) {
                return Response::message(401, "missing or wrong token");
            }
            let body: BrightnessRequest = match serde_json::from_slice(&request.body) {
                Ok(body) => body,
                Err(e) => return Response::message(400, format!("bad brightness: {e}")),
            };
            if body.value > 100 {
                return Response::message(400, "brightness is above 100");
            }
            roundtrip(context, |reply| Command::Brightness {
                value: body.value,
                reply,
            })
        }
        ("POST", "/api/reload") => {
            if !api_token_ok(context, request) {
                return Response::message(401, "missing or wrong token");
            }
            roundtrip(context, |reply| Command::Reload { reply })
        }
        _ => Response::message(404, "no such path"),
    }
}

fn api_token_ok(context: &Context, request: &Request) -> bool {
    request.header("x-token") == Some(context.token.as_str())
}

fn json_state(state: &State) -> Response {
    match serde_json::to_string(state) {
        Ok(text) => Response::json(200, text),
        Err(e) => Response::message(500, format!("cannot encode state: {e}")),
    }
}

fn roundtrip(context: &Context, build: impl FnOnce(Reply) -> Command) -> Response {
    let (reply, answer) = channel();
    if context.commands.send(build(reply)).is_err() {
        return Response::message(503, "the daemon is not running");
    }
    match answer.recv_timeout(REPLY_TIMEOUT) {
        Ok(Ok(state)) => json_state(&state),
        Ok(Err(message)) => Response::message(400, message),
        Err(_) => Response::message(500, "the daemon did not answer"),
    }
}

/// Token: 32 hex characters from the clock and the process ID.
///
/// The socket is loopback only and the token lives in a per-user file, so this
/// is a guard against a wrong client, not a cryptographic secret.
fn make_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(1);
    let mut x = nanos ^ ((std::process::id() as u128) << 64) ^ 0x9E37_79B9_7F4A_7C15;
    let mut token = String::with_capacity(32);
    for _ in 0..4 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        token.push_str(&format!("{:08x}", x as u32));
    }
    token
}

#[derive(Debug, Deserialize)]
struct PageRequest {
    name: Option<String>,
    index: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct BrightnessRequest {
    value: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        let state = State::new(Profile::starter(), 0);
        let (commands, _receiver) = channel();
        Context {
            shared: Shared::new(state),
            commands,
            token: "abc123".to_string(),
        }
    }

    fn request(method: &str, path: &str) -> Request {
        Request {
            method: method.to_string(),
            path: path.to_string(),
            query: Vec::new(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn the_page_needs_the_token_in_the_query() {
        let context = context();
        let mut request = request("GET", "/");
        assert_eq!(route(&context, &request).status, 403);
        request.query.push(("token".to_string(), "abc123".to_string()));
        let response = route(&context, &request);
        assert_eq!(response.status, 200);
        assert!(String::from_utf8_lossy(&response.body).contains("<html"));
    }

    #[test]
    fn the_api_needs_the_token_header() {
        let context = context();
        let mut request = request("GET", "/api/state");
        assert_eq!(route(&context, &request).status, 401);
        request
            .headers
            .push(("X-Token".to_string(), "abc123".to_string()));
        let response = route(&context, &request);
        assert_eq!(response.status, 200);
        let text = String::from_utf8_lossy(&response.body);
        assert!(text.contains("\"connected\""));
        assert!(text.contains("\"profile\""));
    }

    #[test]
    fn a_bad_profile_is_rejected_before_the_daemon() {
        let context = context();
        let mut request = request("PUT", "/api/profile");
        request
            .headers
            .push(("x-token".to_string(), "abc123".to_string()));
        request.body = b"{ not json".to_vec();
        assert_eq!(route(&context, &request).status, 400);
    }

    #[test]
    fn a_brightness_above_100_is_rejected() {
        let context = context();
        let mut request = request("POST", "/api/brightness");
        request
            .headers
            .push(("x-token".to_string(), "abc123".to_string()));
        request.body = br#"{ "value": 140 }"#.to_vec();
        assert_eq!(route(&context, &request).status, 400);
    }

    #[test]
    fn an_unknown_path_is_404() {
        let context = context();
        let mut request = request("GET", "/nope");
        request
            .headers
            .push(("x-token".to_string(), "abc123".to_string()));
        assert_eq!(route(&context, &request).status, 404);
    }

    #[test]
    fn tokens_are_32_hex_characters() {
        let token = make_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_runtime_file_lives_in_the_streamdock_folder() {
        let path = runtime_path();
        assert!(path.ends_with("StreamDock/runtime.json") || path.ends_with("StreamDock\\runtime.json"));
    }

    #[test]
    fn state_keeps_the_page_name_in_step() {
        let profile = Profile::demo();
        let mut state = State::new(profile, 0);
        assert_eq!(state.page_name, "Main");
        state.set_page(1);
        assert_eq!(state.page_name, "Second");
        state.set_page(9);
        assert_eq!(state.page, 1);
    }

    #[test]
    fn a_new_profile_clamps_the_page() {
        let mut state = State::new(Profile::demo(), 1);
        state.set_profile(Profile::starter());
        assert_eq!(state.page, 0);
        assert_eq!(state.page_name, "Main");
    }
}
