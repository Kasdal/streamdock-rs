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
    /// Colour scheme for keys without an explicit colour.
    #[serde(default)]
    pub theme: Theme,
    /// Process names that switch to this profile, for example `obs64.exe`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apps: Vec<String>,
    /// Widgets of the side strip.
    #[serde(default)]
    pub screen: Screen,
    /// Pages in display order.
    #[serde(default)]
    pub pages: Vec<Page>,
}

/// Colours that a key uses when its image does not set one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_foreground")]
    pub foreground: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: default_background(),
            foreground: default_foreground(),
        }
    }
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

/// The side strip: three widgets, one per third of the display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    /// Widgets from the top. The list holds at most three.
    #[serde(default = "default_slots")]
    pub slots: Vec<Slot>,
    /// Size of the text, 1.0 is the normal size.
    #[serde(default = "default_text_scale")]
    pub text_scale: f32,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            slots: default_slots(),
            text_scale: default_text_scale(),
        }
    }
}

/// Normal text size on the strip.
pub const TEXT_SCALE_DEFAULT: f32 = 1.0;

/// Smallest and largest text size on the strip.
pub const TEXT_SCALE_MIN: f32 = 0.6;
/// Largest text size on the strip.
pub const TEXT_SCALE_MAX: f32 = 1.6;

fn default_text_scale() -> f32 {
    TEXT_SCALE_DEFAULT
}

fn default_slots() -> Vec<Slot> {
    vec![
        Slot::Meter {
            metric: Metric::Cpu,
        },
        Slot::Meter {
            metric: Metric::Gpu,
        },
        Slot::Meter {
            metric: Metric::Memory,
        },
    ]
}

/// One widget of the side strip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Slot {
    /// A load meter with a bar.
    Meter {
        #[serde(default)]
        metric: Metric,
    },
    /// Time, date, or both.
    Clock {
        #[serde(default)]
        style: ClockStyle,
    },
    /// The weather for one place.
    Weather {
        city: String,
        #[serde(default)]
        latitude: f64,
        #[serde(default)]
        longitude: f64,
        #[serde(default)]
        units: Units,
    },
    /// Nothing at all.
    Empty,
}

/// What a meter shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    #[default]
    Cpu,
    Gpu,
    Memory,
    Disk,
}

/// What a clock shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockStyle {
    /// The time over the date.
    #[default]
    Both,
    Time,
    Date,
}

