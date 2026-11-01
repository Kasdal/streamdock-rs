//! One JSON file per profile.
//!
//! The folder holds `Name.json` and an optional `assets` folder. A save writes
//! a temporary file and renames it over the old one, so a crash cannot leave a
//! half written profile.

use std::collections::BTreeSet;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

use crate::profile::{ImageSpec, Profile, VERSION};

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

    /// Write a profile and its assets into one zip file.
    ///
    /// Returns the number of asset files that were added. An asset that is
    /// missing on disk is skipped, so an export never fails on a stale path.
    pub fn export(&self, name: &str, out: &Path) -> Result<usize> {
        let (bytes, count) = self.export_bytes(name)?;
        std::fs::write(out, bytes)?;
        Ok(count)
    }

    /// Write a profile and its assets into one zip in memory.
    pub fn export_bytes(&self, name: &str) -> Result<(Vec<u8>, usize)> {
        let profile = self.load(name)?;
        let text = serde_json::to_string_pretty(&profile)?;

        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file(format!("{}.json", profile.name), options)
            .map_err(zip_error)?;
        zip.write_all(text.as_bytes())?;

        let mut count = 0;
        for path in asset_paths(&profile) {
            let source = self.dir.join(&path);
            if !source.is_file() {
                continue;
            }
            zip.start_file(path.clone(), options).map_err(zip_error)?;
            let bytes = std::fs::read(&source)?;
            zip.write_all(&bytes)?;
            count += 1;
        }

        let cursor = zip.finish().map_err(zip_error)?;
        Ok((cursor.into_inner(), count))
    }

    /// Read a profile and its assets from one zip file.
    ///
    /// Only entries under `assets/` and `icons/` are written, so an archive
    /// cannot place a file somewhere else on the disk.
    pub fn import(&self, archive: &Path, new_name: Option<&str>) -> Result<Profile> {
        let file = std::fs::File::open(archive)?;
        self.import_from(file, new_name)
    }

    /// Read a profile and its assets from zip bytes.
    pub fn import_bytes(&self, bytes: &[u8], new_name: Option<&str>) -> Result<Profile> {
        self.import_from(std::io::Cursor::new(bytes), new_name)
    }

    fn import_from<R: Read + std::io::Seek>(
        &self,
        reader: R,
        new_name: Option<&str>,
    ) -> Result<Profile> {
        let mut zip = zip::ZipArchive::new(reader).map_err(zip_error)?;

        let mut profile_text: Option<String> = None;
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();

        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(zip_error)?;
            if !entry.is_file() {
                continue;
            }
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;

            if profile_text.is_none() && name.ends_with(".json") {
                profile_text = match String::from_utf8(bytes) {
                    Ok(text) => Some(text),
                    Err(_) => {
                        return Err(StoreError::Invalid(
                            "the profile JSON in the archive is not text".into(),
                        ))
                    }
                };
            } else if safe_relative(&name) {
                files.push((name, bytes));
            }
        }

        let text = profile_text
            .ok_or_else(|| StoreError::Invalid("the archive has no profile JSON".into()))?;
        let mut profile: Profile = serde_json::from_str(&text)?;
        if let Some(name) = new_name {
            profile.name = name.to_string();
        }
        profile.validate().map_err(StoreError::Invalid)?;

        for (name, bytes) in files {
            let path = self.dir.join(&name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, bytes)?;
        }

        self.save(&profile)?;
        Ok(profile)
    }
}

/// Every relative asset path that a profile refers to.
fn asset_paths(profile: &Profile) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();

    for page in &profile.pages {
        for key in page.keys.values() {
            match &key.image {
                Some(ImageSpec::File { path })
                | Some(ImageSpec::Svg { path, .. })
                | Some(ImageSpec::Animation { path, .. }) => {
                    if safe_relative(path) {
                        paths.insert(path.clone());
                    }
                }
                Some(ImageSpec::Icon { name, .. }) => {
                    let path = format!("icons/{name}.svg");
                    if safe_relative(&path) {
                        paths.insert(path);
                    }
                }
                _ => {}
            }
        }
    }

    paths
}

/// True for `assets/...` or `icons/...` with no parent directory step.
fn safe_relative(name: &str) -> bool {
    let mut parts = Path::new(name).components();

    let first = match parts.next() {
        Some(std::path::Component::Normal(part)) => part.to_str().unwrap_or(""),
        _ => return false,
    };
    if first != "assets" && first != "icons" {
        return false;
    }

    let mut depth = 1;
    for part in parts {
        match part {
            std::path::Component::Normal(_) => depth += 1,
            _ => return false,
        }
    }

    depth >= 2
}

fn zip_error(error: zip::result::ZipError) -> StoreError {
    StoreError::Invalid(format!("zip: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Action, ImageSpec, Key, Page};
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

    #[test]
    fn export_and_import_round_trip() {
        let dir = temp_dir("export");
        let store = Store::new(&dir);
        let mut profile = sample();
        std::fs::create_dir_all(dir.join("assets")).expect("assets");
        std::fs::write(dir.join("assets").join("pic.jpg"), b"fake image").expect("write");
        profile.pages[0].keys.get_mut(&1).expect("key 1").image = Some(ImageSpec::File {
            path: "assets/pic.jpg".into(),
        });
        store.save(&profile).expect("save");

        let archive = dir.join("out.zip");
        let count = store.export(&profile.name, &archive).expect("export");
        assert_eq!(count, 1);

        let other = temp_dir("import");
        let store2 = Store::new(&other);
        let imported = store2.import(&archive, Some("Copy")).expect("import");
        assert_eq!(imported.name, "Copy");
        assert!(other.join("assets").join("pic.jpg").exists());
        assert!(store2.load("Copy").is_ok());

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn only_asset_and_icon_paths_are_safe() {
        assert!(safe_relative("assets/a.png"));
        assert!(safe_relative("icons/mic.svg"));
        assert!(!safe_relative("assets/../evil.png"));
        assert!(!safe_relative("/assets/a.png"));
        assert!(!safe_relative("other/a.png"));
        assert!(!safe_relative("assets"));
        assert!(!safe_relative("C:/assets/a.png"));
    }

    #[test]
    fn a_zip_in_memory_round_trips() {
        let dir = temp_dir("zip-bytes");
        let store = Store::new(&dir);
        let profile = sample();
        store.save(&profile).expect("save");

        let (bytes, count) = store.export_bytes(&profile.name).expect("export");
        assert_eq!(count, 0);
        assert!(bytes.starts_with(b"PK"));

        let other = temp_dir("zip-bytes-2");
        let store2 = Store::new(&other);
        let imported = store2.import_bytes(&bytes, Some("FromBytes")).expect("import");
        assert_eq!(imported.name, "FromBytes");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }
}
