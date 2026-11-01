//! Profile data model.
//!
//! One profile is one JSON file plus optional asset files. The model is version
//! tagged so a later release can migrate old files.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Default",
//!   "device": { "pid": "0x3010", "layout": "akp153e" },
//!   "pages": [
//!     {
//!       "name": "Main",
//!       "keys": {
//!         "1": {
//!           "label": "Mute",
//!           "image": { "kind": "text", "text": "MUTE" },
//!           "actions": [ { "type": "hotkey", "keys": ["ctrl", "shift", "m"] } ]
//!         }
//!       }
//!     }
//!   ]
//! }
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use streamdock_core::model;

/// Model version of this file format.
pub const VERSION: u32 = 1;

/// Deepest allowed nesting of a `multi` action.
pub const MAX_ACTION_DEPTH: u32 = 8;

/// Layout name for the 18 key unit.
pub const LAYOUT_AKP153E: &str = "akp153e";

/// One profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// File format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Profile name. Also the file name without the extension.
    pub name: String,
    /// Device that the profile targets.
    #[serde(default)]
    pub device: DeviceSpec,
    /// Pages in display order.
    #[serde(default)]
    pub pages: Vec<Page>,
}

/// Device binding of a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceSpec {
    /// Product ID as text, for example `0x3010`.
    #[serde(default = "default_pid")]
    pub pid: String,
    /// Layout name from `model`.
    #[serde(default = "default_layout")]
    pub layout: String,
}

impl Default for DeviceSpec {
    fn default() -> Self {
        Self {
            pid: default_pid(),
            layout: default_layout(),
        }
    }
}

impl DeviceSpec {
    /// Product ID as a number.
    pub fn product_id(&self) -> Option<u16> {
        let text = self.pid.trim();
        let text = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"));
        match text {
            Some(hex) => u16::from_str_radix(hex, 16).ok(),
            None => self.pid.trim().parse().ok(),
        }
    }
}

/// One page of keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// Page name, unique inside the profile.
    pub name: String,
    /// Key number to key configuration. Key numbers are 1 based.
    #[serde(default)]
    pub keys: BTreeMap<u8, Key>,
}

impl Page {
    /// An empty page with this name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            keys: BTreeMap::new(),
        }
    }

    /// Key configuration for a key number.
    pub fn key(&self, key: u8) -> Option<&Key> {
        self.keys.get(&key)
    }
}

/// One key: how it looks and what it does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Optional text label. An image replaces it when both exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// How the tile is drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageSpec>,
    /// Actions that run on a press, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
}

/// How a key tile is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSpec {
    /// One solid colour.
    Color {
        #[serde(default = "default_background")]
        color: String,
    },
    /// Centred uppercase text on a background.
    Text {
        text: String,
        #[serde(default = "default_background")]
        background: String,
        #[serde(default = "default_foreground")]
        foreground: String,
    },
    /// A JPEG or PNG file on disk.
    File { path: String },
    /// An animated GIF file on disk.
    Animation { path: String },
    /// A named icon on a background. The icon library arrives in M4.
    Icon {
        name: String,
        #[serde(default = "default_background")]
        background: String,
    },
}

/// One action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Press a key combination, for example `["ctrl", "shift", "m"]`.
    Hotkey { keys: Vec<String> },
    /// Type a string. The delay is between characters in milliseconds.
    Text {
        text: String,
        #[serde(default)]
        delay_ms: u64,
    },
    /// Send one media key.
    Media { command: MediaCommand },
    /// Open a file, a folder or a URL.
    Launch { target: String },
    /// Run several actions in order.
    Multi {
        actions: Vec<Action>,
        #[serde(default)]
        pause_ms: u64,
    },
    /// Switch to another page by name.
    Page { target: String },
    /// Set the panel brightness.
    Brightness { value: u8 },
    /// Run a program. The user must opt in, because it runs arbitrary code.
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

/// Media keys that Windows understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    Stop,
    VolumeUp,
    VolumeDown,
    Mute,
}

