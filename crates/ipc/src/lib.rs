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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use streamdock_core::{image, model};
use streamdock_render as render;
use streamdock_store::profile::Profile;

use http::{Request, Response};

/// Most connections served at the same time.
pub const MAX_CONNECTIONS: usize = 16;

/// Largest image that the editor may upload.
pub const MAX_ASSET_BYTES: usize = 8 * 1024 * 1024;

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
    /// Bumped on every profile change, so a stale save can be refused.
    pub rev: u64,
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
            rev: 0,
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

    /// Replace the profile and keep the page in range. This bumps `rev`.
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
        self.rev += 1;
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

/// Answer from the daemon for one command.
pub enum Reply {
    /// The change was applied.
    Ok(State),
    /// Another client changed the profile first. Carries the current state.
    Conflict(State),
    /// The change was refused.
    Error(String),
}

/// Sender for one reply.
pub type ReplySender = Sender<Reply>;

/// A change that only the daemon thread can apply.
pub enum Command {
    /// Replace the profile, save it and repaint.
    Save {
        profile: Profile,
        /// Revision that the client started from. `None` overwrites.
        expected_rev: Option<u64>,
        reply: ReplySender,
    },
    /// Show another page by name.
    SwitchPage { target: String, reply: ReplySender },
    /// Set the panel brightness.
    Brightness { value: u8, reply: ReplySender },
    /// Read the profile from disk again.
    Reload { reply: ReplySender },
}

/// A running server.
pub struct Server {
    pub port: u16,
    pub token: String,
}

impl Server {
    /// Bind a free loopback port and start to serve.
    ///
    /// `base` is the profile folder. A relative image path in a profile is
    /// resolved against it, and an uploaded image is stored under `assets`.
    pub fn start(shared: Arc<Shared>, commands: Sender<Command>, base: PathBuf) -> io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let token = make_token();

        let context = Arc::new(Context {
            shared,
            commands,
            token: token.clone(),
            base,
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

#[derive(Clone)]
struct Context {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    token: String,
    base: PathBuf,
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
            let expected_rev = request
                .header("x-rev")
                .and_then(|value| value.trim().parse::<u64>().ok());
            roundtrip(context, |reply| Command::Save {
                profile,
                expected_rev,
                reply,
            })
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
        ("GET", "/api/tile") => {
            if request.query_value("token") != Some(context.token.as_str()) {
                return Response::message(401, "missing or wrong token");
            }
            tile_image(context, request)
        }
        ("POST", "/api/asset") => {
            if !api_token_ok(context, request) {
                return Response::message(401, "missing or wrong token");
            }
            store_asset(context, request)
        }
        _ => Response::message(404, "no such path"),
    }
}

/// Draw one key tile as a JPEG for the editor grid.
///
/// The tile is upright, because that is what a person expects to see. The
/// panel rotation is added only when the image goes to the device.
fn tile_image(context: &Context, request: &Request) -> Response {
    let page_index: usize = match request.query_value("page").and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return Response::message(400, "need a page number"),
    };
    let key: u8 = match request.query_value("key").and_then(|v| v.parse().ok()) {
        Some(v) if model::is_valid_key(v) => v,
        _ => return Response::message(400, "need a key number 1..=18"),
    };

    let state = context.shared.snapshot();
    let Some(page) = state.profile.pages.get(page_index) else {
        return Response::message(404, "no such page");
    };

    let scale = render::scale_for(page, key);
    let tile = match render::compose_in(page.key(key), model::key_size(key), scale, &context.base) {
        Ok(tile) => tile,
        Err(message) => return Response::message(400, message),
    };

    match image::encode_quality(&tile, 88) {
        Ok(jpeg) => Response {
            status: 200,
            content_type: "image/jpeg",
            body: jpeg,
        },
        Err(message) => Response::message(500, message),
    }
}

/// Save an uploaded image under `assets` and return its relative path.
fn store_asset(context: &Context, request: &Request) -> Response {
    let name = request.query_value("name").unwrap_or("image");
    let Some(extension) = image_extension(name) else {
        return Response::message(400, "only .jpg and .png files are accepted");
    };

    if request.body.len() > MAX_ASSET_BYTES {
        return Response::message(
            413,
            format!(
                "image is {} bytes, the limit is {MAX_ASSET_BYTES}",
                request.body.len()
            ),
        );
    }

    // Decode first: a file with the wrong content never reaches the profile.
    if let Err(message) = image::decode(&request.body) {
        return Response::message(400, message);
    }

    let folder = context.base.join("assets");
    if let Err(e) = std::fs::create_dir_all(&folder) {
        return Response::message(500, format!("cannot create {}: {e}", folder.display()));
    }

    let file = format!(
        "{}-{}.{}",
        sanitize_stem(name),
        short_hash(&request.body),
        extension
    );
    let path = folder.join(&file);
    if let Err(e) = std::fs::write(&path, &request.body) {
        return Response::message(500, format!("cannot write {}: {e}", path.display()));
    }

    let relative = format!("assets/{file}");
    Response::json(200, serde_json::json!({ "path": relative }).to_string())
}

fn image_extension(name: &str) -> Option<&'static str> {
    let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        _ => None,
    }
}

