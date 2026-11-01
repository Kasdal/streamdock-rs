//! Per-application profiles.
//!
//! Every profile can list process names in `apps`. The daemon looks at the
//! foreground process once a second and switches to the profile that matches.
//! When no profile matches, it returns to the default profile.
//!
//! The check is a poll, so it needs no hook and no elevated right.

use std::time::{Duration, Instant};

use streamdock_store::Store;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Shortest time between two checks.
pub const CHECK_INTERVAL: Duration = Duration::from_millis(1000);

/// Longest process path that the watcher reads.
const MAX_PATH_CHARS: usize = 1024;

/// Rules from process name to profile name, with the active profile.
pub struct AutoSwitch {
    rules: Vec<(String, String)>,
    default_profile: String,
    current: Option<String>,
    next_check: Instant,
    enabled: bool,
    verbose: bool,
    /// Number of rules that the last build found.
    pub rule_count: usize,
}

impl AutoSwitch {
    /// Build the rules and log the foreground process on every check.
    pub fn build_verbose(
        store: &Store,
        default_profile: &str,
        enabled: bool,
        verbose: bool,
    ) -> Self {
        let mut this = Self {
            rules: Vec::new(),
            default_profile: default_profile.to_string(),
            current: None,
            next_check: Instant::now(),
            enabled,
            verbose,
            rule_count: 0,
        };
        this.rebuild(store);
        this
    }

    /// Read the rules again, after a profile changed.
    pub fn rebuild(&mut self, store: &Store) {
        self.rules.clear();

        if let Ok(names) = store.list() {
            for name in names {
                let Ok(profile) = store.load(&name) else {
                    continue;
                };
                for app in &profile.apps {
                    let key = app.trim().to_ascii_lowercase();
                    if !key.is_empty() {
                        self.rules.push((key, name.clone()));
                    }
                }
            }
        }

        self.rule_count = self.rules.len();
        self.current = None;
    }

    /// The profile that the daemon started with.
    pub fn default_profile(&self) -> &str {
        &self.default_profile
    }

    /// True when the poll runs.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The profile that matches one process name.
    pub fn profile_for(&self, process: &str) -> Option<&str> {
        let key = process.to_ascii_lowercase();
        self.rules
            .iter()
            .find(|(app, _)| *app == key)
            .map(|(_, name)| name.as_str())
    }

    /// Ask for a profile switch. Returns the name only when it changed.
    pub fn check(&mut self, now: Instant) -> Option<String> {
        if !self.enabled || now < self.next_check {
            return None;
        }
        self.next_check = now + CHECK_INTERVAL;

        let process = foreground_process()?;
        if self.verbose {
            crate::log(format!("foreground process: {process}"));
        }
        let target = self
            .profile_for(&process)
            .map(|name| name.to_string())
            .unwrap_or_else(|| self.default_profile.clone());

        if self.current.as_deref() == Some(target.as_str()) {
            return None;
        }
        self.current = Some(target.clone());
        Some(target)
    }
}

/// File name of the process that owns the foreground window.
pub fn foreground_process() -> Option<String> {
    // SAFETY: the handle is closed on every path, and the buffer is local.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == 0 {
            return None;
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }

        let mut buffer = vec![0u16; MAX_PATH_CHARS];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);

        if ok == 0 || length == 0 {
            return None;
        }

        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        std::path::Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use streamdock_store::profile::Profile;

    fn store_with_profiles() -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!(
            "streamdock-apps-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let store = Store::new(&dir);

        let mut obs = Profile::new("OBS");
        obs.apps = vec!["obs64.exe".into()];
        store.save(&obs).expect("save obs");

        let mut art = Profile::new("Art");
        art.apps = vec!["Photoshop.exe".into(), "Krita.exe".into()];
        store.save(&art).expect("save art");

        (dir, store)
    }

    #[test]
    fn rules_come_from_every_profile() {
        let (dir, store) = store_with_profiles();
        let auto = AutoSwitch::build_verbose(&store, "Default", true, false);
        assert_eq!(auto.rule_count, 3);
        assert_eq!(auto.profile_for("obs64.exe"), Some("OBS"));
        assert_eq!(auto.profile_for("PHOTOSHOP.EXE"), Some("Art"));
        assert_eq!(auto.profile_for("krita.exe"), Some("Art"));
        assert_eq!(auto.profile_for("notepad.exe"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_disabled_watcher_never_switches() {
        let (dir, store) = store_with_profiles();
        let mut auto = AutoSwitch::build_verbose(&store, "Default", false, false);
        assert!(!auto.enabled());
        assert_eq!(auto.check(Instant::now() + CHECK_INTERVAL), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_check_is_rate_limited() {
        let (dir, store) = store_with_profiles();
        let mut auto = AutoSwitch::build_verbose(&store, "Default", true, false);
        let now = Instant::now();
        // The first call runs the poll; later calls inside the interval do not.
        let _ = auto.check(now);
        assert_eq!(auto.check(now + Duration::from_millis(10)), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
