# streamdock-rs

Rust controller for the AJAZZ AKP-153E keypad.
The crate talks to the device through the Win32 HID API.
It needs no C compiler, no vendor driver and no vendor software.

## Status

| Part | State |
| --- | --- |
| Device enumeration | verified on hardware |
| Interface capabilities | verified on hardware |
| Command writes | verified on hardware |
| Key events (read) | verified on hardware |
| Image upload | verified on hardware |
| Image rotation | measured on hardware: 270 degrees |
| Reconnect after unplug | verified on hardware |
| Daemon with actions | verified on hardware: page switch, repaint |
| Browser editor | verified on hardware: edit a key, panel updates |
| Drag and drop | verified on hardware: drop a PNG on a key |
| Tray icon and autostart | verified on hardware: menu, quit, Run key |
| Undo and save conflict | verified on hardware: undo, warning on a stale save |
| Animated GIF keys | verified on hardware: 4 frame loop, keys stay responsive |
| Real text font | verified on hardware: TrueType labels at a readable size |
| SVG icons | verified on hardware: microphone icon on a key |
| Animation phases | verified on hardware: two keys run out of step |
| Unit tests | 142 tests, no device needed |

## Device facts (measured)

Model: AJAZZ AKP-153E.
Device: `VID 0x0300`, `PID 0x3010`, version `0x0002`.
Product string `HOTSPOTEKUSB HID DEMO`, serial `0300D0782435`.

Two HID collections:

| Interface | Usage | Input | Output | Feature |
| --- | --- | --- | --- | --- |
| `MI_00` vendor | `0xFFA0:0x0001` | 513 bytes | 1025 bytes | 0 bytes |
| `MI_01` keyboard | `0x0001:0x0006` | 9 bytes | 2 bytes | 0 bytes |

The descriptor declares no numbered reports. Byte 0 of every buffer is a
report ID slot and stays `0x00`.
The vendor input report holds 512 payload bytes: byte 1 is usage
`0xFFA1:0x0003`, bytes 2 to 512 are usage `0xFFA1:0x0004`.
The vendor output report holds 1024 payload bytes: byte 1 is usage
`0xFFA1:0x0005`, bytes 2 to 1024 are usage `0xFFA1:0x0006`.

Windows denies user-mode access to the keyboard collection.
The vendor collection is the only user-mode channel.

## Protocol

Each output report is 1025 bytes:

```
byte 0        0x00 report ID
byte 1..5     0x43 0x52 0x54 0x00 0x00   "CRT"
byte 6..      command word and parameters, zero padded
```

Commands: `DIS` wake, `CONNECT` heartbeat, `STP` refresh, `LIG` brightness,
`BAT` image header, `CLE` clear. Image data follows the header as raw JPEG
bytes in 1024-byte chunks, and each chunk starts with the `0x00` byte.

The words come from the public Bitfocus Companion surface driver
(`bitfocus/companion-surface-mirabox-stream-dock`).
That driver lists the same USB IDs for the AKP-153E.

Key events arrive as input reports. Byte 9 is the key ID, byte 10 is the
state, `0x00` is up and any other value is down.

Measured on this unit, the report starts with the report ID byte, so the
key ID is at byte 10 and the state at byte 11:

```
00 41 43 4b 00 00 4f 4b 00 00 0d 01 00 ...
   A  C  K        O  K        key  state
```

The panel shows every image rotated. The host must rotate the bitmap before
it encodes the JPEG. Measured on this unit: 90 degrees shows the image
upside-down, 270 degrees shows it upright.

The panel sends no key report until the host sends `CONNECT`.
The host must repeat `CONNECT` every 8 seconds.

## Key layout

Keys 1 to 15 form a 5 by 3 grid of 96x96 pixel buttons.
Keys 16 to 18 are three 80x80 pixel displays on the right side.
The side strip has no buttons and sends no key event.

| Key | ID | Size | Button |
| --- | --- | --- | --- |
| 1 to 15 | `0x0D 0x0A 0x07 0x04 0x01` / `0x0E 0x0B 0x08 0x05 0x02` / `0x0F 0x0C 0x09 0x06 0x03` | 96x96 | yes |
| 16 to 18 | `0x10 0x11 0x12` | 80x80 | no, display only |

## Workspace

