//! Action execution on Windows.
//!
//! The engine turns a stored `Action` into input events. It holds no device
//! state. An action that changes the panel returns an `Effect`, and the daemon
//! applies it.
//!
//! Safety: `Command` runs a program. The profile is user data, so the daemon
//! must ask for an explicit opt in before it accepts a profile with a
//! `command` action.

use std::time::Duration;

use streamdock_store::profile::{Action, MediaCommand};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_RETURN,
    VK_TAB, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// What the daemon must do after an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing beyond the action itself.
    None,
    /// Show another page.
    SwitchPage(String),
    /// Set the panel brightness.
    SetBrightness(u8),
}

/// Failure from one action.
#[derive(Debug)]
pub enum ActionError {
    /// A key name is not known.
    UnknownKey(String),
    /// A hotkey has modifiers but no key.
    EmptyHotkey,
    /// A text action has no text.
    EmptyText,
    /// The system rejected the input.
    Input(String),
    /// A file or program call failed.
    Os(String),
    /// The action is not available yet.
    Unsupported(String),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::UnknownKey(k) => write!(f, "unknown key name: {k}"),
            ActionError::EmptyHotkey => f.write_str("hotkey has modifiers but no key"),
            ActionError::EmptyText => f.write_str("text action is empty"),
            ActionError::Input(m) => write!(f, "input injection failed: {m}"),
            ActionError::Os(m) => write!(f, "{m}"),
            ActionError::Unsupported(m) => write!(f, "not supported: {m}"),
        }
    }
}

impl std::error::Error for ActionError {}

/// Result alias for the engine.
pub type Result<T> = std::result::Result<T, ActionError>;

/// One key to press, with the extended flag for the arrow pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub vk: u16,
    pub extended: bool,
}

/// A parsed hotkey: zero or more modifiers and one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: Vec<u16>,
    pub key: KeyPress,
}

/// How long a hotkey stays down, in milliseconds.
pub const HOTKEY_HOLD_MS: u64 = 12;

/// Map a key name to a virtual key code.
///
/// Names are case insensitive. Modifiers are not keys here; use `parse_hotkey`.
pub fn parse_key(name: &str) -> Option<KeyPress> {
    let n = name.trim().to_ascii_lowercase();
    let simple = |vk: u16| Some(KeyPress { vk, extended: false });
    let extended = |vk: u16| Some(KeyPress { vk, extended: true });

    if n.chars().count() == 1 {
        let c = n.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return simple(c.to_ascii_uppercase() as u16);
        }
        if c.is_ascii_digit() {
            return simple(c as u16);
        }
    }

    if let Some(rest) = n.strip_prefix('f') {
        if let Ok(number) = rest.parse::<u16>() {
            if (1..=24).contains(&number) {
                return simple(0x70 + number - 1);
            }
        }
    }

    match n.as_str() {
        "enter" | "return" => simple(0x0D),
        "tab" => simple(0x09),
        "escape" | "esc" => simple(0x1B),
        "space" => simple(0x20),
        "backspace" => simple(0x08),
        "delete" | "del" => extended(0x2E),
        "insert" | "ins" => extended(0x2D),
        "home" => extended(0x24),
        "end" => extended(0x23),
        "pageup" | "pgup" => extended(0x21),
        "pagedown" | "pgdn" => extended(0x22),
        "up" => extended(0x26),
        "down" => extended(0x28),
        "left" => extended(0x25),
        "right" => extended(0x27),
        "printscreen" | "prtsc" => extended(0x2C),
        "pause" => simple(0x13),
        "capslock" => simple(0x14),
        "numlock" => extended(0x90),
        "scrolllock" => simple(0x91),
        "minus" => simple(0xBD),
        "plus" | "equal" => simple(0xBB),
        "comma" => simple(0xBC),
        "period" | "dot" => simple(0xBE),
        "slash" => simple(0xBF),
        "backslash" => simple(0xDC),
        "semicolon" => simple(0xBA),
        "quote" => simple(0xDE),
        "backtick" | "grave" => simple(0xC0),
        "bracketleft" => simple(0xDB),
        "bracketright" => simple(0xDD),
        _ => None,
    }
}

/// Map a modifier name to a virtual key code.
pub fn parse_modifier(name: &str) -> Option<u16> {
    match name.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(VK_LCONTROL as u16),
        "shift" => Some(VK_LSHIFT as u16),
        "alt" | "menu" => Some(VK_LMENU as u16),
        "win" | "super" | "meta" => Some(VK_LWIN as u16),
        _ => None,
    }
}

