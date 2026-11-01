//! streamdock - minimal Rust controller for Stream Dock / AJAZZ keypads.
//!
//! Commands:
//!   list                       show every matching HID interface
//!   desc                       show report IDs, sizes and usages
//!   capture --seconds N        dump input reports (optionally with heartbeat)
//!   init                       wake the panel and start the session
//!   brightness --value N       set panel brightness 0..100
//!   image --key N --file X.jpg upload one JPEG to one key
//!   send --hex ...             write one raw report (explicit opt in)

mod hid;
mod image;
mod model;
mod proto;

use std::io::Write;
use std::time::{Duration, Instant};

fn usage() {
    eprintln!("streamdock commands:");
    eprintln!("  list");
    eprintln!("  desc");
    eprintln!("  capture [--seconds N] [--pid 0xNNNN]");
    eprintln!("  init");
    eprintln!("  brightness --value 0..100");
    eprintln!("  image --key 1..18 --file PATH [--rotate 0|90|180|270]");
    eprintln!("  send --hex 4352540000444953 --i-understand");
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == name)?;
    args.get(pos + 1).cloned()
}

fn parse_u16(text: &str) -> Option<u16> {
    let t = text.trim();
    let t = t.strip_prefix("0x").unwrap_or(t);
    u16::from_str_radix(t, 16).ok()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("list");

    match command {
        "list" => list(),
        "desc" => desc(),
        "capture" => capture(&args),
        "init" => init(),
        "brightness" => brightness(&args),
        "image" => image(&args),
        "send" => send(&args),
        "-h" | "--help" | "help" => usage(),
        other => {
            eprintln!("unknown command: {other}");
            usage();
            std::process::exit(2);
        }
    }
}

/// Open the vendor collection. `write` asks for write access as well.
fn open_vendor(write: bool) -> hid::Device {
    let devices = hid::enumerate(hid::VENDOR_ID);
    let info = match devices.into_iter().find(|d| d.is_vendor_collection()) {
        Some(i) => i,
        None => {
            eprintln!("device not found");
            std::process::exit(1);
        }
    };

    let opened = if write {
        hid::Device::open(info.clone())
    } else {
        hid::Device::open_read(info.clone())
    };

    match opened {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            std::process::exit(1);
        }
    }
}

fn list() {
    let devices = hid::enumerate(hid::VENDOR_ID);
    if devices.is_empty() {
        println!("no HID interface with vendor id 0x{:04X}", hid::VENDOR_ID);
        return;
    }
    for d in devices {
        let kind = if d.is_vendor_collection() {
            "vendor"
        } else if d.is_keyboard() {
            "keyboard"
        } else {
            "other"
        };
        println!(
            "{kind:<8} pid=0x{:04X} ver=0x{:04X} usage=0x{:04X}:0x{:04X} in={} out={} feature={} product={:?}",
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
}

fn desc() {
    let devices = hid::enumerate(hid::VENDOR_ID);
    if devices.is_empty() {
        println!("no HID interface with vendor id 0x{:04X}", hid::VENDOR_ID);
        return;
    }

    for d in devices {
        let kind = if d.is_vendor_collection() {
            "vendor"
        } else if d.is_keyboard() {
            "keyboard"
        } else {
            "other"
        };
        println!("== {kind} {}", d.path);
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
}

/// Send one command payload and report the result.
fn write_command(dev: &hid::Device, command: &[u8]) -> std::io::Result<usize> {
    let report = proto::frame(command);
    dev.write_report(&report)
}

fn init() {
    let dev = open_vendor(true);

    for (name, command) in [
        ("wake", proto::cmd::WAKE.to_vec()),
        ("heartbeat", proto::cmd::HEARTBEAT.to_vec()),
        ("refresh", proto::cmd::REFRESH.to_vec()),
    ] {
        match write_command(&dev, &command) {
            Ok(n) => println!("{name}: wrote {n} bytes"),
            Err(e) => {
                eprintln!("{name} failed: {e}");
                std::process::exit(1);
            }
        }
    }

    println!("session started on {}", model::NAME);
}

fn brightness(args: &[String]) {
    let value: u8 = match arg_value(args, "--value").and_then(|s| s.trim().parse().ok()) {
        Some(v) if v <= 100 => v,
        _ => {
            eprintln!("brightness needs --value 0..100");
            std::process::exit(2);
        }
    };

    let dev = open_vendor(true);
    match write_command(&dev, &proto::cmd::brightness(value)) {
        Ok(n) => println!("brightness {value}: wrote {n} bytes"),
        Err(e) => {
            eprintln!("brightness failed: {e}");
            std::process::exit(1);
        }
    }
}

fn image(args: &[String]) {
    let key: u8 = match arg_value(args, "--key").and_then(|s| s.trim().parse().ok()) {
        Some(k) if (1..=model::KEY_COUNT).contains(&k) => k,
        _ => {
            eprintln!("image needs --key 1..{}", model::KEY_COUNT);
            std::process::exit(2);
        }
    };

    let path = match arg_value(args, "--file") {
        Some(p) => p,
        None => {
            eprintln!("image needs --file PATH");
            std::process::exit(2);
        }
    };

    let file = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(2);
        }
    };

    let degrees: u16 = match arg_value(args, "--rotate") {
        Some(v) => match v.trim().parse::<u16>() {
            Ok(d) if d % 90 == 0 => d,
            _ => {
                eprintln!("--rotate must be 0, 90, 180 or 270");
                std::process::exit(2);
            }
        },
        None => model::ROTATION,
    };

    let decoded = match image::decode(&file) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(2);
        }
    };

    let (want_w, want_h) = model::key_size(key);
    if decoded.width != want_w || decoded.height != want_h {
        println!(
            "note: key {key} expects {want_w}x{want_h}, the file is {}x{}",
            decoded.width, decoded.height
        );
    }

    let rotated = image::rotate(&decoded, degrees);
    let data = match image::encode_max(&rotated, proto::MAX_IMAGE_BYTES) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(2);
        }
    };

    println!(
        "key {key}: {}x{} rotated {degrees} degrees, {} bytes of JPEG",
        rotated.width,
        rotated.height,
        data.len()
    );

    let id = model::key_id(key);
    let dev = open_vendor(true);

    match write_command(&dev, &proto::cmd::image_header(id, data.len())) {
        Ok(_) => println!("sent image header: {} bytes for key {key}", data.len()),
        Err(e) => {
            eprintln!("header failed: {e}");
            std::process::exit(1);
        }
    }

    for (index, part) in data.chunks(proto::PAYLOAD).enumerate() {
        let report = proto::chunk(part);
        if let Err(e) = dev.write_report(&report) {
            eprintln!("data chunk {index} failed: {e}");
            std::process::exit(1);
        }
    }
    println!("sent {} image chunks", data.len().div_ceil(proto::PAYLOAD));

    match write_command(&dev, proto::cmd::REFRESH) {
        Ok(_) => println!("refresh sent"),
        Err(e) => eprintln!("refresh failed: {e}"),
    }
}

