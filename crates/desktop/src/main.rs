//! Stream Dock desktop window.
//!
//! The editor page is bundled in this program. The daemon still owns the
//! device and the API. The window finds the API endpoint, starts the daemon
//! when it is not running, and shows the editor.
//!
//! The tray menu keeps the editor one click away after the window is closed.
//!
//! Only one copy of the window runs. A second start hands its command line to
//! the first copy and exits. The first copy shows and focuses its window.
//!
//! The window comes back at the place and size it had. The shape is in
//! `%LOCALAPPDATA%\StreamDock\window.json`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

/// The smallest window the layout allows.
const MIN_WIDTH: u32 = 1000;
const MIN_HEIGHT: u32 = 680;

/// How long a move or a resize waits before the shape reaches the disk.
const SAVE_DELAY: Duration = Duration::from_millis(500);

static TRAY_READY: AtomicBool = AtomicBool::new(false);
/// The last shape that was written, so a still window writes nothing.
static LAST_SHAPE: Mutex<Option<WindowRect>> = Mutex::new(None);
/// Set while a save waits for the window to stop moving.
static PENDING_SAVE: AtomicBool = AtomicBool::new(false);

/// Where and how big the window was.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
struct WindowRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    /// Set when the window was maximized. The rect then holds the size from
    /// before the maximize.
    #[serde(default)]
    maximized: bool,
}

/// API endpoint of the running daemon.
#[derive(Serialize)]
struct Endpoint {
    port: u16,
    token: String,
}

fn main() {
    let app = tauri::Builder::default()
        // This plugin must come first: a second start must exit before it
        // creates a window or a tray icon. The first copy takes the command
        // line and brings its window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            trace("a second copy started; show the window");
            show_editor(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![endpoint, export_profile])
        .setup(|app| {
            // Build the window hidden, put it back where it was, then show it.
            // This way the restored window never appears at the default place
            // first.
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Stream Dock")
                .inner_size(1320.0, 860.0)
                .min_inner_size(1000.0, 680.0)
                .visible(false)
                .build()?;
            restore_window(&window);
            window.show()?;

            // Start the daemon when it is not running. The page waits for the
            // endpoint, so this must not block the window.
            std::thread::spawn(|| {
                let _ = endpoint_or_start();
            });

            let show = MenuItem::with_id(app, "show", "Show editor", true, None::<&str>)?;
            let reload = MenuItem::with_id(app, "reload", "Reload profile", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &reload, &quit])?;

            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.ico"))?;
            TrayIconBuilder::with_id("editor")
                .icon(icon)
                .tooltip("Stream Dock editor")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_editor(app),
                    "reload" => reload_profile(),
                    "quit" => {
                        trace("quit from the tray menu");
                        if let Some(window) = app.get_webview_window("main") {
                            remember(&window.as_ref().window(), true);
                        }
                        // Release the single-instance lock first, because the
                        // direct exit below skips the normal cleanup.
                        tauri_plugin_single_instance::destroy(app);
                        app.exit(0);
                        // The tray icon can keep the event loop alive. Exit
                        // directly so the tray entry never stays behind.
                        std::process::exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            TRAY_READY.store(true, Ordering::SeqCst);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if TRAY_READY.load(Ordering::SeqCst) {
                    // Save now: a window that is only hidden may not move again.
                    remember(window, true);
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => remember(window, false),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("cannot start the Stream Dock window");

    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(window) = app.get_webview_window("main") {
                remember(&window.as_ref().window(), true);
            }
        }
    });
}

/// The file that holds the window shape.
fn window_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(base.join("StreamDock").join("window.json"))
}

fn load_window_rect() -> Option<WindowRect> {
    let text = std::fs::read_to_string(window_path()?).ok()?;
    serde_json::from_str(&text).ok()
}

