//! External plugin host.
//!
//! A plugin is a folder with a `manifest.json` and a program:
//!
//! ```json
//! { "command": "plugin.exe", "args": [] }
//! ```
//!
//! The daemon starts the program, writes one JSON line and reads one JSON line
//! back. The program decides what to do. This keeps arbitrary code out of the
//! daemon, and any language can write a plugin.
//!
//! Request: `{ "action": "play", "settings": { ... } }`
//! Reply:   `{ "ok": true }` or `{ "error": "why" }`

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::{ActionError, Result};

/// How long a plugin may take to answer.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// Environment variable that moves the plugin folder, for tests.
pub const DIR_ENV: &str = "STREAMDOCK_PLUGINS";

/// Folder that holds one folder per plugin.
pub fn plugin_dir() -> PathBuf {
    if let Some(path) = std::env::var_os(DIR_ENV) {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("StreamDock").join("profiles").join("plugins")
}

#[derive(serde::Deserialize)]
struct Manifest {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Reply {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    error: String,
}

/// Run one plugin action.
pub fn run(id: &str, action: &str, settings: &serde_json::Value) -> Result<()> {
    let folder = plugin_dir().join(id);
    let manifest_path = folder.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| {
        ActionError::Os(format!(
            "plugin {id}: cannot read {}: {e}",
            manifest_path.display()
        ))
    })?;
    let manifest: Manifest = serde_json::from_str(&text)
        .map_err(|e| ActionError::Os(format!("plugin {id}: bad manifest: {e}")))?;

    let mut child = Command::new(&manifest.command)
        .args(&manifest.args)
        .current_dir(&folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ActionError::Os(format!("plugin {id}: cannot start: {e}")))?;

    let payload = serde_json::json!({ "action": action, "settings": settings }).to_string();
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(payload.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| ActionError::Os(format!("plugin {id}: cannot send: {e}")))?;
    }
    drop(child.stdin.take());

    let (sender, receiver) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = sender.send(line);
        });
    }

    let line = match receiver.recv_timeout(TIMEOUT) {
        Ok(line) => line,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ActionError::Os(format!(
                "plugin {id}: no answer within {} s",
                TIMEOUT.as_secs()
            )));
        }
    };

    let _ = child.wait();

    let reply: Reply = serde_json::from_str(line.trim()).map_err(|e| {
        ActionError::Os(format!("plugin {id}: bad reply {line:?}: {e}"))
    })?;

    if reply.ok {
        Ok(())
    } else if reply.error.is_empty() {
        Err(ActionError::Os(format!("plugin {id}: {action} failed")))
    } else {
        Err(ActionError::Os(format!(
            "plugin {id}: {}",
            reply.error
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_answers_one_request() {
        let dir = std::env::temp_dir().join(format!("streamdock-plugin-{}", std::process::id()));
        let plugin = dir.join("echo");
        std::fs::create_dir_all(&plugin).expect("dir");

        std::fs::write(
            plugin.join("manifest.json"),
            r#"{ "command": "powershell.exe", "args": ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "plugin.ps1"] }"#,
        )
        .expect("manifest");
        std::fs::write(
            plugin.join("plugin.ps1"),
            r#"$line = [Console]::In.ReadLine()
$data = $line | ConvertFrom-Json
if ($data.action -eq "ping") { Write-Output '{"ok":true}' }
else { Write-Output '{"error":"unknown action"}' }
"#,
        )
        .expect("script");

        std::env::set_var(DIR_ENV, &dir);
        let ok = run("echo", "ping", &serde_json::json!({}));
        let failed = run("echo", "nope", &serde_json::json!({}));
        std::env::remove_var(DIR_ENV);

        assert!(ok.is_ok(), "{ok:?}");
        assert!(failed.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_plugin_is_an_error() {
        std::env::set_var(DIR_ENV, std::env::temp_dir());
        let result = run("no-such-plugin", "x", &serde_json::json!({}));
        std::env::remove_var(DIR_ENV);
        assert!(result.is_err());
    }
}