fn capture(args: &[String]) {
    let seconds: u64 = arg_value(args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let want_pid = arg_value(args, "--pid").and_then(|s| parse_u16(&s));

    let devices = hid::enumerate(hid::VENDOR_ID);
    let picked = devices.into_iter().find(|d| {
        d.is_vendor_collection() && want_pid.map(|p| p == d.product_id).unwrap_or(true)
    });

    let info = match picked {
        Some(i) => i,
        None => {
            eprintln!("device not found");
            std::process::exit(1);
        }
    };

    println!(
        "opening pid=0x{:04X} in={} out={} for {} s",
        info.product_id, info.input_len, info.output_len, seconds
    );
    println!("press keys on the device now");

    let mut dev = match hid::Device::open(info.clone()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            std::process::exit(1);
        }
    };

    // The panel sends no key reports until the host starts a session.
    if let Err(e) = write_command(&dev, proto::cmd::HEARTBEAT) {
        eprintln!("heartbeat failed: {e}");
    }

    let start = Instant::now();
    let deadline = Duration::from_secs(seconds);
    let mut buf = vec![0u8; info.input_len as usize];
    let mut seen = 0usize;
    let mut next_beat = Duration::from_secs(8);

    while start.elapsed() < deadline {
        if start.elapsed() >= next_beat {
            if let Err(e) = write_command(&dev, proto::cmd::HEARTBEAT) {
                eprintln!("heartbeat failed: {e}");
            }
            next_beat = start.elapsed() + Duration::from_secs(8);
        }

        match dev.read_report_timeout(&mut buf, 200) {
            Ok(hid::ReadOutcome::Data(n)) => {
                seen += 1;
                let slice = &buf[..n.min(buf.len())];
                let head = &slice[..slice.len().min(16)];
                print!(
                    "{:>8.3}s len={} head={}",
                    start.elapsed().as_secs_f64(),
                    n,
                    proto::hex(head)
                );
                if let Some(event) = proto::parse_input(slice) {
                    let name = match model::key_number(event.key) {
                        Some(n) => format!("key {n}"),
                        None => "unknown key".to_string(),
                    };
                    print!(
                        " {name} (0x{:02X}) {}",
                        event.key,
                        if event.pressed { "down" } else { "up" }
                    );
                }
                println!();
                let _ = std::io::stdout().flush();
            }
            Ok(hid::ReadOutcome::Timeout) => continue,
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        }
    }

    println!("{seen} reports received");
    let _ = std::io::stdout().flush();
}

fn send(args: &[String]) {
    let payload = match arg_value(args, "--hex") {
        Some(v) => match proto::parse_hex(&v) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("bad hex: {e}");
                std::process::exit(2);
            }
        },
        None => {
            eprintln!("send needs --hex");
            std::process::exit(2);
        }
    };

    // Writes go to the hardware. Require an explicit acknowledgement.
    let confirmed = args.iter().any(|a| a == "--i-understand");
    if !confirmed {
        eprintln!("refusing to write to the device without --i-understand");
        std::process::exit(3);
    }

    let dev = open_vendor(true);
    let report = proto::chunk(&payload);

    match dev.write_report(&report) {
        Ok(n) => println!("wrote {n} bytes: {}", proto::hex(&payload[..payload.len().min(32)])),
        Err(e) => {
            eprintln!("write failed: {e}");
            std::process::exit(1);
        }
    }
}