| Path | Content |
| --- | --- |
| `crates/core` | protocol, HID handle, image pipeline, reconnect (`streamdock-core`) |
| `crates/store` | profile model and JSON store (`streamdock-store`) |
| `crates/render` | key tile renderer: colour, text, image files (`streamdock-render`) |
| `crates/actions` | hotkey, text, media, launch, multi actions (`streamdock-actions`) |
| `crates/ipc` | loopback HTTP server, token, editor page (`streamdock-ipc`) |
| `crates/daemon` | the `streamdockd` binary |
| `crates/cli` | the `streamdock` binary |
| `handoff/HANDOFF.md` | session record and the product plan |
| `installer-analysis.md` | static analysis of the vendor installer |

The core crate holds no policy and no UI:

```rust
use std::time::Duration;
use streamdock_core::{Device, Poll};

let mut device = Device::open_first()?;
device.start_session()?;
device.set_brightness(60)?;
device.send_image(1, &jpeg_bytes)?;
if let Poll::Event(e) = device.poll_event(Duration::from_millis(200))? {
    println!("key {} pressed={}", e.key, e.pressed);
}
```

`Session` adds reconnect. It owns at most one `Device`.
A failed call drops the handle, and the next poll reopens the panel.
Reopen tries are limited to one every 500 ms.

## Daemon

`streamdockd` owns the device and runs the profile while no window is open.

```powershell
streamdock profile demo
streamdockd --profile Demo
```

| Flag | Meaning |
| --- | --- |
| `--profile NAME` | profile to load, default `Default` |
| `--dir PATH` | profile folder, default `%LOCALAPPDATA%\StreamDock\profiles` |
| `--seconds N` | stop after N seconds |
| `--no-blank` | keep the keys on exit |
| `--verbose` | log every input report |
| `--no-ui` | do not start the editor server |
| `--no-tray` | do not show the tray icon |

The daemon prints the editor URL at start. `streamdock ui` opens it in the
default browser. The port is random, and the token is in
`%LOCALAPPDATA%\StreamDock\runtime.json`.

The editor is one HTML page with a key grid and a key editor. Apply a key and
the panel changes within a second. The page needs no build step and no extra
runtime.

The grid shows the real tile that the daemon draws, not an approximation.
Drag a JPEG, PNG, GIF or SVG file onto a key to set its image. A GIF becomes
an animation. The file is copied into the `assets` folder next to the profile,
so the profile stays portable.

The Undo button restores the profile from before the last save. Ctrl+Z does
the same when the focus is not in a text field.

Every save carries the revision that the editor started from. If another
window saved first, the daemon refuses the change with a conflict. The editor
then warns, and a second press of Apply key overwrites on purpose.

The socket binds to `127.0.0.1` only. Every request needs the token.
A second daemon exits, because a named mutex guards the device.

The tray icon has three items:

| Item | Effect |
| --- | --- |
| Open editor | Opens the editor page in the default browser |
| Reload profile | Reads the profile file again and repaints |
| Quit | Stops the daemon, clears the keys, removes the icon |

`streamdock autostart install` adds the daemon to the per-user Run key, so it
starts at logon. No administrator rights are needed.
`streamdock autostart remove` deletes the value.
`streamdock autostart status` shows the current value.

The daemon does these steps:

1. Take a named mutex, so a second copy exits with an error.
2. Load the profile. A missing file is created as a starter profile.
3. Start the panel thread. It owns the device and the heartbeat.
4. Paint all 18 keys. The side strip is a display, so it takes a tile too.
5. Poll for key events with a heartbeat.
6. Run the actions of a pressed key in order.
7. Repaint the page after a page switch or a reconnect.
8. Clear the keys on exit, unless `--no-blank` is set.

The daemon uses two threads:

| Thread | Work |
| --- | --- |
| panel | writes images and brightness, reads key reports, sends the heartbeat |
| coordinator | profile, actions, editor server, animation timing |

The panel thread drains a bounded queue for at most 40 ms, then reads keys.
An animation frame is only a queue write, because every frame is pre-encoded.
Key presses stay responsive while a key animates.

Measured: a page switch repaints 18 keys in about 20 ms.

`streamdock preview --name Demo --out sheet.jpg` writes a picture of one page
without the panel. Use it to check a profile on any machine.

## Profiles

One profile is one JSON file. A page holds keys by number.

```json
{
  "version": 1,
  "name": "Default",
  "device": { "pid": "0x3010", "layout": "akp153e" },
  "pages": [
    {
      "name": "Main",
      "keys": {
        "1": {
          "label": "Mute",
          "actions": [ { "type": "hotkey", "keys": ["ctrl", "shift", "m"] } ]
        },
        "2": {
          "image": { "kind": "text", "text": "SCENE 2", "background": "#14375c" },
          "actions": [ { "type": "page", "target": "Second" } ]
        }
      }
    }
  ]
}
```