/// Units of the weather widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
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
    /// An SVG file on disk.
    Svg {
        path: String,
        #[serde(default = "default_background")]
        background: String,
    },
    /// An animated GIF file on disk.
    Animation {
        path: String,
        /// Start offset in milliseconds. When absent, each key starts at its
        /// own offset so a page of animations does not blink together.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase_ms: Option<u64>,
    },
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
    /// Send one HTTP request. Only `http://` is supported.
    Http {
        #[serde(default = "default_method")]
        method: String,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Call one external plugin.
    Plugin {
        id: String,
        action: String,
        #[serde(default)]
        settings: serde_json::Value,
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

/// A key with a bundled icon.
fn icon_key(name: &str, actions: Vec<Action>) -> Key {
    Key {
        label: None,
        image: Some(ImageSpec::Icon {
            name: name.to_string(),
            background: "#14375c".to_string(),
        }),
        actions,
    }
}

/// A key with a text label.
fn plain_key(label: &str, actions: Vec<Action>) -> Key {
    Key {
        label: Some(label.to_string()),
        image: None,
        actions,
    }
}

/// One key combination.
fn hotkey(keys: &[&str]) -> Action {
    Action::Hotkey {
        keys: keys.iter().map(|key| key.to_string()).collect(),
    }
}

/// Add the four page keys to the bottom row of a page.
fn add_navigation(page: &mut Page, _current: &str) {
    let navigation = [
        (11u8, "MEDIA", "Media"),
        (12, "SYSTEM", "System"),
        (13, "APPS", "Apps"),
        (14, "TOOLS", "Tools"),
    ];
    for (key, label, target) in navigation {
        page.keys.insert(
            key,
            plain_key(label, vec![Action::Page {
                target: target.to_string(),
            }]),
        );
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
            theme: Theme::default(),
            apps: Vec::new(),
            screen: Screen::default(),
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
            theme: Theme::default(),
            apps: Vec::new(),
            screen: Screen::default(),
            pages: vec![page],
        }
    }

    /// The pages that a new install starts with: the tools that a Stream Deck
    /// usually ships with.
    ///
    /// Media, System, Apps and Tools. The bottom row of every page switches
    /// page, so a user never gets lost.
    pub fn tools() -> Self {
        let mut media = Page::new("Media");
        media.keys.insert(
            1,
            icon_key("prev", vec![Action::Media {
                command: MediaCommand::Previous,
            }]),
        );
        media.keys.insert(
            2,
            icon_key("play", vec![Action::Media {
                command: MediaCommand::PlayPause,
            }]),
        );
        media.keys.insert(
            3,
            icon_key("next", vec![Action::Media {
                command: MediaCommand::Next,
            }]),
        );
        media.keys.insert(
            4,
            icon_key("stop", vec![Action::Media {
                command: MediaCommand::Stop,
            }]),
        );
        media.keys.insert(
            5,
            icon_key("mute", vec![Action::Media {
                command: MediaCommand::Mute,
            }]),
        );
        media.keys.insert(
            6,
            icon_key("volume-down", vec![Action::Media {
                command: MediaCommand::VolumeDown,
            }]),
        );
        media.keys.insert(
            7,
            icon_key("volume", vec![Action::Media {
                command: MediaCommand::VolumeUp,
            }]),
        );
        add_navigation(&mut media, "Media");

        let mut system = Page::new("System");
        system
            .keys
            .insert(1, icon_key("lock", vec![hotkey(&["win", "l"])]));
        system
            .keys
            .insert(2, icon_key("screen", vec![hotkey(&["win", "d"])]));
        system.keys.insert(
            3,
            icon_key("terminal", vec![hotkey(&["ctrl", "shift", "escape"])]),
        );
        system.keys.insert(
            4,
            icon_key("camera", vec![hotkey(&["win", "shift", "s"])]),
        );
        system
            .keys
            .insert(5, icon_key("copy", vec![hotkey(&["ctrl", "c"])]));
        system
            .keys
            .insert(6, icon_key("paste", vec![hotkey(&["ctrl", "v"])]));
        system
            .keys
            .insert(7, icon_key("undo", vec![hotkey(&["ctrl", "z"])]));
        system
            .keys
            .insert(8, icon_key("redo", vec![hotkey(&["ctrl", "y"])]));
        system
            .keys
            .insert(9, icon_key("save", vec![hotkey(&["ctrl", "s"])]));
        system
            .keys
            .insert(10, icon_key("select", vec![hotkey(&["ctrl", "a"])]));
        add_navigation(&mut system, "System");

        let mut apps = Page::new("Apps");
        apps.keys.insert(
            1,
            icon_key("folder", vec![Action::Launch {
                target: "explorer.exe".into(),
            }]),
        );
        apps.keys.insert(
            2,
            icon_key("edit", vec![Action::Launch {
                target: "notepad.exe".into(),
            }]),
        );
        apps.keys.insert(
            3,
            icon_key("calculator", vec![Action::Launch {
                target: "calc.exe".into(),
            }]),
        );
        apps.keys.insert(
            4,
            icon_key("brush", vec![Action::Launch {
                target: "mspaint.exe".into(),
            }]),
        );
        apps.keys.insert(
            5,
            icon_key("gear", vec![Action::Launch {
                target: "ms-settings:".into(),
            }]),
        );
        apps.keys.insert(
            6,
            icon_key("terminal", vec![Action::Launch {
                target: "cmd.exe".into(),
            }]),
        );
        apps.keys.insert(
            7,
            icon_key("browser", vec![Action::Launch {
                target: "https://www.google.com".into(),
            }]),
        );
        apps.keys.insert(
            8,
            icon_key("mail", vec![Action::Launch {
                target: "mailto:".into(),
            }]),
        );
        add_navigation(&mut apps, "Apps");

        let mut tools = Page::new("Tools");
        tools
            .keys
            .insert(1, icon_key("minus", vec![Action::Brightness { value: 30 }]));
        tools
            .keys
            .insert(2, icon_key("plus", vec![Action::Brightness { value: 70 }]));
        tools
            .keys
            .insert(3, icon_key("check", vec![Action::Brightness { value: 100 }]));
        tools.keys.insert(
            4,
            icon_key("camera", vec![hotkey(&["win", "shift", "s"])]),
        );
        tools.keys.insert(
            5,
            plain_key(
                "TYPE",
                vec![Action::Text {
                    text: "Hello from Stream Dock".into(),
                    delay_ms: 20,
                }],
            ),
        );
        tools.keys.insert(
            6,
            plain_key(
                "RUN 2",
                vec![Action::Multi {
                    actions: vec![
                        Action::Text {
                            text: "Stream Dock".into(),
                            delay_ms: 20,
                        },
                        hotkey(&["enter"]),
                    ],
                    pause_ms: 300,
                }],
            ),
        );
        add_navigation(&mut tools, "Tools");

        Self {
            version: VERSION,
            name: "Default".to_string(),
            device: DeviceSpec::default(),
            theme: Theme::default(),
            apps: Vec::new(),
            screen: Screen::default(),
            pages: vec![media, system, apps, tools],
        }
    }

    /// A two page demo: page Main switches to Second on key 1 and back again.
    ///
    /// Use it to check the daemon, the profile store and the action engine
    /// without the UI.
    pub fn demo() -> Self {        let mut main = Page::new("Main");
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
            theme: Theme::default(),
            apps: Vec::new(),
            screen: Screen::default(),
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

        parse_color(&self.theme.background)?;
        parse_color(&self.theme.foreground)?;

        if self.screen.slots.len() > model::STRIP_SLOTS {
            return Err(format!(
                "the side strip has {} widgets, the limit is {}",
                self.screen.slots.len(),
                model::STRIP_SLOTS
            ));
        }
        if !(TEXT_SCALE_MIN..=TEXT_SCALE_MAX).contains(&self.screen.text_scale) {
            return Err(format!(
                "the strip text size {} is outside {TEXT_SCALE_MIN}..={TEXT_SCALE_MAX}",
                self.screen.text_scale
            ));
        }
        for slot in &self.screen.slots {
            slot.validate()?;
        }

        for app in &self.apps {
            if app.trim().is_empty() {
                return Err("an app name is empty".into());
            }
            if app.chars().count() > 64 {
                return Err(format!("app name {app:?} is longer than 64 characters"));
            }
            if app.contains('\\') || app.contains('/') {
                return Err(format!("app name {app:?} must be a process name, not a path"));
            }
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
    fn validate(&self) -> Result<(), String> {        match self {
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
            ImageSpec::Svg { path, background } => {
                parse_color(background)?;
                if path.trim().is_empty() {
                    return Err("svg file path is empty".into());
                }
                Ok(())
            }
            ImageSpec::Animation { path, .. } => {
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
            Action::Http { method, url, .. } => format!("http {method} {url}"),
            Action::Plugin { id, action, .. } => format!("plugin {id}/{action}"),
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "..."
}

impl Slot {
    /// Check one strip widget.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Slot::Weather {
                city,
                latitude,
                longitude,
                ..
            } => {
                let city = city.trim();
                if city.is_empty() {
                    return Err("a weather widget needs a place".into());
                }
                if city.chars().count() > 48 {
                    return Err(format!("place name {city:?} is longer than 48 characters"));
                }
                if city.chars().any(|c| c.is_control()) {
                    return Err("place name has a control character".into());
                }
                if !latitude.is_finite() || !(-90.0..=90.0).contains(latitude) {
                    return Err(format!("latitude {latitude} is outside -90..=90"));
                }
                if !longitude.is_finite() || !(-180.0..=180.0).contains(longitude) {
                    return Err(format!("longitude {longitude} is outside -180..=180"));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
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
        Action::Http { method, url, .. } => {
            let method = method.to_ascii_uppercase();
            if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "DELETE") {
                return Err(format!("HTTP method {method} is not supported"));
            }
            if !url.starts_with("http://") {
                return Err(format!(
                    "HTTP URL {url:?} must start with http://. HTTPS is not supported yet."
                ));
            }
        }
        Action::Plugin { id, action, .. } => {
            if id.trim().is_empty() {
                return Err("plugin id is empty".into());
            }
            if !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(format!("plugin id {id:?} may hold only letters, digits, - and _"));
            }
            if action.trim().is_empty() {
                return Err("plugin action is empty".into());
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

fn default_method() -> String {
    "GET".to_string()
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
            theme: Theme::default(),
            apps: Vec::new(),
            screen: Screen::default(),
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
    fn a_new_profile_has_the_three_meters() {
        let profile = Profile::new("Test");
        assert_eq!(profile.screen.slots.len(), model::STRIP_SLOTS);
        assert_eq!(
            profile.screen.slots[0],
            Slot::Meter {
                metric: Metric::Cpu
            }
        );
        assert_eq!(
            profile.screen.slots[1],
            Slot::Meter {
                metric: Metric::Gpu
            }
        );
        assert_eq!(
            profile.screen.slots[2],
            Slot::Meter {
                metric: Metric::Memory
            }
        );
    }

    #[test]
    fn a_profile_without_a_screen_gets_the_meters() {
        let json = r#"{
            "name": "Old",
            "pages": [ { "name": "Main", "keys": {} } ]
        }"#;
        let profile: Profile = serde_json::from_str(json).expect("an old profile loads");
        assert_eq!(profile.screen, Screen::default());
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn the_screen_round_trips_through_json() {
        let mut profile = Profile::new("Test");
        profile.screen.slots = vec![
            Slot::Meter {
                metric: Metric::Disk,
            },
            Slot::Clock {
                style: ClockStyle::Time,
            },
            Slot::Weather {
                city: "Berlin".into(),
                latitude: 52.52,
                longitude: 13.405,
                units: Units::Imperial,
            },
        ];
        let text = serde_json::to_string(&profile).expect("serialise");
        let back: Profile = serde_json::from_str(&text).expect("parse");
        assert_eq!(back.screen, profile.screen);
        assert!(back.validate().is_ok());

        // The JSON names the widget with a tag, so a hand edit is readable.
        assert!(text.contains("\"type\":\"weather\""));
        assert!(text.contains("\"city\":\"Berlin\""));
    }

    #[test]
    fn validate_rejects_a_fourth_widget() {
        let mut profile = Profile::new("Test");
        while profile.screen.slots.len() <= model::STRIP_SLOTS {
            profile.screen.slots.push(Slot::Empty);
        }
        let err = profile.validate().expect_err("four widgets do not fit");
        assert!(err.contains("side strip"));
    }

    #[test]
    fn validate_rejects_a_weather_widget_without_a_place() {
        let mut profile = Profile::new("Test");
        profile.screen.slots[2] = Slot::Weather {
            city: "   ".into(),
            latitude: 0.0,
            longitude: 0.0,
            units: Units::Metric,
        };
        let err = profile.validate().expect_err("an empty place is invalid");
        assert!(err.contains("place"));
    }

    #[test]
    fn a_new_profile_has_the_normal_text_size() {
        let profile = Profile::new("Test");
        assert_eq!(profile.screen.text_scale, TEXT_SCALE_DEFAULT);
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn validate_rejects_a_text_size_out_of_range() {
        let mut profile = Profile::new("Test");
        profile.screen.text_scale = TEXT_SCALE_MAX + 0.1;
        let err = profile.validate().expect_err("too large");
        assert!(err.contains("text size"));

        profile.screen.text_scale = 0.1;
        assert!(profile.validate().is_err());

        profile.screen.text_scale = 1.4;
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn validate_rejects_a_weather_widget_off_the_map() {        let mut profile = Profile::new("Test");
        profile.screen.slots[0] = Slot::Weather {
            city: "Nowhere".into(),
            latitude: 91.0,
            longitude: 0.0,
            units: Units::Metric,
        };
        let err = profile.validate().expect_err("latitude 91 is invalid");
        assert!(err.contains("latitude"));
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
    fn the_tools_profile_has_four_pages_and_is_valid() {
        let profile = Profile::tools();
        assert_eq!(profile.pages.len(), 4);
        assert!(profile.validate().is_ok());

        let names: Vec<&str> = profile.pages.iter().map(|page| page.name.as_str()).collect();
        assert_eq!(names, vec!["Media", "System", "Apps", "Tools"]);

        for page in &profile.pages {
            for key in 11..=14u8 {
                let config = page.keys.get(&key).expect("a navigation key");
                assert!(matches!(config.actions.first(), Some(Action::Page { .. })));
            }
            for config in page.keys.values() {
                for action in &config.actions {
                    if let Action::Page { target } = action {
                        assert!(
                            profile.page_index(target).is_some(),
                            "page {target} is missing"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_tools_profile_covers_the_usual_actions() {
        let profile = Profile::tools();
        let mut media = 0;
        let mut hotkeys = 0;
        let mut launches = 0;
        for page in &profile.pages {
            for config in page.keys.values() {
                for action in &config.actions {
                    match action {
                        Action::Media { .. } => media += 1,
                        Action::Hotkey { .. } => hotkeys += 1,
                        Action::Launch { .. } => launches += 1,
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(media, 7, "play, stop, mute, volume and track keys");
        assert!(hotkeys >= 8, "system shortcuts");
        assert!(launches >= 7, "app shortcuts");
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
