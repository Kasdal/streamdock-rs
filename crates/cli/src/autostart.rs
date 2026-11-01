//! Autostart at logon through the per-user Run key.
//!
//! The value lives in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! It needs no administrator rights and no shortcut file.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

/// Key that Windows reads at logon for this user.
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Name of the value.
const VALUE_NAME: &str = "StreamDockDaemon";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Command line stored in the Run value.
///
/// The path is quoted, so a folder with a space works.
pub fn command_line(daemon: &Path, extra: &[&str]) -> String {
    let mut value = format!("\"{}\"", daemon.display());
    for arg in extra {
        value.push(' ');
        value.push_str(arg);
    }
    value
}

/// Path of `streamdockd.exe` next to this program.
pub fn daemon_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    let folder = exe
        .parent()
        .ok_or_else(|| "this program has no folder".to_string())?;
    let daemon = folder.join("streamdockd.exe");
    if !daemon.exists() {
        return Err(format!(
            "{} is missing. Build the daemon first.",
            daemon.display()
        ));
    }
    Ok(daemon)
}

fn open(access: u32) -> Result<HKEY, String> {
    let path = wide(RUN_KEY);
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    if status != ERROR_SUCCESS {
        return Err(format!(
            "cannot open HKCU\\{RUN_KEY}: error {status}"
        ));
    }
    Ok(key)
}

/// Add the daemon to the logon list.
pub fn install(extra: &[&str]) -> Result<String, String> {
    let daemon = daemon_path()?;
    let value = command_line(&daemon, extra);

    let key = open(KEY_SET_VALUE)?;
    let name = wide(VALUE_NAME);
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: the buffer is a null terminated UTF-16 string and the length is
    // given in bytes, as REG_SZ requires.
    let status = unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
        )
    };
    unsafe { RegCloseKey(key) };

    if status != ERROR_SUCCESS {
        return Err(format!("cannot write the autostart value: error {status}"));
    }
    Ok(value)
}

/// Remove the daemon from the logon list.
pub fn remove() -> Result<bool, String> {
    let key = open(KEY_SET_VALUE)?;
    let name = wide(VALUE_NAME);
    let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
    unsafe { RegCloseKey(key) };

    match status {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        other => Err(format!("cannot remove the autostart value: error {other}")),
    }
}

/// The current Run value, if it exists.
pub fn status() -> Result<Option<String>, String> {
    let key = match open(KEY_QUERY_VALUE) {
        Ok(key) => key,
        Err(_) => return Ok(None),
    };
    let name = wide(VALUE_NAME);

    let mut kind: u32 = 0;
    let mut size: u32 = 0;
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };

    if status == ERROR_FILE_NOT_FOUND {
        unsafe { RegCloseKey(key) };
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        unsafe { RegCloseKey(key) };
        return Err(format!("cannot read the autostart value: error {status}"));
    }

    let mut buffer = vec![0u16; (size as usize / 2) + 1];
    let mut read = size;
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            buffer.as_mut_ptr() as *mut u8,
            &mut read,
        )
    };
    unsafe { RegCloseKey(key) };

    if status != ERROR_SUCCESS {
        return Err(format!("cannot read the autostart value: error {status}"));
    }

    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..length])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_quotes_the_path() {
        let line = command_line(Path::new("C:\\Program Files\\Stream Dock\\streamdockd.exe"), &[]);
        assert_eq!(line, "\"C:\\Program Files\\Stream Dock\\streamdockd.exe\"");
    }

    #[test]
    fn the_command_line_keeps_extra_arguments() {
        let line = command_line(Path::new("C:\\dock\\streamdockd.exe"), &["--profile", "Demo"]);
        assert_eq!(line, "\"C:\\dock\\streamdockd.exe\" --profile Demo");
    }
}