fn store_window_rect(rect: &WindowRect) -> std::io::Result<()> {
    let Some(path) = window_path() else {
        return Ok(());
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let text = serde_json::to_string_pretty(rect).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// Put the window back where it was, if that place still exists.
fn restore_window(window: &tauri::WebviewWindow) {
    let Some(saved) = load_window_rect() else {
        return;
    };

    let width = saved.width.max(MIN_WIDTH);
    let height = saved.height.max(MIN_HEIGHT);
    let _ = window.set_size(tauri::PhysicalSize::new(width, height));

    let mut rect = WindowRect {
        width,
        height,
        ..saved
    };

    if rect_is_visible(window, saved) {
        let _ = window.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
        trace(&format!(
            "restored the window at {},{} with {}x{}",
            saved.x, saved.y, width, height
        ));
    } else {
        // A monitor was removed since the last run. Start centred instead.
        let _ = window.center();
        if let Ok(position) = window.outer_position() {
            rect.x = position.x;
            rect.y = position.y;
        }
        trace("the saved window place is gone; centred the window");
    }

    // Hold the shape in memory, so a maximize right after the start keeps this
    // place instead of storing the maximized rect as the normal one.
    if let Ok(mut last) = LAST_SHAPE.lock() {
        *last = Some(WindowRect {
            maximized: false,
            ..rect
        });
    }

    if saved.maximized {
        let _ = window.maximize();
    }
}

/// True when enough of the rect lies on a monitor.
fn rect_is_visible(window: &tauri::WebviewWindow, rect: WindowRect) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return true;
    };
    monitors.iter().any(|monitor| {
        let area = monitor.work_area();
        let left = rect.x.max(area.position.x);
        let top = rect.y.max(area.position.y);
        let right = (rect.x + rect.width as i32).min(area.position.x + area.size.width as i32);
        let bottom = (rect.y + rect.height as i32).min(area.position.y + area.size.height as i32);
        // The title bar and a corner must stay reachable.
        right - left >= 160 && bottom - top >= 120
    })
}

/// Remember the shape after a move or a resize.
///
/// A moving window queues one save, 500 ms after the last event. The delay
/// also covers the maximize transition, which sends a resize before it sets
/// the maximized flag. `force` writes at once, for the close button and Quit.
fn remember(window: &tauri::Window, force: bool) {
    if force {
        write_shape(window);
        return;
    }
    if PENDING_SAVE.swap(true, Ordering::SeqCst) {
        // A save is already queued. It reads the shape when it runs, so the
        // newest event is covered.
        return;
    }
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SAVE_DELAY);
        PENDING_SAVE.store(false, Ordering::SeqCst);
        write_shape(&window);
    });
}

/// Write the current shape, unless it is the one on disk already.
fn write_shape(window: &tauri::Window) {
    // A minimized window reports a place off the screen.
    if window.is_minimized().unwrap_or(false) {
        return;
    }

    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    let maximized = window.is_maximized().unwrap_or(false);

    let mut last = match LAST_SHAPE.lock() {
        Ok(last) => last,
        Err(_) => return,
    };

    let rect = match (*last, maximized) {
        // Keep the size from before the maximize, so a later restore is normal.
        (Some(previous), true) => WindowRect {
            maximized: true,
            ..previous
        },
        _ => WindowRect {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized,
        },
    };

    if *last == Some(rect) {
        return;
    }

    if store_window_rect(&rect).is_ok() {
        *last = Some(rect);
    }
}

fn show_editor(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// The API endpoint of the daemon, for the page.
#[tauri::command]
fn endpoint() -> Option<Endpoint> {
    let path = runtime_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    let port = value.get("port")?.as_u64()? as u16;
    let token = value.get("token")?.as_str()?.to_string();

    if daemon_answers(port) {
        Some(Endpoint { port, token })
    } else {
        None
    }
}

/// Ask for a file and save the active profile there.
///
/// The dialog is not shown when the daemon is down or it refuses the export,
/// so a broken export never opens an empty save window.
#[tauri::command]
async fn export_profile(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let endpoint = endpoint().ok_or("the daemon is not running")?;
    let path = format!("/api/export?token={}", endpoint.token);
    let bytes = http_get(endpoint.port, &endpoint.token, &path)?;

    let mut dialog = app
        .dialog()
        .file()
        .set_title("Export profile")
        .set_file_name(export_name(endpoint.port, &endpoint.token))
        .add_filter("Zip archive", &["zip"]);
    if let Some(folder) = downloads_folder() {
        dialog = dialog.set_directory(folder);
    }

    // The command runs off the main thread, so this wait is safe. The dialog
    // itself is shown on the main thread.
    let Some(chosen) = dialog.blocking_save_file() else {
        trace("export cancelled");
        return Ok(None);
    };

    let file = chosen.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    trace(&format!("exported the profile to {}", file.display()));
    Ok(Some(file.display().to_string()))
}

/// A suggested file name: the profile name, as a zip.
fn export_name(port: u16, token: &str) -> String {
    let name = http_get(port, token, "/api/state")
        .ok()
        .and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok())
        .and_then(|value| {
            let name = value.get("profile")?.get("name")?.as_str()?.to_string();
            Some(name)
        });

    match name {
        Some(name) => {
            let clean: String = name
                .chars()
                .map(|c| if "\\/:*?\"<>|".contains(c) { '-' } else { c })
                .collect();
            let clean = clean.trim().to_string();
            if clean.is_empty() {
                "StreamDock-profile.zip".to_string()
            } else {
                format!("{clean}.zip")
            }
        }
        None => "StreamDock-profile.zip".to_string(),
    }
}