/// Split a list of names into modifiers and one key.
pub fn parse_hotkey(names: &[String]) -> Result<Hotkey> {
    let mut modifiers = Vec::new();
    let mut key = None;

    for name in names {
        if let Some(vk) = parse_modifier(name) {
            modifiers.push(vk);
            continue;
        }
        if key.is_some() {
            return Err(ActionError::UnknownKey(name.clone()));
        }
        match parse_key(name) {
            Some(k) => key = Some(k),
            None => return Err(ActionError::UnknownKey(name.clone())),
        }
    }

    match key {
        Some(key) => Ok(Hotkey { modifiers, key }),
        None => Err(ActionError::EmptyHotkey),
    }
}

/// Run a list of actions in order.
///
/// The first action that returns an effect stops the list. A page switch
/// must not run the actions after it on the old page.
pub fn run(actions: &[Action]) -> Result<Effect> {
    for action in actions {
        let effect = run_one(action)?;
        if effect != Effect::None {
            return Ok(effect);
        }
    }
    Ok(Effect::None)
}

/// Run one action.
pub fn run_one(action: &Action) -> Result<Effect> {
    match action {
        Action::Hotkey { keys } => {
            let hotkey = parse_hotkey(keys)?;
            press_hotkey(&hotkey, HOTKEY_HOLD_MS)?;
            Ok(Effect::None)
        }
        Action::Text { text, delay_ms } => {
            if text.is_empty() {
                return Err(ActionError::EmptyText);
            }
            type_text(text, *delay_ms)?;
            Ok(Effect::None)
        }
        Action::Media { command } => {
            press_media(*command)?;
            Ok(Effect::None)
        }
        Action::Launch { target } => {
            launch(target)?;
            Ok(Effect::None)
        }
        Action::Multi { actions, pause_ms } => {
            for (index, inner) in actions.iter().enumerate() {
                if index > 0 && *pause_ms > 0 {
                    std::thread::sleep(Duration::from_millis(*pause_ms));
                }
                let effect = run_one(inner)?;
                if effect != Effect::None {
                    return Ok(effect);
                }
            }
            Ok(Effect::None)
        }
        Action::Page { target } => Ok(Effect::SwitchPage(target.clone())),
        Action::Brightness { value } => Ok(Effect::SetBrightness(*value)),
        Action::Command { program, args } => {
            run_command(program, args)?;
            Ok(Effect::None)
        }
    }
}

fn key_input(vk: u16, up: bool, extended: bool) -> INPUT {
    let mut flags: KEYBD_EVENT_FLAGS = 0;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk as VIRTUAL_KEY,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: the slice lives for the call and the layout matches Win32.
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(ActionError::Input(format!(
            "SendInput accepted {sent} of {} events",
            inputs.len()
        )))
    }
}

/// Press a key combination and release it.
pub fn press_hotkey(hotkey: &Hotkey, hold_ms: u64) -> Result<()> {
    let mut down = Vec::with_capacity(hotkey.modifiers.len() + 1);
    for m in &hotkey.modifiers {
        down.push(key_input(*m, false, false));
    }
    down.push(key_input(hotkey.key.vk, false, hotkey.key.extended));
    send(&down)?;

    std::thread::sleep(Duration::from_millis(hold_ms.clamp(1, 500)));

    let mut up = Vec::with_capacity(hotkey.modifiers.len() + 1);
    up.push(key_input(hotkey.key.vk, true, hotkey.key.extended));
    for m in hotkey.modifiers.iter().rev() {
        up.push(key_input(*m, true, false));
    }
    send(&up)
}

/// Type a string as Unicode input. Newline and tab become real keys.
pub fn type_text(text: &str, delay_ms: u64) -> Result<()> {
    let mut first = true;
    for ch in text.chars() {
        if !first && delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        first = false;

        match ch {
            '\r' => continue,
            '\n' => {
                send(&[
                    key_input(VK_RETURN as u16, false, false),
                    key_input(VK_RETURN as u16, true, false),
                ])?;
            }
            '\t' => {
                send(&[
                    key_input(VK_TAB as u16, false, false),
                    key_input(VK_TAB as u16, true, false),
                ])?;
            }
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    send(&[unicode_input(*unit, false), unicode_input(*unit, true)])?;
                }
            }
        }
    }
    Ok(())
}