Image kinds: `color`, `text`, `file`, `svg`, `animation`, `icon`.

An `icon` looks for `<profile folder>/icons/<name>.svg`. Without that file,
the name is drawn as text.

An `animation` is a GIF. Each frame is fitted to the key, rotated and encoded
once. The daemon keeps at most 60 frames per animation and samples a longer
GIF down while keeping the loop time. A frame delay is clamped to 20 ms or
more. Each key starts at its own point in the loop unless `phase_ms` is set.

A `file` path may be absolute, or relative to the profile folder. The editor
stores a dropped file as `assets/<name>-<hash>.<ext>`. JPEG, PNG, GIF and SVG
are accepted.

Text uses a system TrueType font and wraps to fit the key. Set
`STREAMDOCK_FONT` to a font file to override it. The 5x7 bitmap font is the
fallback when no font is found.

Action types: `hotkey`, `text`, `media`, `launch`, `multi`, `page`,
`brightness`, `command`. A `command` action runs a program, so a profile with
one needs an explicit opt in before a release build accepts it.

## Build

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;<mingw64>\bin;<rustup self-contained>\;$env:Path"
cargo build --release
cargo test --release
```

`<mingw64>` is the WinLibs `bin` directory.
`<rustup self-contained>` helps the linker find the matching runtime.

Cargo writes its progress to stderr, so PowerShell reports exit code 1.
Read the `Finished` line.

## Commands

```powershell
streamdock.exe list
streamdock.exe desc
streamdock.exe init
streamdock.exe watch --seconds 30
streamdock.exe capture --seconds 30
streamdock.exe brightness --value 60 [--curve]
streamdock.exe led --value 40
streamdock.exe clear [--key 1]
streamdock.exe image --key 1 --file photo.jpg [--fit crop|stretch|none] [--rotate 270]
streamdock.exe pattern
streamdock.exe probe --test strip-size|strip-span|image-limit
streamdock.exe bench --count 30
streamdock.exe profile <list|init|demo|show|validate> [--name N]
streamdock.exe preview [--name N] [--page N] [--out FILE]
streamdock.exe ui
streamdock.exe autostart <install|remove|status> [--profile N]
streamdock.exe send --hex 4352540000444953 --i-understand
```

* `list` shows every HID interface of the vendor ID.
* `desc` shows the report IDs, sizes and usages of each interface.
* `init` sends wake, heartbeat and refresh.
* `watch` prints key events. It reconnects after unplug, sleep and resume.
* `capture` dumps raw input reports with the decoded key event.
* `brightness` sets panel brightness from 0 to 100. `--curve` applies the
  vendor formula `round((percent / 100) ^ 0.75 * 100)`.
* `led` sets the LED brightness. This unit may have no LED ring.
* `clear` clears one key or every key.
* `image` uploads one image. The command scales the image to the key size,
  rotates it 270 degrees and compresses it under the size limit.
  `--fit crop` keeps the aspect ratio and cuts the long side.
  `--fit stretch` distorts to the key size. `--fit none` sends the file as is.
* `pattern` numbers keys 1 to 15 and leaves the side strip clear.
  It shows the key order and the rotation.
* `probe` answers one hardware question. See `handoff/HANDOFF.md`.
* `bench` measures the image transfer rate.
* `profile` lists, creates, shows and validates profiles. `profile demo`
  writes a two page profile for a daemon test.
* `preview` writes a picture of one page as a JPEG, without the panel.
* `ui` opens the editor of the running daemon in the default browser.
* `autostart` adds or removes the daemon in the per-user Run key.
* `send` writes one raw payload. It refuses to write without `--i-understand`.

## Limits

The key IDs of keys 1 to 15 match the reports that this unit sends.
The side strip has no buttons, so keys 16 to 18 never send a key event.

Hotkey, text and launch actions are verified on hardware: text typed into
Notepad, `ctrl+a` selected it, and `notepad.exe` opened from a key.

A `command` action runs a program. A profile that holds one is user data, so
the daemon does not ask for confirmation yet. Do this before a release.

Text uses a system font, so no font file is redistributed. A product build
must bundle a font with a suitable licence and set `STREAMDOCK_FONT`.

The vendor application was never installed or executed.
The installer payload is not decompressed.
See `installer-analysis.md` for the static analysis, and
`handoff/HANDOFF.md` for the full session record.

## Safety

Read commands are harmless.
Write commands change device state.
Power cycle the device if it stops to respond.