/// The user's Downloads folder, when it exists.
fn downloads_folder() -> Option<PathBuf> {
    let folder = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)?
        .join("Downloads");
    folder.is_dir().then_some(folder)
}

/// The endpoint, after starting the daemon when it is not running.
fn endpoint_or_start() -> Option<Endpoint> {
    if let Some(endpoint) = endpoint() {
        trace("daemon already running");
        return Some(endpoint);
    }

    match daemon_exe() {
        Some(daemon) => {
            trace(&format!("starting {}", daemon.display()));
            match Command::new(&daemon).arg("--no-tray").spawn() {
                Ok(child) => trace(&format!("started pid {}", child.id())),
                Err(error) => trace(&format!("cannot start the daemon: {error}")),
            }
        }
        None => trace("no streamdockd.exe found"),
    }

    for _ in 0..24 {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(endpoint) = endpoint() {
            trace("daemon is ready");
            return Some(endpoint);
        }
    }

    trace("the daemon did not answer");
    None
}

fn daemon_answers(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    let request = "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Token: probe\r\nConnection: close\r\n\r\n";
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buffer = [0u8; 64];
    match stream.read(&mut buffer) {
        Ok(read) => {
            let text = String::from_utf8_lossy(&buffer[..read]);
            text.starts_with("HTTP/1.1 200") || text.starts_with("HTTP/1.1 401")
        }
        Err(_) => false,
    }
}

fn http_get(port: u16, token: &str, path: &str) -> Result<Vec<u8>, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|e| format!("cannot connect to the daemon: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Token: {token}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("cannot send the request: {e}"))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| format!("cannot read the answer: {e}"))?;

    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "bad HTTP answer".to_string())?;
    let head = String::from_utf8_lossy(&response[..split]).to_string();
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!(
            "the daemon refused the export: {}",
            head.lines().next().unwrap_or("")
        ));
    }
    Ok(response[split + 4..].to_vec())
}

/// Reload the profile in the running daemon.
fn reload_profile() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let address = SocketAddr::from(([127, 0, 0, 1], endpoint.port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(1)) else {
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request = format!(
        "POST /api/reload HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Token: {}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}",
        endpoint.token
    );
    let _ = stream.write_all(request.as_bytes());
    let mut buffer = [0u8; 64];
    let _ = stream.read(&mut buffer);
}

/// Append one line to the desktop log.
fn trace(message: &str) {
    let Some(base) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return;
    };
    let folder = base.join("StreamDock").join("logs");
    let _ = std::fs::create_dir_all(&folder);
    let path = folder.join("desktop.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}

fn runtime_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(base.join("StreamDock").join("runtime.json"))
}

/// `streamdockd.exe` next to this program, or in a build folder above it.
fn daemon_exe() -> Option<PathBuf> {
    let own = std::env::current_exe().ok()?;
    let mut folder = own.parent();

    for _ in 0..8 {
        let dir = folder?;
        for candidate in [
            dir.join("streamdockd.exe"),
            dir.join("release").join("streamdockd.exe"),
            dir.join("target").join("release").join("streamdockd.exe"),
        ] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
        folder = dir.parent();
    }

    None
}