/// Send one media key.
pub fn press_media(command: MediaCommand) -> Result<()> {
    let vk = match command {
        MediaCommand::PlayPause => VK_MEDIA_PLAY_PAUSE,
        MediaCommand::Next => VK_MEDIA_NEXT_TRACK,
        MediaCommand::Previous => VK_MEDIA_PREV_TRACK,
        MediaCommand::Stop => VK_MEDIA_STOP,
        MediaCommand::VolumeUp => VK_VOLUME_UP,
        MediaCommand::VolumeDown => VK_VOLUME_DOWN,
        MediaCommand::Mute => VK_VOLUME_MUTE,
    };
    send(&[
        key_input(vk as u16, false, true),
        key_input(vk as u16, true, true),
    ])
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Open a file, a folder or a URL with the system handler.
pub fn launch(target: &str) -> Result<()> {
    let operation = wide("open");
    let file = wide(target);
    // SAFETY: both strings are null terminated and live for the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<std::ffi::c_void>() as HWND,
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = result as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(ActionError::Os(format!(
            "ShellExecuteW failed for {target:?} with code {code}"
        )))
    }
}

/// Start a program with no console window.
pub fn run_command(program: &str, args: &[String]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| ActionError::Os(format!("cannot start {program:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_keys_map_to_virtual_codes() {
        assert_eq!(parse_key("a"), Some(KeyPress { vk: 0x41, extended: false }));
        assert_eq!(parse_key("Z"), Some(KeyPress { vk: 0x5A, extended: false }));
        assert_eq!(parse_key("7"), Some(KeyPress { vk: 0x37, extended: false }));
        assert_eq!(parse_key("f1"), Some(KeyPress { vk: 0x70, extended: false }));
        assert_eq!(parse_key("f12"), Some(KeyPress { vk: 0x7B, extended: false }));
        assert_eq!(parse_key("f24"), Some(KeyPress { vk: 0x87, extended: false }));
        assert_eq!(parse_key("f25"), None);
        assert_eq!(parse_key("enter"), Some(KeyPress { vk: 0x0D, extended: false }));
        assert_eq!(parse_key("up"), Some(KeyPress { vk: 0x26, extended: true }));
        assert_eq!(parse_key("nonsense"), None);
    }

    #[test]
    fn modifiers_are_recognised() {
        assert_eq!(parse_modifier("CTRL"), Some(0xA2));
        assert_eq!(parse_modifier("shift"), Some(0xA0));
        assert_eq!(parse_modifier("alt"), Some(0xA4));
        assert_eq!(parse_modifier("win"), Some(0x5B));
        assert_eq!(parse_modifier("hyper"), None);
    }

    #[test]
    fn hotkeys_split_into_modifiers_and_key() {
        let names = vec!["ctrl".to_string(), "shift".to_string(), "m".to_string()];
        let hotkey = parse_hotkey(&names).expect("valid hotkey");
        assert_eq!(hotkey.modifiers, vec![0xA2, 0xA0]);
        assert_eq!(hotkey.key.vk, 0x4D);
    }

    #[test]
    fn a_hotkey_needs_one_key() {
        let names = vec!["ctrl".to_string(), "alt".to_string()];
        assert!(matches!(
            parse_hotkey(&names),
            Err(ActionError::EmptyHotkey)
        ));
    }

    #[test]
    fn a_hotkey_rejects_two_keys() {
        let names = vec!["a".to_string(), "b".to_string()];
        assert!(matches!(
            parse_hotkey(&names),
            Err(ActionError::UnknownKey(_))
        ));
    }

    #[test]
    fn a_page_action_returns_an_effect() {
        let action = Action::Page {
            target: "Media".into(),
        };
        assert_eq!(
            run_one(&action).expect("effect"),
            Effect::SwitchPage("Media".into())
        );
    }

    #[test]
    fn a_brightness_action_returns_an_effect() {
        let action = Action::Brightness { value: 40 };
        assert_eq!(
            run_one(&action).expect("effect"),
            Effect::SetBrightness(40)
        );
    }

    #[test]
    fn an_effect_stops_the_list() {
        let actions = vec![
            Action::Page {
                target: "A".into(),
            },
            Action::Brightness { value: 90 },
        ];
        assert_eq!(
            run(&actions).expect("effect"),
            Effect::SwitchPage("A".into())
        );
    }

    #[test]
    fn an_empty_list_does_nothing() {
        assert_eq!(run(&[]).expect("no effect"), Effect::None);
    }

    #[test]
    fn multi_propagates_an_effect() {
        let action = Action::Multi {
            actions: vec![
                Action::Brightness { value: 10 },
                Action::Page {
                    target: "B".into(),
                },
            ],
            pause_ms: 0,
        };
        assert_eq!(
            run_one(&action).expect("effect"),
            Effect::SetBrightness(10)
        );
    }
}