/// Parse a colour such as `#20a0ff` or `#2af`.
pub fn parse_color(text: &str) -> Result<[u8; 3], String> {
    let hex = text.trim().strip_prefix('#').unwrap_or(text.trim());
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(|| format!("bad colour: {text}"))?;

    match digits.len() {
        3 => Ok([digits[0] * 17, digits[1] * 17, digits[2] * 17]),
        6 => Ok([
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
        ]),
        _ => Err(format!("bad colour: {text}")),
    }
}

impl Profile {
    /// Find a page by name.
    pub fn page(&self, name: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.name == name)
    }

    /// Index of a page by name.
    pub fn page_index(&self, name: &str) -> Option<usize> {
        self.pages.iter().position(|p| p.name == name)
    }

    /// A new profile with one empty page.
    pub fn new(name: &str) -> Self {
        Self {
            version: VERSION,
            name: name.to_string(),
            device: DeviceSpec::default(),
            pages: vec![Page::new("Main")],
        }
    }

    /// A starter profile with numbered keys, so a new install shows a layout.
    pub fn starter() -> Self {
        let mut page = Page::new("Main");
        for key in 1..=model::KEY_COUNT {
            page.keys.insert(
                key,
                Key {
                    label: Some(key.to_string()),
                    image: None,
                    actions: Vec::new(),
                },
            );
        }
        Self {
            version: VERSION,
            name: "Default".to_string(),
            device: DeviceSpec::default(),
            pages: vec![page],
        }
    }

    /// A two page demo: page Main switches to Second on key 1 and back again.
    ///
    /// Use it to check the daemon, the profile store and the action engine
    /// without the UI.
    pub fn demo() -> Self {
        let mut main = Page::new("Main");
        for key in 1..=model::KEY_COUNT {
            let label = if key <= model::GRID_KEYS {
                key.to_string()
            } else {
                format!("S{}", key - model::GRID_KEYS)
            };
            main.keys.insert(
                key,
                Key {
                    label: Some(label),
                    image: None,
                    actions: Vec::new(),
                },
            );
        }
        main.keys.get_mut(&1).map(|k| {
            k.actions = vec![Action::Page {
                target: "Second".to_string(),
            }];
        });

        let mut second = Page::new("Second");
        for key in 1..=model::KEY_COUNT {
            let text = if key <= model::GRID_KEYS {
                format!("S{key}")
            } else {
                format!("S{}", key - model::GRID_KEYS)
            };
            second.keys.insert(
                key,
                Key {
                    label: None,
                    image: Some(ImageSpec::Text {
                        text,
                        background: "#14375c".to_string(),
                        foreground: "#ffffff".to_string(),
                    }),
                    actions: Vec::new(),
                },
            );
        }
        second.keys.get_mut(&1).map(|k| {
            k.actions = vec![Action::Page {
                target: "Main".to_string(),
            }];
        });

        Self {
            version: VERSION,
            name: "Demo".to_string(),
            device: DeviceSpec::default(),
            pages: vec![main, second],
        }
    }

    /// Check the file for mistakes before the daemon uses it.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("profile name is empty".into());
        }
        if self.pages.is_empty() {
            return Err("profile has no pages".into());
        }

        let mut seen = std::collections::BTreeSet::new();
        for page in &self.pages {
            if page.name.trim().is_empty() {
                return Err("a page name is empty".into());
            }
            if !seen.insert(page.name.as_str()) {
                return Err(format!("duplicate page name: {}", page.name));
            }
            for (&key, config) in &page.keys {
                if !model::is_valid_key(key) {
                    return Err(format!(
                        "page {} has key {key}, the limit is 1..={}",
                        page.name,
                        model::KEY_COUNT
                    ));
                }
                if let Some(image) = &config.image {
                    image.validate()?;
                }
                for action in &config.actions {
                    validate_action(action, 1)?;
                }
            }
        }

        Ok(())
    }
}

