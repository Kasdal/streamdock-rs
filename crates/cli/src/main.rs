//! streamdock - command line front end for streamdock-core.
//!
//! Commands:
//!   list                     show every matching HID interface
//!   desc                     show report IDs, sizes and usages
//!   init                     wake the panel and start the session
//!   watch                    show key events, survive unplug and replug
//!   capture                  dump raw input reports
//!   brightness               set panel brightness 0..100
//!   led                      set LED brightness 0..100
//!   clear                    clear one key or every key
//!   image                    upload one image to one key
//!   pattern                  number every key, then show key events
//!   bench                    measure the image transfer rate
//!   send                     write one raw report (explicit opt in)

mod autostart;
mod update;

use std::time::{Duration, Instant};

use streamdock_core::device::{Device, Poll};
use streamdock_core::error::{Error, Result};
use streamdock_core::hid::{self, ReadOutcome};
use streamdock_core::image;
use streamdock_core::model;
use streamdock_core::proto;
use streamdock_core::session::{Session, SessionPoll};
use streamdock_core::tile;
use streamdock_store::profile::{Action, ImageSpec, Profile};
use streamdock_store::Store;

fn usage() {
    eprintln!("streamdock commands:");
    eprintln!("  list");
    eprintln!("  desc");
    eprintln!("  init");
    eprintln!("  watch [--seconds N] [--pid 0xNNNN]");
    eprintln!("  capture [--seconds N] [--pid 0xNNNN]");
    eprintln!("  brightness --value 0..100 [--curve]");
    eprintln!("  led --value 0..100");
    eprintln!("  clear [--key 1..18]");
    eprintln!("  image --key 1..18 --file PATH [--rotate 0|90|180|270] [--fit crop|stretch|none]");
    eprintln!("  pattern [--seconds N]");
    eprintln!("  probe --test strip-size|strip-span|image-limit");
    eprintln!("  bench [--count N] [--key 1..18]");
    eprintln!("  profile <list|init|demo|show|validate|export|import> [--name N] [--dir PATH]");
    eprintln!("  preview [--name N] [--page N] [--out FILE] [--dir PATH]");
    eprintln!("  ui");
    eprintln!("  autostart <install|remove|status> [--profile N]");
    eprintln!("  update check --url URL [--key HEX]");
    eprintln!("  update sign --file latest.json --secret HEX");
    eprintln!("  send --hex 4352540000444953 --i-understand");
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == name)?;
    args.get(pos + 1).cloned()
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn parse_u16(text: &str) -> Option<u16> {
    let t = text.trim();
    let t = t.strip_prefix("0x").unwrap_or(t);
    u16::from_str_radix(t, 16).ok()
}

