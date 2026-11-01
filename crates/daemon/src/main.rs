//! streamdockd - background panel daemon.
//!
//! Two threads:
//!
//! 1. The panel thread owns the HID session. It drains a bounded queue and
//!    reads key reports, so the heartbeat never stops.
//! 2. This thread owns the profile, the actions, the editor server and the
//!    animation clocks. It encodes images and queues them.
//!
//! Usage:
//!   streamdockd [--profile NAME] [--dir PATH] [--seconds N] [--no-blank]
//!               [--verbose] [--no-ui] [--no-tray]
//!
//! A release build has no console. The log goes to
//! `%LOCALAPPDATA%\StreamDock\logs\daemon.log`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod painter;
mod panel;
mod single;
mod tray;

use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use streamdock_actions::Effect;
use streamdock_core::model;
use streamdock_core::proto::KeyEvent;
use streamdock_ipc as ipc;
use streamdock_render as render;
use streamdock_store::profile::Profile;
use streamdock_store::Store;

use painter::Painter;
use panel::{Panel, PanelEvent};
use single::SingleInstance;

static START: OnceLock<Instant> = OnceLock::new();
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
static LOG_FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

/// Write one line to the log file, and to the console when there is one.
pub(crate) fn log(message: impl AsRef<str>) {
    let start = START.get_or_init(Instant::now);
    let line = format!("[{:>9.3}] {}", start.elapsed().as_secs_f64(), message.as_ref());

    if let Some(file) = LOG_FILE.get_or_init(open_log).as_ref() {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(file, "{line}");
        }
    }
    if console_attached() {
        println!("{line}");
    }
}

/// Write plain text to the console, when there is one.
fn say(text: impl AsRef<str>) {
    if console_attached() {
        println!("{}", text.as_ref());
    }
}

fn log_dir() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("StreamDock").join("logs")
}

/// Open the log file and rotate it when it grows past 1 MB.
fn open_log() -> Option<Mutex<std::fs::File>> {
    let dir = log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("daemon.log");

    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.len() > 1_000_000 {
            let _ = std::fs::rename(&path, dir.join("daemon.old.log"));
        }
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    Some(Mutex::new(file))
}

fn console_attached() -> bool {
    unsafe { !windows_sys::Win32::System::Console::GetConsoleWindow().is_null() }
}

/// Ask the loop to stop. The tray menu and Ctrl+C both use this.
fn request_shutdown() {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

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
        say("streamdockd [--profile NAME] [--dir PATH] [--seconds N] [--no-blank] [--verbose] [--no-ui] [--no-tray] [--no-auto-switch]");
        say("");
        say("  --profile NAME  profile to load, default Default");
        say("  --dir PATH      profile folder, default %LOCALAPPDATA%\\StreamDock\\profiles");
        say("  --seconds N     stop after N seconds, default run until Ctrl+C");
        say("  --no-blank      keep the keys on exit instead of clearing them");
        say("  --verbose       log every input report");
        say("  --no-ui         do not start the editor server");
        say("  --no-tray       do not show the tray icon");
        say("  --no-auto-switch  keep one profile, ignore the app rules");
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
    let verbose = args.iter().any(|a| a == "--verbose");
    let auto_switch = !args.iter().any(|a| a == "--no-auto-switch");

    let _guard = SingleInstance::acquire(single::MUTEX_NAME)?;

    let store = Store::new(&dir);
    let first_run = store
        .path_for(&profile_name)
        .map(|path| !path.exists())
        .unwrap_or(false);
    let mut profile = store.load_or_create(&profile_name)?;
    let pid = profile.device.product_id().unwrap_or(model::PID);

    log(format!(
        "profile {} from {} ({} pages, pid 0x{pid:04X})",
        profile.name,
        store.dir().display(),
        profile.pages.len()
    ));

    let mut panel = Panel::start(pid)?;
    let mut painter = Painter::new(store.dir().to_path_buf());
    let mut page = 0usize;
    let mut auto = apps::AutoSwitch::build_verbose(&store, &profile.name, auto_switch, verbose);
    if auto.enabled() {
        log(format!(
            "app switch on, {} rule(s), default profile {}",
            auto.rule_count,
            auto.default_profile()
        ));
    }

    // The editor talks to this thread through the channel. Only the panel
    // thread touches the device, so a save cannot race with a paint.
    let shared = ipc::Shared::new(ipc::State::new(profile.clone(), page));
    let (commands, incoming) = mpsc::channel::<ipc::Command>();
    let mut tray = None;
    if !args.iter().any(|a| a == "--no-ui") {
        let tray_sender = commands.clone();
        match ipc::Server::start(Arc::clone(&shared), commands, store.dir().to_path_buf()) {
            Ok(server) => {
                match server.write_runtime_file() {
                    Ok(path) => log(format!("runtime file {}", path.display())),
                    Err(e) => log(format!("cannot write the runtime file: {e}")),
                }
                log(format!("editor {}", server.url()));
                if !args.iter().any(|a| a == "--no-tray") {
                    match tray::Tray::spawn(server.url(), tray_sender, request_shutdown) {
                        Ok(icon) => {
                            tray = Some(icon);
                            log("tray icon ready");
                        }
                        Err(e) => log(format!("cannot create the tray icon: {e}")),
                    }
                }
            }
            Err(e) => log(format!("cannot start the editor server: {e}")),
        }
    }

    if first_run {
        log(format!(
            "first run: created profile {} with the tools pages",
            profile.name
        ));
        if let Some(icon) = &tray {
            icon.notify(
                "Stream Dock",
                "Keys can send keystrokes. Open the editor from the tray menu.",
            );
        }
    }

    let started = Instant::now();

    loop {
        while let Ok(command) = incoming.try_recv() {
            handle_command(
                command,
                &mut profile,
                &mut page,
                &panel,
                &mut painter,
                &store,
                &shared,
                &mut auto,
            );
        }

        let mut busy = false;
        while let Some(event) = panel.try_event() {
            busy = true;
            handle_panel_event(
                event,
                &mut profile,
                &mut page,
                &panel,
                &mut painter,
                &shared,
                verbose,
            );
        }

        if painter.tick(&panel) {
            busy = true;
        }

        if let Some(target) = auto.check(Instant::now()) {
            switch_profile(
                &target,
                &mut profile,
                &mut page,
                &panel,
                &mut painter,
                &store,
                &shared,
            );
            busy = true;
        }

        if shutdown_requested() {
            log("shutdown requested");
            break;
        }
        if seconds > 0 && started.elapsed() >= Duration::from_secs(seconds) {
            log(format!("stopping after {seconds} s"));
            break;
        }

        if !busy {
            thread::sleep(Duration::from_millis(painter::IDLE_TICK_MS));
        }
    }

    drop(tray);

    if blank_on_exit {
        panel.blank();
        log("keys cleared");
    }
    panel.stop();

    Ok(())
}

