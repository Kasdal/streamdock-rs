//! One JSON file per profile.
//!
//! The folder holds `Name.json` and an optional `assets` folder. A save writes
//! a temporary file and renames it over the old one, so a crash cannot leave a
//! half written profile.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::profile::{Profile, VERSION};

/// Failure from the store.
#[derive(Debug)]
pub enum StoreError {
    /// A file operation failed.
    Io(std::io::Error),
    /// The JSON text is not valid.
    Json(serde_json::Error),
    /// The profile breaks a rule.
    Invalid(String),
    /// No profile has this name.
    NotFound(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "file error: {e}"),
            StoreError::Json(e) => write!(f, "profile JSON error: {e}"),
            StoreError::Invalid(m) => write!(f, "invalid profile: {m}"),
            StoreError::NotFound(name) => write!(f, "no profile named {name:?}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Io(e) => Some(e),
            StoreError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Json(e)
    }
}

/// Result alias for the store.
pub type Result<T> = std::result::Result<T, StoreError>;

/// A folder of profile files.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Use this folder. The folder is created on the first save.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The user profile folder: `%LOCALAPPDATA%\StreamDock\profiles`.
    pub fn default_dir() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("StreamDock").join("profiles")
    }

    /// The folder in use.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path of one profile file. Rejects names that are not a plain file name.
    pub fn path_for(&self, name: &str) -> Result<PathBuf> {
        if name.trim().is_empty() {
            return Err(StoreError::Invalid("profile name is empty".into()));
        }
        let safe = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_');
        if !safe {
            return Err(StoreError::Invalid(format!(
                "profile name {name:?} may hold only letters, digits, space, - and _"
            )));
        }
        Ok(self.dir.join(format!("{name}.json")))
    }

    /// Every profile name in the folder, sorted.
    pub fn list(&self) -> Result<Vec<String>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Read one profile and check it.
    pub fn load(&self, name: &str) -> Result<Profile> {
        let path = self.path_for(name)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::NotFound(name.to_string()));
            }
            Err(e) => return Err(e.into()),
        };

        let profile: Profile = serde_json::from_str(&text)?;
        if profile.version > VERSION {
            return Err(StoreError::Invalid(format!(
                "profile version {} is newer than this program ({VERSION})",
                profile.version
            )));
        }
        profile.validate().map_err(StoreError::Invalid)?;
        Ok(profile)
    }

    /// Write one profile. The profile is checked first.
    pub fn save(&self, profile: &Profile) -> Result<()> {
        profile.validate().map_err(StoreError::Invalid)?;
        let path = self.path_for(&profile.name)?;
        std::fs::create_dir_all(&self.dir)?;

        let text = serde_json::to_string_pretty(profile)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, text.as_bytes())?;
        std::fs::rename(&temp, &path)?;
        Ok(())
    }

    /// Load a profile, or write the starter profile when the file is absent.
    pub fn load_or_create(&self, name: &str) -> Result<Profile> {
        match self.load(name) {
            Ok(profile) => Ok(profile),
            Err(StoreError::NotFound(_)) => {
                let mut profile = Profile::starter();
                profile.name = name.to_string();
                self.save(&profile)?;
                Ok(profile)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Action, Key, Page};
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "streamdock-store-test-{}-{tag}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> Profile {
        let mut profile = Profile::new("RoundTrip");
        profile.pages[0].keys.insert(
            1,
            Key {
                label: Some("One".into()),
                actions: vec![Action::Page {
                    target: "Main".into(),
                }],
                ..Key::default()
            },
        );
        profile.pages.push(Page::new("Second"));
        profile
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = temp_dir("roundtrip");
        let store = Store::new(&dir);
        let profile = sample();
        store.save(&profile).expect("save");
        let back = store.load("RoundTrip").expect("load");
        assert_eq!(profile, back);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_finds_json_files_only() {
        let dir = temp_dir("list");
        let store = Store::new(&dir);
        store.save(&sample()).expect("save");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("notes.txt"), b"ignore me").expect("write");
        assert_eq!(store.list().expect("list"), vec!["RoundTrip".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_folder_lists_empty() {
        let dir = temp_dir("missing");
        let store = Store::new(&dir);
        assert!(store.list().expect("list").is_empty());
        assert!(matches!(
            store.load("Nope"),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn bad_names_are_rejected() {
        let store = Store::new(temp_dir("names"));
        for name in ["../evil", "a/b", "a\\b", "a:b", ""] {
            assert!(store.path_for(name).is_err(), "name {name:?}");
        }
        assert!(store.path_for("Good - name_1").is_ok());
    }

    #[test]
    fn save_rejects_an_invalid_profile() {
        let dir = temp_dir("invalid");
        let store = Store::new(&dir);
        let mut profile = sample();
        profile.pages.clear();
        assert!(matches!(store.save(&profile), Err(StoreError::Invalid(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_create_writes_the_starter() {
        let dir = temp_dir("create");
        let store = Store::new(&dir);
        let profile = store.load_or_create("Fresh").expect("create");
        assert_eq!(profile.name, "Fresh");
        assert!(dir.join("Fresh.json").exists());
        let again = store.load_or_create("Fresh").expect("load");
        assert_eq!(profile, again);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_version_is_rejected() {
        let dir = temp_dir("version");
        let store = Store::new(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let text = format!(
            r#"{{ "version": {}, "name": "Future", "pages": [ {{ "name": "Main" }} ] }}"#,
            VERSION + 1
        );
        std::fs::write(dir.join("Future.json"), text).expect("write");
        assert!(matches!(
            store.load("Future"),
            Err(StoreError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_leaves_no_temp_file() {
        let dir = temp_dir("temp");
        let store = Store::new(&dir);
        store.save(&sample()).expect("save");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