impl ImageSpec {
    fn validate(&self) -> Result<(), String> {
        match self {
            ImageSpec::Color { color } => parse_color(color).map(|_| ()),
            ImageSpec::Text {
                text,
                background,
                foreground,
            } => {
                parse_color(background)?;
                parse_color(foreground)?;
                if text.chars().count() > 64 {
                    return Err("text label is longer than 64 characters".into());
                }
                Ok(())
            }
            ImageSpec::File { path } => {
                if path.trim().is_empty() {
                    return Err("image file path is empty".into());
                }
                Ok(())
            }
            ImageSpec::Animation { path } => {
                if path.trim().is_empty() {
                    return Err("animation file path is empty".into());
                }
                Ok(())
            }
            ImageSpec::Icon { name, background } => {
                parse_color(background)?;
                if name.trim().is_empty() {
                    return Err("icon name is empty".into());
                }
                Ok(())
            }
        }
    }
}

impl Action {
    /// A one line description for logs.
    pub fn describe(&self) -> String {
        match self {
            Action::Hotkey { keys } => format!("hotkey {}", keys.join("+")),
            Action::Text { text, .. } => format!("text {:?}", truncate(text, 24)),
            Action::Media { command } => format!("media {command:?}"),
            Action::Launch { target } => format!("launch {target}"),
            Action::Multi { actions, .. } => format!("multi with {} actions", actions.len()),
            Action::Page { target } => format!("page {target}"),
            Action::Brightness { value } => format!("brightness {value}"),
            Action::Command { program, .. } => format!("command {program}"),
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "..."
}

fn validate_action(action: &Action, depth: u32) -> Result<(), String> {
    if depth > MAX_ACTION_DEPTH {
        return Err(format!("action nesting is deeper than {MAX_ACTION_DEPTH}"));
    }
    match action {
        Action::Hotkey { keys } => {
            if keys.is_empty() {
                return Err("hotkey has no keys".into());
            }
            if keys.len() > 6 {
                return Err("hotkey has more than 6 keys".into());
            }
        }
        Action::Text { text, .. } => {
            if text.is_empty() {
                return Err("text action is empty".into());
            }
            if text.chars().count() > 4096 {
                return Err("text action is longer than 4096 characters".into());
            }
        }
        Action::Launch { target } => {
            if target.trim().is_empty() {
                return Err("launch target is empty".into());
            }
        }
        Action::Multi { actions, .. } => {
            if actions.is_empty() {
                return Err("multi action has no actions".into());
            }
            for inner in actions {
                validate_action(inner, depth + 1)?;
            }
        }
        Action::Page { target } => {
            if target.trim().is_empty() {
                return Err("page target is empty".into());
            }
        }
        Action::Brightness { value } => {
            if *value > 100 {
                return Err("brightness is above 100".into());
            }
        }
        Action::Command { program, .. } => {
            if program.trim().is_empty() {
                return Err("command program is empty".into());
            }
        }
        Action::Media { .. } => {}
    }
    Ok(())
}

fn default_version() -> u32 {
    VERSION
}

fn default_pid() -> String {
    "0x3010".to_string()
}

fn default_layout() -> String {
    LAYOUT_AKP153E.to_string()
}

fn default_background() -> String {
    "#202020".to_string()
}

fn default_foreground() -> String {
    "#f0f0f0".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profile {
        let mut page = Page::new("Main");
        page.keys.insert(
            1,
            Key {
                label: Some("Mute".into()),
                image: Some(ImageSpec::Text {
                    text: "MUTE".into(),
                    background: "#101010".into(),
                    foreground: "#ffffff".into(),
                }),
                actions: vec![Action::Hotkey {
                    keys: vec!["ctrl".into(), "shift".into(), "m".into()],
                }],
            },
        );
        page.keys.insert(
            16,
            Key {
                label: None,
                image: Some(ImageSpec::Color {
                    color: "#204060".into(),
                }),
                actions: vec![],
            },
        );
        Profile {
            version: VERSION,
            name: "Default".into(),
            device: DeviceSpec::default(),
            pages: vec![page, Page::new("Media")],
        }
    }

    #[test]
    fn profile_round_trips_through_json() {
        let profile = sample();
        let text = serde_json::to_string_pretty(&profile).expect("serialize");
        let back: Profile = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(profile, back);
    }

    #[test]
    fn key_numbers_are_json_strings() {
        let text = serde_json::to_string(&sample()).expect("serialize");
        assert!(text.contains("\"1\":"));
        assert!(text.contains("\"16\":"));
    }

    #[test]
    fn defaults_fill_missing_fields() {
        let text = r#"{
            "name": "Test",
            "pages": [ { "name": "Main", "keys": {
                "3": { "label": "3", "actions": [ { "type": "page", "target": "Main" } ] }
            } } ]
        }"#;
        let profile: Profile = serde_json::from_str(text).expect("deserialize");
        assert_eq!(profile.version, VERSION);
        assert_eq!(profile.device.pid, "0x3010");
        assert_eq!(profile.device.layout, LAYOUT_AKP153E);
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn product_id_accepts_hex_and_decimal() {
        let mut spec = DeviceSpec::default();
        assert_eq!(spec.product_id(), Some(0x3010));
        spec.pid = "12304".into();
        assert_eq!(spec.product_id(), Some(0x3010));
        spec.pid = "nonsense".into();
        assert_eq!(spec.product_id(), None);
    }

    #[test]
    fn colours_are_parsed() {
        assert_eq!(parse_color("#000000"), Ok([0, 0, 0]));
        assert_eq!(parse_color("#ffffff"), Ok([255, 255, 255]));
        assert_eq!(parse_color("#2af"), Ok([34, 170, 255]));
        assert_eq!(parse_color("20a0ff"), Ok([32, 160, 255]));
        assert!(parse_color("#12").is_err());
        assert!(parse_color("#gggggg").is_err());
    }

    #[test]
    fn validate_rejects_duplicate_pages() {
        let mut profile = Profile::new("Test");
        profile.pages.push(Page::new("Main"));
        assert!(profile.validate().is_err());
    }

    #[test]
    fn validate_rejects_a_bad_key_number() {
        let mut profile = Profile::new("Test");
        profile.pages[0].keys.insert(19, Key::default());
        let err = profile.validate().expect_err("key 19 is invalid");
        assert!(err.contains("19"));
    }

    #[test]
    fn validate_rejects_deep_nesting() {
        let mut action = Action::Hotkey {
            keys: vec!["a".into()],
        };
        for _ in 0..(MAX_ACTION_DEPTH + 1) {
            action = Action::Multi {
                actions: vec![action],
                pause_ms: 0,
            };
        }
        let mut profile = Profile::new("Test");
        profile.pages[0].keys.insert(
            1,
            Key {
                actions: vec![action],
                ..Key::default()
            },
        );
        assert!(profile.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_colours() {
        let mut profile = Profile::new("Test");
        profile.pages[0].keys.insert(
            1,
            Key {
                image: Some(ImageSpec::Color {
                    color: "not-a-colour".into(),
                }),
                ..Key::default()
            },
        );
        assert!(profile.validate().is_err());
    }

    #[test]
    fn validate_accepts_a_well_formed_profile() {
        assert!(sample().validate().is_ok());
        assert!(Profile::starter().validate().is_ok());
    }

    #[test]
    fn starter_covers_every_key() {
        let profile = Profile::starter();
        let page = profile.page("Main").expect("Main page");
        assert_eq!(page.keys.len(), model::KEY_COUNT as usize);
        for key in 1..=model::KEY_COUNT {
            assert_eq!(page.keys[&key].label.as_deref(), Some(key.to_string().as_str()));
        }
    }

    #[test]
    fn demo_has_two_pages_and_valid_actions() {
        let profile = Profile::demo();
        assert_eq!(profile.pages.len(), 2);
        assert!(profile.validate().is_ok());
        let main = profile.page("Main").expect("Main");
        assert_eq!(
            main.keys[&1].actions,
            vec![Action::Page {
                target: "Second".into()
            }]
        );
        let second = profile.page("Second").expect("Second");
        assert_eq!(second.keys.len(), model::KEY_COUNT as usize);
    }

    #[test]
    fn page_lookup_works() {
        let profile = sample();
        assert!(profile.page("Media").is_some());
        assert_eq!(profile.page_index("Media"), Some(1));
        assert!(profile.page("Missing").is_none());
    }

    #[test]
    fn action_descriptions_are_short() {
        let action = Action::Text {
            text: "x".repeat(100),
            delay_ms: 0,
        };
        assert!(action.describe().len() < 40);
    }
}