/// Handle one report from the panel thread.
fn handle_panel_event(
    event: PanelEvent,
    profile: &mut Profile,
    page: &mut usize,
    panel: &Panel,
    painter: &mut Painter,
    shared: &Arc<ipc::Shared>,
    verbose: bool,
) {
    match event {
        PanelEvent::Connected => {
            log("panel connected");
            shared.update(|state| state.connected = true);
            paint_page(panel, painter, profile, *page);
        }
        PanelEvent::Disconnected(reason) => {
            log(format!("panel disconnected: {reason}"));
            shared.update(|state| state.connected = false);
            painter.stop_animations();
        }
        PanelEvent::Key(event) => {
            handle_key(event, profile, page, panel, painter, shared, verbose);
        }
    }
}

fn handle_key(
    event: KeyEvent,
    profile: &mut Profile,
    page: &mut usize,
    panel: &Panel,
    painter: &mut Painter,
    shared: &Arc<ipc::Shared>,
    verbose: bool,
) {
    if verbose {
        log(format!(
            "report: key id 0x{:02X} pressed={}",
            event.key, event.pressed
        ));
    }
    if !event.pressed {
        return;
    }

    let Some(number) = model::key_number(event.key) else {
        log(format!("unknown key id 0x{:02X}", event.key));
        return;
    };
    if !model::is_button(number) {
        log(format!("key {number} is a display and has no button"));
        return;
    }

    let actions = match profile.pages[*page].key(number) {
        Some(config) => config.actions.clone(),
        None => {
            if verbose {
                log(format!("key {number} has no configuration"));
            }
            return;
        }
    };
    if actions.is_empty() {
        if verbose {
            log(format!("key {number} has no actions"));
        }
        return;
    }

    log(format!("key {number} -> {} action(s)", actions.len()));

    match streamdock_actions::run(&actions) {
        Ok(Effect::None) => {}
        Ok(Effect::SetBrightness(value)) => {
            panel.brightness(value);
            log(format!("brightness {value}"));
            shared.update(|state| state.brightness = Some(value));
        }
        Ok(Effect::SwitchPage(target)) => match profile.page_index(&target) {
            Some(index) if index != *page => {
                *page = index;
                log(format!("page {}", profile.pages[*page].name));
                shared.update(|state| state.set_page(index));
                paint_page(panel, painter, profile, *page);
            }
            Some(_) => {}
            None => log(format!("no page named {target:?}")),
        },
        Err(e) => log(format!("action failed: {e}")),
    }
}

