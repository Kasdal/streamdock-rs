//! streamdockd - background panel daemon.
//!
//! The daemon owns the HID handle. It paints the keys of one profile, runs the
//! actions of the pressed keys, and repaints after a reconnect.
//!
//! Usage:
//!   streamdockd [--profile NAME] [--dir PATH] [--seconds N] [--no-blank]
//!
//! The keys keep working when no window is open, because there is no window.

mod single;

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use streamdock_actions::Effect;
use streamdock_core::model;
use streamdock_core::session::{Session, SessionPoll};
use streamdock_render as render;
use streamdock_store::profile::Profile;
use streamdock_store::Store;

use single::SingleInstance;

static START: OnceLock<Instant> = OnceLock::new();
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Print one line with the seconds since start.
fn log(message: impl AsRef<str>) {
    let start = START.get_or_init(Instant::now);
    println!("[{:>9.3}] {}", start.elapsed().as_secs_f64(), message.as_ref());
}

/// Ask the loop to stop on Ctrl+C and on console close.
unsafe extern "system" fn on_ctrl(_signal: u32) -> i32 {
    SHUTDOWN.store(true, Ordering::SeqCst);
    1
}

fn install_shutdown_handler() {
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(on_ctrl), 1);
    }
}

fn shutdown_requested() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == name)?;
    args.get(pos + 1).cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("streamdockd [--profile NAME] [--dir PATH] [--seconds N] [--no-blank]");
        println!();
        println!("  --profile NAME  profile to load, default Default");
        println!("  --dir PATH      profile folder, default %LOCALAPPDATA%\\StreamDock\\profiles");
        println!("  --seconds N     stop after N seconds, default run until Ctrl+C");
        println!("  --no-blank      keep the keys on exit instead of clearing them");
        return ExitCode::SUCCESS;
    }

    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log(format!("fatal: {e}"));
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    START.get_or_init(Instant::now);
    install_shutdown_handler();

    let profile_name = arg_value(args, "--profile").unwrap_or_else(|| "Default".to_string());
    let dir = arg_value(args, "--dir")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Store::default_dir);
    let seconds: u64 = arg_value(args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let blank_on_exit = !args.iter().any(|a| a == "--no-blank");

    let _guard = SingleInstance::acquire(single::MUTEX_NAME)?;

    let store = Store::new(&dir);
    let profile = store.load_or_create(&profile_name)?;
    let pid = profile.device.product_id().unwrap_or(model::PID);

    log(format!(
        "profile {} from {} ({} pages, pid 0x{pid:04X})",
        profile.name,
        store.dir().display(),
        profile.pages.len()
    ));

    let mut session = Session::new(pid);
    let mut page = 0usize;
    let started = Instant::now();

    loop {
        if shutdown_requested() {
            log("shutdown requested");
            break;
        }
        if seconds > 0 && started.elapsed() >= Duration::from_secs(seconds) {
            log(format!("stopping after {seconds} s"));
            break;
        }

        match session.poll(Duration::from_millis(200)) {
            SessionPoll::Connected => {
                log("panel connected");
                paint_all(&mut session, &profile, page);
            }
            SessionPoll::Disconnected => {
                let reason = match session.last_error() {
                    Some(e) => e.to_string(),
                    None => "not present".to_string(),
                };
                log(format!("panel disconnected: {reason}"));
            }
            SessionPoll::Event(event) => {
                if !event.pressed {
                    continue;
                }
                let Some(number) = model::key_number(event.key) else {
                    log(format!("unknown key id 0x{:02X}", event.key));
                    continue;
                };
                if !model::is_button(number) {
                    log(format!("key {number} is a display and has no button"));
                    continue;
                }

                let actions = match profile.pages[page].key(number) {
                    Some(config) => config.actions.clone(),
                    None => continue,
                };
                if actions.is_empty() {
                    continue;
                }

                log(format!(
                    "key {number} -> {} action(s)",
                    actions.len()
                ));

                match streamdock_actions::run(&actions) {
                    Ok(Effect::None) => {}
                    Ok(Effect::SetBrightness(value)) => {
                        match session.with_device(|dev| dev.set_brightness(value)) {
                            Ok(()) => log(format!("brightness {value}")),
                            Err(e) => log(format!("brightness failed: {e}")),
                        }
                    }
                    Ok(Effect::SwitchPage(target)) => match profile.page_index(&target) {
                        Some(index) if index != page => {
                            page = index;
                            log(format!("page {}", profile.pages[page].name));
                            paint_all(&mut session, &profile, page);
                        }
                        Some(_) => {}
                        None => log(format!("no page named {target:?}")),
                    },
                    Err(e) => log(format!("action failed: {e}")),
                }
            }
            SessionPoll::Idle => {}
        }
    }

    if blank_on_exit {
        match session.with_device(|dev| dev.blank()) {
            Ok(()) => log("keys cleared"),
            Err(e) => log(format!("could not clear the keys: {e}")),
        }
    }

    Ok(())
}

/// Paint every key of one page.
///
/// A render fault draws a red ERR tile, so a broken image file does not leave
/// a stale key on the panel.
fn paint_all(session: &mut Session, profile: &Profile, page: usize) {
    let page_ref = &profile.pages[page];
    let mut sent = 0usize;

    let result = session.with_device(|dev| {
        sent = 0;
        for key in 1..=model::KEY_COUNT {
            let jpeg = match render::encode_key(page_ref, key) {
                Ok(jpeg) => jpeg,
                Err(e) => {
                    log(format!("key {key}: {e}"));
                    match render::fault_tile(model::key_size(key)) {
                        Ok(jpeg) => jpeg,
                        Err(e) => {
                            log(format!("key {key}: cannot draw the fault tile: {e}"));
                            continue;
                        }
                    }
                }
            };
            dev.send_image(key, &jpeg)?;
            sent += 1;
        }
        Ok(())
    });

    match result {
        Ok(()) => log(format!("page {} painted, {sent} keys", page_ref.name)),
        Err(e) => log(format!("paint stopped after {sent} keys: {e}")),
    }
}
