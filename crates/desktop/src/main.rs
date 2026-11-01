//! Stream Dock desktop window.
//!
//! The editor page is bundled in this program. The daemon still owns the
//! device and the API. The window finds the API endpoint, starts the daemon
//! when it is not running, and shows the editor.
//!
//! The tray menu keeps the editor one click away after the window is closed.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

static TRAY_READY: AtomicBool = AtomicBool::new(false);

/// API endpoint of the running daemon.
#[derive(Serialize)]
struct Endpoint {
    port: u16,
    token: String,
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![endpoint, export_profile])
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Stream Dock")
                .inner_size(1320.0, 860.0)
                .min_inner_size(1000.0, 680.0)
                .build()?;

            // Start the daemon when it is not running. The page waits for the
            // endpoint, so this must not block the window.
            std::thread::spawn(|| {
                let _ = endpoint_or_start();
            });

            let show = MenuItem::with_id(app, "show", "Show editor", true, None::<&str>)?;
            let reload = MenuItem::with_id(app, "reload", "Reload profile", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &reload, &quit])?;

            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if TRAY_READY.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("cannot start the Stream Dock window");
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

/// Save the active profile as a zip in the user's Downloads folder.
#[tauri::command]
fn export_profile() -> Result<String, String> {
    let endpoint = endpoint().ok_or("the daemon is not running")?;
    let path = format!("/api/export?token={}", endpoint.token);
    let bytes = http_get(endpoint.port, &endpoint.token, &path)?;

    let folder = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .ok_or_else(|| "no USERPROFILE".to_string())?
        .join("Downloads");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;

    let file = folder.join("StreamDock-profile.zip");
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    Ok(file.display().to_string())
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