/// A safe file stem: lowercase letters, digits and single dashes.
fn sanitize_stem(name: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");

    let mut out = String::with_capacity(stem.len());
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    let short: String = trimmed.chars().take(32).collect();
    if short.is_empty() {
        "image".to_string()
    } else {
        short
    }
}

/// Eight hex characters from the content, so the same file gets one name.
fn short_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (hash ^ (hash >> 32)) as u32)
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

fn roundtrip(context: &Context, build: impl FnOnce(ReplySender) -> Command) -> Response {
    let (reply, answer) = channel();
    if context.commands.send(build(reply)).is_err() {
        return Response::message(503, "the daemon is not running");
    }
    match answer.recv_timeout(REPLY_TIMEOUT) {
        Ok(Reply::Ok(state)) => json_state(&state),
        Ok(Reply::Conflict(state)) => match serde_json::to_string(&state) {
            Ok(text) => Response::json(409, text),
            Err(e) => Response::message(500, format!("cannot encode state: {e}")),
        },
        Ok(Reply::Error(message)) => Response::message(400, message),
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
            base: PathBuf::from("."),
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

    fn context_with_commands() -> (Context, std::sync::mpsc::Receiver<Command>) {
        let state = State::new(Profile::starter(), 0);
        let (commands, receiver) = channel();
        (
            Context {
                shared: Shared::new(state),
                commands,
                token: "abc123".to_string(),
                base: PathBuf::from("."),
            },
            receiver,
        )
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

    #[test]
    fn a_profile_change_bumps_the_revision() {
        let mut state = State::new(Profile::starter(), 0);
        assert_eq!(state.rev, 0);
        state.set_profile(Profile::demo());
        assert_eq!(state.rev, 1);
        state.set_page(1);
        assert_eq!(state.rev, 1);
        state.set_profile(Profile::starter());
        assert_eq!(state.rev, 2);
    }

    #[test]
    fn a_stale_save_gets_a_conflict_and_the_current_state() {
        let (context, commands) = context_with_commands();
        let mut request = request("PUT", "/api/profile");
        request
            .headers
            .push(("x-token".to_string(), "abc123".to_string()));
        request
            .headers
            .push(("x-rev".to_string(), "3".to_string()));
        request.body = serde_json::to_vec(&Profile::starter()).expect("json");

        let worker = context.clone();
        let handle = std::thread::spawn(move || route(&worker, &request));

        match commands.recv().expect("a save command") {
            Command::Save {
                expected_rev,
                reply,
                ..
            } => {
                assert_eq!(expected_rev, Some(3));
                let current = State::new(Profile::starter(), 0);
                let _ = reply.send(Reply::Conflict(current));
            }
            _ => panic!("expected a save command"),
        }

        let response = handle.join().expect("thread");
        assert_eq!(response.status, 409);
        let text = String::from_utf8(response.body).expect("utf8");
        assert!(text.contains("\"rev\""));
    }

    #[test]
    fn a_save_without_a_revision_is_accepted() {
        let (context, commands) = context_with_commands();
        let mut request = request("PUT", "/api/profile");
        request
            .headers
            .push(("x-token".to_string(), "abc123".to_string()));
        request.body = serde_json::to_vec(&Profile::demo()).expect("json");

        let worker = context.clone();
        let handle = std::thread::spawn(move || route(&worker, &request));

        match commands.recv().expect("a save command") {
            Command::Save {
                expected_rev,
                reply,
                profile,
            } => {
                assert_eq!(expected_rev, None);
                assert_eq!(profile.name, "Demo");
                let _ = reply.send(Reply::Ok(State::new(profile, 0)));
            }
            _ => panic!("expected a save command"),
        }

        let response = handle.join().expect("thread");
        assert_eq!(response.status, 200);
    }
}
