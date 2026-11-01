//! Stream Dock desktop window.
//!
//! The daemon owns the device and serves the editor page. This program is the
//! native window around that page: no browser chrome, its own icon and its own
//! entry in the task bar.
//!
//! On start it finds the daemon through `runtime.json`. When the daemon is not
//! running it starts `streamdockd.exe` next to this program, waits for the
//! endpoint and then shows the editor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::App("index.html".into()),
            )
            .title("Stream Dock")
            .inner_size(1320.0, 860.0)
            .min_inner_size(1000.0, 680.0)
            .build()?;

            std::thread::spawn(move || {
                if let Some(url) = endpoint_or_start() {
                    if let Ok(parsed) = url.parse() {
                        let _ = window.navigate(parsed);
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("cannot start the Stream Dock window");
}

/// The editor URL, after starting the daemon when it is not running.
fn endpoint_or_start() -> Option<String> {
    if let Some(url) = endpoint() {
        trace("daemon already running");
        return Some(url);
    }

    match daemon_exe() {
        Some(daemon) => {
            trace(&format!("starting {}", daemon.display()));
            match Command::new(&daemon).spawn() {
                Ok(child) => trace(&format!("started pid {}", child.id())),
                Err(error) => trace(&format!("cannot start the daemon: {error}")),
            }
        }
        None => trace("no streamdockd.exe found"),
    }

    for _ in 0..24 {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(url) = endpoint() {
            trace("daemon is ready");
            return Some(url);
        }
    }

    trace("the daemon did not answer");
    None
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

/// Read `runtime.json` and check that the daemon answers.
fn endpoint() -> Option<String> {
    let path = runtime_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    let port = value.get("port")?.as_u64()? as u16;
    let token = value.get("token")?.as_str()?.to_string();

    if daemon_answers(port, &token) {
        Some(format!("http://127.0.0.1:{port}/?token={token}"))
    } else {
        None
    }
}

fn daemon_answers(port: u16, token: &str) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(600)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let request = format!(
        "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Token: {token}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut buffer = [0u8; 64];
    match stream.read(&mut buffer) {
        Ok(read) => String::from_utf8_lossy(&buffer[..read]).starts_with("HTTP/1.1 200"),
        Err(_) => false,
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

#[allow(dead_code)]
fn window_title(app: &tauri::AppHandle) -> Option<String> {
    app.get_webview_window("main")?.title().ok()
}