/// Apply one editor command.
fn handle_command(
    command: ipc::Command,
    profile: &mut Profile,
    page: &mut usize,
    panel: &Panel,
    painter: &mut Painter,
    store: &Store,
    shared: &Arc<ipc::Shared>,
    auto: &mut apps::AutoSwitch,
) {
    match command {
        ipc::Command::Save {
            profile: next,
            expected_rev,
            reply,
        } => {
            if let Some(expected) = expected_rev {
                let current = shared.snapshot().rev;
                if expected != current {
                    log(format!(
                        "refused a stale save (client rev {expected}, current {current})"
                    ));
                    let _ = reply.send(ipc::Reply::Conflict(shared.snapshot()));
                    return;
                }
            }
            if let Err(e) = next.validate() {
                let _ = reply.send(ipc::Reply::Error(e));
                return;
            }
            if let Err(e) = store.save(&next) {
                let _ = reply.send(ipc::Reply::Error(e.to_string()));
                return;
            }
            *profile = next;
            if *page >= profile.pages.len() {
                *page = profile.pages.len().saturating_sub(1);
            }
            auto.rebuild(store);
            log(format!(
                "profile saved by the editor, page {}",
                profile.pages[*page].name
            ));
            paint_page(panel, painter, profile, *page);
            shared.update(|state| {
                state.set_profile(profile.clone());
                state.set_page(*page);
            });
            let _ = reply.send(ipc::Reply::Ok(shared.snapshot()));
        }
        ipc::Command::SwitchPage { target, reply } => match profile.page_index(&target) {
            Some(index) => {
                *page = index;
                log(format!("page {} from the editor", profile.pages[index].name));
                paint_page(panel, painter, profile, *page);
                shared.update(|state| state.set_page(index));
                let _ = reply.send(ipc::Reply::Ok(shared.snapshot()));
            }
            None => {
                let _ = reply.send(ipc::Reply::Error(format!("no page named {target:?}")));
            }
        },
        ipc::Command::Brightness { value, reply } => {
            panel.brightness(value);
            log(format!("brightness {value} from the editor"));
            shared.update(|state| state.brightness = Some(value));
            let _ = reply.send(ipc::Reply::Ok(shared.snapshot()));
        }
        ipc::Command::Reload { reply } => match store.load(&profile.name) {
            Ok(next) => {
                *profile = next;
                if *page >= profile.pages.len() {
                    *page = 0;
                }
                auto.rebuild(store);
                log(format!(
                    "profile reloaded, page {}",
                    profile.pages[*page].name
                ));
                paint_page(panel, painter, profile, *page);
                shared.update(|state| {
                    state.set_profile(profile.clone());
                    state.set_page(*page);
                });
                let _ = reply.send(ipc::Reply::Ok(shared.snapshot()));
            }
            Err(e) => {
                let _ = reply.send(ipc::Reply::Error(e.to_string()));
            }
        },
        ipc::Command::Import {
            bytes,
            new_name,
            reply,
        } => match store.import_bytes(&bytes, new_name.as_deref()) {
            Ok(imported) => {
                *profile = imported;
                *page = 0;
                auto.rebuild(store);
                log(format!(
                    "profile {} imported, page {}",
                    profile.name, profile.pages[0].name
                ));
                paint_page(panel, painter, profile, *page);
                shared.update(|state| {
                    state.set_profile(profile.clone());
                    state.set_page(0);
                });
                let _ = reply.send(ipc::Reply::Ok(shared.snapshot()));
            }
            Err(e) => {
                let _ = reply.send(ipc::Reply::Error(e.to_string()));
            }
        },
    }
}

/// Switch to another profile that a foreground app asked for.
fn switch_profile(
    target: &str,
    profile: &mut Profile,
    page: &mut usize,
    panel: &Panel,
    painter: &mut Painter,
    store: &Store,
    shared: &Arc<ipc::Shared>,
) {
    if profile.name == target {
        return;
    }
    match store.load(target) {
        Ok(next) => {
            *profile = next;
            *page = 0;
            log(format!("app switch to profile {}", profile.name));
            paint_page(panel, painter, profile, *page);
            shared.update(|state| {
                state.set_profile(profile.clone());
                state.set_page(0);
            });
        }
        Err(e) => log(format!("cannot switch to profile {target}: {e}")),
    }
}

/// Queue every key of one page and start its animations.
fn paint_page(panel: &Panel, painter: &mut Painter, profile: &Profile, page: usize) {
    let page_ref = &profile.pages[page];
    let palette = render::palette(&profile.theme).unwrap_or_default();
    let mut warnings = Vec::new();
    let queued = painter.paint(panel, page_ref, &palette, &mut |message| warnings.push(message));

    for warning in warnings {
        log(warning);
    }

    if painter.is_animated() {
        log(format!(
            "page {} queued, {queued} keys, animation running",
            page_ref.name
        ));
    } else {
        log(format!("page {} queued, {queued} keys", page_ref.name));
    }
}