fn parse_key(args: &[String]) -> Result<u8> {
    match arg_value(args, "--key").and_then(|s| s.trim().parse::<u8>().ok()) {
        Some(k) if model::is_valid_key(k) => Ok(k),
        _ => Err(Error::Message(format!(
            "--key must be 1..{}",
            model::KEY_COUNT
        ))),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("list");

    let result = match command {
        "list" => list(),
        "desc" => desc(),
        "init" => init(),
        "watch" => watch(&args),
        "capture" => capture(&args),
        "brightness" => brightness(&args),
        "led" => led(&args),
        "clear" => clear(&args),
        "image" => image_cmd(&args),
        "pattern" => pattern(&args),
        "probe" => probe(&args),
        "bench" => bench(&args),
        "profile" => profile_cmd(&args),
        "preview" => preview(&args),
        "ui" => ui(),
        "autostart" => autostart_cmd(&args),
        "update" => update_cmd(&args),
        "send" => send(&args),
        "-h" | "--help" | "help" => {
            usage();
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn kind_of(info: &hid::Info) -> &'static str {
    if info.is_vendor_collection() {
        "vendor"
    } else if info.is_keyboard() {
        "keyboard"
    } else {
        "other"
    }
}

fn list() -> Result<()> {
    let devices = hid::enumerate(hid::VENDOR_ID);
    if devices.is_empty() {
        println!("no HID interface with vendor id 0x{:04X}", hid::VENDOR_ID);
        return Ok(());
    }
    for d in devices {
        println!(
            "{:<8} pid=0x{:04X} ver=0x{:04X} usage=0x{:04X}:0x{:04X} in={} out={} feature={} product={:?}",
            kind_of(&d),
            d.product_id,
            d.version,
            d.usage_page,
            d.usage,
            d.input_len,
            d.output_len,
            d.feature_len,
            d.product
        );
        println!("         {}", d.path);
    }
    println!();
    println!("known model for pid 0x{:04X}: {}", model::PID, model::NAME);
    Ok(())
}

fn desc() -> Result<()> {
    let devices = hid::enumerate(hid::VENDOR_ID);
    if devices.is_empty() {
        println!("no HID interface with vendor id 0x{:04X}", hid::VENDOR_ID);
        return Ok(());
    }

    for d in devices {
        println!("== {} {}", kind_of(&d), d.path);
        match hid::describe(&d.path) {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
            }
            Err(e) => println!("describe failed: {e}"),
        }
        println!();
    }
    Ok(())
}

fn init() -> Result<()> {
    let mut dev = Device::open_first()?;
    dev.start_session()?;
    println!("session started on {}", model::NAME);
    println!("input report {} bytes, output report {} bytes",
        dev.info().input_len, dev.info().output_len);
    Ok(())
}

fn brightness(args: &[String]) -> Result<()> {
    let value: u8 = match arg_value(args, "--value").and_then(|s| s.trim().parse().ok()) {
        Some(v) if v <= 100 => v,
        _ => return Err(Error::Message("brightness needs --value 0..100".into())),
    };

    let mut dev = Device::open_first()?;
    if has_flag(args, "--curve") {
        dev.set_brightness_curved(value)?;
        println!(
            "brightness {value}: raw value {} (vendor curve)",
            proto::map_brightness(value)
        );
    } else {
        dev.set_brightness(value)?;
        println!("brightness {value}: raw value {value}");
    }
    Ok(())
}

fn led(args: &[String]) -> Result<()> {
    let value: u8 = match arg_value(args, "--value").and_then(|s| s.trim().parse().ok()) {
        Some(v) if v <= 100 => v,
        _ => return Err(Error::Message("led needs --value 0..100".into())),
    };

    let mut dev = Device::open_first()?;
    dev.set_led_brightness(value)?;
    println!("LED brightness {value}");
    println!("note: this unit may have no LED ring");
    Ok(())
}

fn clear(args: &[String]) -> Result<()> {
    let mut dev = Device::open_first()?;

    if arg_value(args, "--key").is_some() {
        let key = parse_key(args)?;
        dev.clear_key(key)?;
        println!("cleared key {key} (id 0x{:02X})", model::key_id(key));
    } else {
        dev.blank()?;
        println!("cleared every key");
    }
    Ok(())
}

fn image_cmd(args: &[String]) -> Result<()> {
    let key = parse_key(args)?;

    let path = arg_value(args, "--file")
        .ok_or_else(|| Error::Message("image needs --file PATH".into()))?;

    let bytes = std::fs::read(&path)
        .map_err(|e| Error::Message(format!("cannot read {path}: {e}")))?;

    let degrees: u16 = match arg_value(args, "--rotate") {
        Some(v) => {
            let d: u16 = v
                .trim()
                .parse()
                .map_err(|_| Error::Message("--rotate must be a number".into()))?;
            if d % 90 != 0 {
                return Err(Error::Message("--rotate must be 0, 90, 180 or 270".into()));
            }
            d
        }
        None => model::ROTATION,
    };

    let mode = arg_value(args, "--fit").unwrap_or_else(|| "crop".into());
    if !matches!(mode.as_str(), "crop" | "stretch" | "none") {
        return Err(Error::Message("--fit must be crop, stretch or none".into()));
    }

    let mut decoded = image::decode(&bytes).map_err(Error::Message)?;
    let (want_w, want_h) = model::key_size(key);

    match mode.as_str() {
        "crop" => decoded = image::fit_cover(&decoded, want_w, want_h),
        "stretch" => decoded = image::resize(&decoded, want_w, want_h),
        _ => {
            if decoded.width != want_w || decoded.height != want_h {
                println!(
                    "note: key {key} expects {want_w}x{want_h}, the file is {}x{}",
                    decoded.width, decoded.height
                );
            }
        }
    }

    let rotated = image::rotate(&decoded, degrees);
    let data = image::encode_max(&rotated, proto::MAX_IMAGE_BYTES).map_err(Error::Message)?;

    println!(
        "key {key}: {}x{} rotated {degrees} degrees, {} bytes of JPEG",
        rotated.width,
        rotated.height,
        data.len()
    );

    let mut dev = Device::open_first()?;
    dev.start_session()?;
    dev.send_image(key, &data)?;
    println!("key {key} updated (id 0x{:02X})", model::key_id(key));
    Ok(())
}

fn pattern(args: &[String]) -> Result<()> {
    let mut dev = Device::open_first()?;
    dev.start_session()?;
    dev.clear_all()?;

    // Only the grid keys are buttons. The side strip is a display, so a test
    // pattern must not put key numbers there.
    for key in 1..=model::BUTTON_COUNT {
        let size = model::key_size(key);
        let upright = tile::number_tile(key, size);
        let rotated = image::rotate(&upright, model::ROTATION);
        let jpeg = image::encode_max(&rotated, proto::MAX_IMAGE_BYTES).map_err(Error::Message)?;
        dev.send_image(key, &jpeg)?;
        println!(
            "key {:<2} id=0x{:02X} {}x{} {} bytes",
            key,
            model::key_id(key),
            size.0,
            size.1,
            jpeg.len()
        );
    }

    dev.refresh()?;
    println!();
    println!("the top left marker is red.");
    println!(
        "keys 1 to {} are buttons. Keys {} to {} are displays only and are blank.",
        model::BUTTON_COUNT,
        model::BUTTON_COUNT + 1,
        model::KEY_COUNT
    );

    let seconds: u64 = arg_value(args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if seconds > 0 {
        println!();
        watch_loop(&mut dev, Duration::from_secs(seconds))?;
    }
    Ok(())
}

/// Send diagnostic images that answer one hardware question each.
fn probe(args: &[String]) -> Result<()> {
    let test = arg_value(args, "--test").ok_or_else(|| {
        Error::Message("probe needs --test strip-size|strip-span|image-limit".into())
    })?;

    let mut dev = Device::open_first()?;
    dev.start_session()?;

    match test.as_str() {
        "strip-size" => {
            // Three frames of three widths. The frame that shows all four
            // sides tells the real display width of the side strip.
            let cases: [(u8, (u16, u16), [u8; 3], &str); 3] = [
                (16, (80, 80), [255, 255, 255], "white"),
                (17, (64, 80), [64, 255, 64], "green"),
                (18, (56, 80), [80, 160, 255], "blue"),
            ];
            for (key, size, rgb, name) in cases {
                let upright = tile::frame_tile(size, 2, rgb);
                let rotated = image::rotate(&upright, model::ROTATION);
                let jpeg =
                    image::encode_max(&rotated, proto::MAX_IMAGE_BYTES).map_err(Error::Message)?;
                dev.send_image(key, &jpeg)?;
                println!(
                    "key {key}: {name} frame {}x{} (id 0x{:02X})",
                    size.0,
                    size.1,
                    model::key_id(key)
                );
            }
            println!();
            println!("Look at the side strip. A frame that shows all four sides has");
            println!("the correct width. A frame with a cut side is too wide.");
        }
        "strip-span" => {
            // One image that is three strip keys tall. If it fills the whole
            // strip, the strip is one display.
            let size = (80, 240);
            let upright = tile::frame_tile(size, 2, [255, 255, 255]);
            let rotated = image::rotate(&upright, model::ROTATION);
            let jpeg = image::encode_max(&rotated, proto::MAX_IMAGE_BYTES).map_err(Error::Message)?;
            dev.send_image_raw(model::key_id(16), &jpeg)?;
            dev.refresh()?;
            println!("sent one {}x{} image to key 16", size.0, size.1);
            println!();
            println!("If the whole side strip shows one tall frame, the strip is one");
            println!("display. If only the top part changes, it is three displays.");
        }
        "image-limit" => {
            // One noise tile at several qualities gives a ladder of JPEG sizes.
            // Noise compresses badly, so every step is a useful data point.
            let list = arg_value(args, "--qualities")
                .unwrap_or_else(|| "89,90,91,92".to_string());
            let qualities: Vec<u8> = list
                .split(',')
                .filter_map(|s| s.trim().parse::<u8>().ok())
                .collect();
            if qualities.is_empty() {
                return Err(Error::Message("--qualities needs a list such as 89,90,91,92".into()));
            }

            let noise = tile::noise_tile((96, 96), 7);
            let rotated = image::rotate(&noise, model::ROTATION);
            let pause: u64 = arg_value(args, "--pause")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            for (index, quality) in qualities.iter().take(model::BUTTON_COUNT as usize).enumerate() {
                let key = index as u8 + 1;
                let jpeg = image::encode_quality(&rotated, *quality).map_err(Error::Message)?;
                dev.send_image_raw(model::key_id(key), &jpeg)?;
                println!("key {key}: quality {quality}, {} bytes", jpeg.len());
                if pause > 0 {
                    std::thread::sleep(Duration::from_millis(pause));
                }
            }
            dev.refresh()?;
            println!();
            println!("Keys 1 to {} now hold the same noise image at these sizes.", qualities.len().min(model::BUTTON_COUNT as usize));
            println!("Tell me which keys show a complete image.");
        }
        other => {
            return Err(Error::Message(format!("unknown probe test: {other}")));
        }
    }

    Ok(())
}

/// Decode and print events on an open device until the deadline.
fn watch_loop(dev: &mut Device, limit: Duration) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < limit {
        match dev.poll_event(Duration::from_millis(200)) {
            Ok(Poll::Event(e)) => print_event(e.key, e.pressed),
            Ok(Poll::Idle) => {}
            Err(e) => {
                println!("device error after {:.1}s: {e}", start.elapsed().as_secs_f64());
                return Err(e);
            }
        }
    }
    Ok(())
}

fn print_event(id: u8, pressed: bool) {
    let name = match model::key_number(id) {
        Some(n) => format!("key {n}"),
        None => "unknown key".to_string(),
    };
    println!(
        "{name} (0x{id:02X}) {}",
        if pressed { "down" } else { "up" }
    );
}

fn watch(args: &[String]) -> Result<()> {
    let seconds: u64 = arg_value(args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let pid = arg_value(args, "--pid")
        .and_then(|s| parse_u16(&s))
        .unwrap_or(model::PID);

    println!(
        "watching {} (pid 0x{pid:04X}) for {}",
        model::NAME,
        if seconds == 0 {
            "ever".to_string()
        } else {
            format!("{seconds} s")
        }
    );
    println!("the session reconnects after unplug, sleep or resume");

    let mut session = Session::new(pid);
    let start = Instant::now();

    loop {
        if seconds > 0 && start.elapsed() >= Duration::from_secs(seconds) {
            break;
        }

        match session.poll(Duration::from_millis(200)) {
            SessionPoll::Event(e) => print_event(e.key, e.pressed),
            SessionPoll::Connected => println!("panel connected"),
            SessionPoll::Disconnected => {
                let reason = match session.last_error() {
                    Some(e) => e.to_string(),
                    None => "device not present".to_string(),
                };
                println!("panel disconnected: {reason}");
            }
            SessionPoll::Idle => {}
        }
    }

    println!("stopped");
    Ok(())
}

fn capture(args: &[String]) -> Result<()> {
    let seconds: u64 = arg_value(args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let pid = arg_value(args, "--pid")
        .and_then(|s| parse_u16(&s))
        .unwrap_or(model::PID);

    let mut dev = Device::open_pid(pid)?;
    dev.start_session()?;
    println!(
        "opening pid=0x{:04X} in={} out={} for {seconds} s",
        pid,
        dev.info().input_len,
        dev.info().output_len
    );
    println!("press keys on the device now");

    let start = Instant::now();
    let mut seen = 0usize;

    while start.elapsed() < Duration::from_secs(seconds) {
        match dev.read_raw(Duration::from_millis(200))? {
            ReadOutcome::Data(n) => {
                seen += 1;
                let report = dev.last_report(n);
                let head = &report[..report.len().min(16)];
                print!(
                    "{:>8.3}s len={n} head={}",
                    start.elapsed().as_secs_f64(),
                    proto::hex(head)
                );
                if let Some(event) = proto::parse_input(report) {
                    print!(
                        " key {event:?} id=0x{:02X} {}",
                        event.key,
                        if event.pressed { "down" } else { "up" }
                    );
                }
                println!();
            }
            ReadOutcome::Timeout => {}
        }
    }

    println!("{seen} reports received");
    Ok(())
}

fn bench(args: &[String]) -> Result<()> {
    let key = match arg_value(args, "--key") {
        Some(_) => parse_key(args)?,
        None => 1,
    };
    let count: u32 = arg_value(args, "--count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    let size = model::key_size(key);
    let upright = tile::number_tile(key, size);
    let rotated = image::rotate(&upright, model::ROTATION);
    let jpeg = image::encode_max(&rotated, proto::MAX_IMAGE_BYTES).map_err(Error::Message)?;

    let mut dev = Device::open_first()?;
    dev.start_session()?;

    println!(
        "sending {} bytes to key {key}, {count} times",
        jpeg.len()
    );

    let mut times = Vec::with_capacity(count as usize);
    for i in 0..count {
        let start = Instant::now();
        dev.send_image(key, &jpeg)?;
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        times.push(ms);
        if i % 10 == 9 {
            println!("  {}/{}: {:.1} ms", i + 1, count, ms);
        }
    }

    let total: f64 = times.iter().sum();
    let min = times.iter().cloned().fold(f64::MAX, f64::min);
    let max = times.iter().cloned().fold(0.0, f64::max);
    println!(
        "min {min:.1} ms, average {:.1} ms, max {max:.1} ms",
        total / count as f64
    );
    println!(
        "about {:.1} uploads per second",
        count as f64 / (total / 1000.0)
    );
    Ok(())
}

fn profile_store(args: &[String]) -> Store {
    let dir = arg_value(args, "--dir")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Store::default_dir);
    Store::new(dir)
}

fn profile_name(args: &[String], fallback: &str) -> String {
    arg_value(args, "--name").unwrap_or_else(|| fallback.to_string())
}

fn image_summary(spec: &ImageSpec) -> String {
    match spec {
        ImageSpec::Color { color } => format!("colour {color}"),
        ImageSpec::Text { text, .. } => format!("text {text:?}"),
        ImageSpec::File { path } => format!("file {path}"),
        ImageSpec::Svg { path, .. } => format!("svg {path}"),
        ImageSpec::Animation { path, phase_ms } => match phase_ms {
            Some(phase) => format!("animation {path} phase {phase} ms"),
            None => format!("animation {path}"),
        },
        ImageSpec::Icon { name, .. } => format!("icon {name}"),
    }
}

fn profile_cmd(args: &[String]) -> Result<()> {
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
    let store = profile_store(args);
    let map = |e: streamdock_store::StoreError| Error::Message(e.to_string());

    match sub {
        "list" => {
            let names = store.list().map_err(map)?;
            if names.is_empty() {
                println!("no profiles in {}", store.dir().display());
                println!("create one with: streamdock profile init");
                return Ok(());
            }
            for name in names {
                println!("{name}");
            }
        }
        "init" => {
            let name = profile_name(args, "Default");
            let mut profile = Profile::starter();
            profile.name = name.clone();
            store.save(&profile).map_err(map)?;
            println!("wrote {}", store.path_for(&name).map_err(map)?.display());
        }
        "demo" => {
            let name = profile_name(args, "Demo");
            let mut profile = Profile::demo();
            profile.name = name.clone();
            store.save(&profile).map_err(map)?;
            println!("wrote {}", store.path_for(&name).map_err(map)?.display());
            println!("run: streamdockd --profile {name}");
        }
        "export" => {
            let name = profile_name(args, "Default");
            let out = arg_value(args, "--out").unwrap_or_else(|| format!("{name}.zip"));
            let count = store
                .export(&name, std::path::Path::new(&out))
                .map_err(map)?;
            println!("exported {name} to {out} with {count} file(s)");
        }
        "import" => {
            let file = arg_value(args, "--file")
                .ok_or_else(|| Error::Message("import needs --file PATH".into()))?;
            let new_name = arg_value(args, "--name");
            let profile = store
                .import(std::path::Path::new(&file), new_name.as_deref())
                .map_err(map)?;
            println!(
                "imported {} with {} page(s)",
                profile.name,
                profile.pages.len()
            );
        }
        "show" | "validate" => {
            let name = profile_name(args, "Default");
            let profile = store.load(&name).map_err(map)?;
            println!(
                "profile {} (version {}) pid={} layout={}",
                profile.name, profile.version, profile.device.pid, profile.device.layout
            );
            if sub == "validate" {
                println!("valid: {} pages", profile.pages.len());
                return Ok(());
            }
            for page in &profile.pages {
                println!("  page {}: {} keys", page.name, page.keys.len());
                for (key, config) in &page.keys {
                    let mut parts = Vec::new();
                    if let Some(label) = &config.label {
                        parts.push(format!("label {label:?}"));
                    }
                    if let Some(image) = &config.image {
                        parts.push(image_summary(image));
                    }
                    if !config.actions.is_empty() {
                        let list: Vec<String> =
                            config.actions.iter().map(Action::describe).collect();
                        parts.push(list.join(", "));
                    }
                    if parts.is_empty() {
                        continue;
                    }
                    println!("    key {key}: {}", parts.join(" | "));
                }
            }
        }
        other => {
            return Err(Error::Message(format!("unknown profile command: {other}")));
        }
    }

    Ok(())
}

/// Write a picture of one page, so a user can check a profile without the panel.
fn preview(args: &[String]) -> Result<()> {
    let store = profile_store(args);
    let name = profile_name(args, "Default");
    let profile = store
        .load(&name)
        .map_err(|e| Error::Message(e.to_string()))?;

    let page_number: usize = arg_value(args, "--page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    if page_number == 0 || page_number > profile.pages.len() {
        return Err(Error::Message(format!(
            "--page must be 1..={}",
            profile.pages.len()
        )));
    }
    let page = &profile.pages[page_number - 1];

    let palette = streamdock_render::palette(&profile.theme).unwrap_or_default();
    let sheet = streamdock_render::contact_sheet_in_themed(page, store.dir(), &palette)
        .map_err(Error::Message)?;
    let jpeg = image::encode_quality(&sheet, 90).map_err(Error::Message)?;

    let out = arg_value(args, "--out").unwrap_or_else(|| "profile-preview.jpg".to_string());
    std::fs::write(&out, &jpeg)
        .map_err(|e| Error::Message(format!("cannot write {out}: {e}")))?;

    println!(
        "{} page {} \"{}\": {}x{} -> {out} ({} bytes)",
        profile.name,
        page_number,
        page.name,
        sheet.width,
        sheet.height,
        jpeg.len()
    );
    Ok(())
}

/// Open the editor of the running daemon in the default browser.
fn ui() -> Result<()> {
    let path = streamdock_ipc::runtime_path();
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Error::Message(format!(
            "cannot read {}: {e}. Is streamdockd running?",
            path.display()
        ))
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| Error::Message(format!("bad runtime file: {e}")))?;

    let port = value
        .get("port")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::Message("the runtime file has no port".into()))?;
    let token = value
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Message("the runtime file has no token".into()))?;

    let url = format!("http://127.0.0.1:{port}/?token={token}");
    streamdock_actions::launch(&url).map_err(|e| Error::Message(e.to_string()))?;
    println!("opened {url}");
    Ok(())
}

fn autostart_cmd(args: &[String]) -> Result<()> {
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");

    match sub {
        "install" => {
            let mut extra: Vec<String> = Vec::new();
            if let Some(profile) = arg_value(args, "--profile") {
                extra.push("--profile".to_string());
                extra.push(profile);
            }
            let borrowed: Vec<&str> = extra.iter().map(|s| s.as_str()).collect();
            let value = autostart::install(&borrowed).map_err(Error::Message)?;
            println!("autostart installed:");
            println!("  HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run");
            println!("  {value}");
        }
        "remove" => {
            let removed = autostart::remove().map_err(Error::Message)?;
            if removed {
                println!("autostart removed");
            } else {
                println!("autostart was not set");
            }
        }
        "status" => match autostart::status().map_err(Error::Message)? {
            Some(value) => println!("autostart: {value}"),
            None => println!("autostart: not set"),
        },
        other => {
            return Err(Error::Message(format!(
                "unknown autostart command: {other}. Use install, remove or status"
            )));
        }
    }

    Ok(())
}

fn update_cmd(args: &[String]) -> Result<()> {
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("check");

    match sub {
        "check" => {
            let url = arg_value(args, "--url")
                .ok_or_else(|| Error::Message("update check needs --url URL".into()))?;
            let key =
                arg_value(args, "--key").unwrap_or_else(|| update::PUBLIC_KEY_HEX.to_string());
            let outcome =
                update::check(&url, &key, env!("CARGO_PKG_VERSION")).map_err(Error::Message)?;

            println!("current version {}", outcome.current);
            println!(
                "server version {} at {}",
                outcome.manifest.version, outcome.manifest.url
            );
            if !outcome.manifest.sha256.is_empty() {
                println!("sha256 {}", outcome.manifest.sha256);
            }
            if outcome.newer {
                println!("an update is available");
                if !outcome.manifest.notes.is_empty() {
                    println!("notes: {}", outcome.manifest.notes);
                }
            } else {
                println!("this program is up to date");
            }
        }
        "sign" => {
            let file = arg_value(args, "--file")
                .ok_or_else(|| Error::Message("update sign needs --file PATH".into()))?;
            let secret = arg_value(args, "--secret")
                .ok_or_else(|| Error::Message("update sign needs --secret HEX".into()))?;
            let bytes = std::fs::read(&file)
                .map_err(|e| Error::Message(format!("cannot read {file}: {e}")))?;
            let signature = update::sign(&bytes, &secret).map_err(Error::Message)?;
            println!("{signature}");
        }
        other => {
            return Err(Error::Message(format!(
                "unknown update command: {other}. Use check or sign"
            )));
        }
    }

    Ok(())
}

fn send(args: &[String]) -> Result<()> {    let payload = match arg_value(args, "--hex") {
        Some(v) => proto::parse_hex(&v).map_err(Error::Message)?,
        None => return Err(Error::Message("send needs --hex".into())),
    };

    // Writes go to the hardware. Require an explicit acknowledgement.
    if !has_flag(args, "--i-understand") {
        eprintln!("refusing to write to the device without --i-understand");
        std::process::exit(3);
    }

    let mut dev = Device::open_first()?;
    let report = proto::chunk(&payload);
    let wrote = dev.write_report(&report)?;
    println!(
        "wrote {wrote} bytes: {}",
        proto::hex(&payload[..payload.len().min(32)])
    );
    Ok(())
}
