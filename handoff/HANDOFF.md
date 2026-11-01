# Handoff: Stream Dock / AJAZZ AKP-153E Rust controller

Date: 2026-11-01
Author of this document: previous agent session (REA static analysis plus hardware tests).

Read this file first. Then read `README.md`.
The file `installer-analysis.md` holds the static analysis of the vendor installer.
Section 13 to 18 hold the plan for the full replacement product.

## 1. Objective

The user asked to inspect a vendor installer because they suspected malware.
The file was not malware.
The user then asked to rebuild the device support in Rust and make it better.
Scope agreed with the user: **core HID controller only**.
The crate controls the panel through Win32 HID.
It has no GUI, no C dependency and no vendor software.

## 2. The artifact that was analysed

Path: `C:\Users\plesm\Downloads\Stream-Dock-AJAZZ-Installer_Windows_global2.0.exe`
Size: 653 MB.

SHA-256:

```
D6FB46511E17D5FCB9A3A4553C5F0DF251FD1E3D2B30FDAE49EBECFE479F7B9A
```

Verdict: **not malware**.

Evidence:

1. A valid Authenticode signature covers the whole file.
2. The signer is Shenzhen An Rui Xin Technology. This company makes the
   MiraBox, Stream Dock and AJAZZ NACODEX hardware.
3. The file is an Advanced Installer (Caphyon) bootstrapper.
4. The bootstrapper is a stub, 30 theme resource PEs, a plaintext MSI catalog
   and one LZMA payload. The layout starts at these offsets:
   stub `0x0`, themes `0x1A9400`, catalog `0x4B0800`, payload `0x7D0000`,
   certificate table to the end of file.
5. The bootstrapper extracts the MSI to
   `%APPDATA%\HotSpot\Stream Dock AJAZZ Global 3.10.202.0711\install`,
   and then runs `msiexec`.
6. The application is Qt5 + QCefView + CEF 109, with `node20.exe` plugins and
   `hidapi` / `libusb`.
7. MSI ProductCode is `{4C83B5CA-0D61-4E8C-BF4A-A41E79F0570A}`.

Negative checks, all clean:

1. No `Run` key.
2. No scheduled task.
3. No service.
4. No miner, keylogger, wallet address or webhook.
5. No `powershell -enc` command.

Limits of the analysis:

1. The LZMA payload was catalogued, not decompressed.
2. The installer was never executed.
3. The vendor application was never installed.

## 3. The device (measured on hardware)

Model: **AJAZZ AKP-153E**. The user confirmed the model with a photo.

| Item | Value |
| --- | --- |
| Vendor ID | `0x0300` |
| Product ID | `0x3010` |
| Version | `0x0002` |
| Product string | `HOTSPOTEKUSB HID DEMO` |
| Serial | `0300D0782435` |

Two HID collections:

| Interface | Usage | Input | Output | Feature |
| --- | --- | --- | --- | --- |
| `MI_00` vendor | `0xFFA0:0x0001` | 513 | 1025 | 0 |
| `MI_01` keyboard | `0x0001:0x0006` | 9 | 2 | 0 |

Vendor collection report layout, from the HID caps of this crate:

1. The descriptor declares no numbered reports. Byte 0 is a report ID slot and
   always stays `0x00`.
2. Input payload: byte 1 is usage `0xFFA1:0x0003`, bytes 2 to 512 are usage
   `0xFFA1:0x0004`.
3. Output payload: byte 1 is usage `0xFFA1:0x0005`, bytes 2 to 1024 are usage
   `0xFFA1:0x0006`.
4. There are no feature reports.

## 4. Facts that are verified on hardware

| Fact | How it was verified |
| --- | --- |
| Enumeration of both collections | `streamdock list` |
| Report IDs, sizes and usages | `streamdock desc` |
| Command writes are accepted | `streamdock init` wrote 1025 bytes for each command |
| Key reports arrive | Capture run, 157 reports during key presses |
| Key decode offsets | Value `0x01` to `0x0f` appeared, state toggled `00` and `01` |
| Key ID table | Pressed keys matched the Companion table |
| Image upload | 18 images uploaded, all 18 keys showed the tile |
| Rotation | 90 degrees is upside-down, 270 degrees is upright |
| Heartbeat is required | Two earlier capture runs saw no report without it |
| Side strip is display only | A finger on the strip sends no report |
| Side strip is three 80x80 displays | A complete 80x80 frame on each segment |
| Digit sizes | 1 to 15 in the right order, no mirroring |
| Reconnect after unplug | `watch` printed disconnected, then connected |
| Images do not survive a replug | Panel shows only the AJAZZ start logo |
| Image size rule | 15 keys at 9264 bytes each all show complete tiles |

## 5. Protocol

Every output report is 1025 bytes:

```
byte 0        0x00 report ID slot
byte 1..5     0x43 0x52 0x54 0x00 0x00   "CRT"
byte 6..      command word and parameters, zero padded
```

Command words:

| Word | Bytes | Meaning |
| --- | --- | --- |
| `DIS` | `44 49 53` | Wake the panel |
| `CONNECT` | `43 4f 4e 4e 45 43 54` | Heartbeat |
| `STP` | `53 54 50` | Refresh the panel |
| `LIG` | `4c 49 47 00 00 <0..100>` | Panel brightness |
| `LBLIG` | `4c 42 4c 49 47 <0..100>` | LED brightness |
| `BAT` | `42 41 54 <size:4> <key>` | Image header |
| `CLE` | `43 4c 45 00 00 00 <key or ff>` | Clear one key or all keys |
| `SETLB` | `53 45 54 4c 42 <r> <g> <b>` | LED array |

Image transfer:

1. Send `BAT` with the JPEG size and the key ID.
2. Send the JPEG bytes as raw payload chunks of 1024 bytes.
3. Every chunk report starts with the `0x00` byte.
4. Send `STP`.
5. Keep the JPEG below 10240 bytes. Decode, rotate and re-encode the image when
   needed.

Key report, 513 bytes:

```
00 41 43 4b 00 00 4f 4b 00 00 <key> <state> 00 ...
   A  C  K        O  K
```

1. Key ID at byte 10, state at byte 11.
2. State `0x00` is up. Any other value is down.
3. Compare offsets with the Companion driver with care. That driver reads
   payloads without the report ID byte, so its offsets are one lower.

Source of the protocol words: the public Bitfocus Companion surface driver
`bitfocus/companion-surface-mirabox-stream-dock`, file `src/streamdock.ts`.
It lists `vendorId 0x0300` and `productIds [0x3010]` for the AJAZZ AKP-153E,
which is exactly this unit.

## 6. Key layout

Keys 1 to 15 fill the grid row by row, left to right.
Keys 16 to 18 are the strip keys.

| Key | ID | Image |
| --- | --- | --- |
| 1 to 5 | `0x0D 0x0A 0x07 0x04 0x01` | 96x96 |
| 6 to 10 | `0x0E 0x0B 0x08 0x05 0x02` | 96x96 |
| 11 to 15 | `0x0F 0x0C 0x09 0x06 0x03` | 96x96 |
| 16 to 18 | `0x10 0x11 0x12` | 80x80 |

The user confirmed the layout with numbered tiles: correct order, correct
orientation, no mirroring.

## 7. Code map

| File | Content |
| --- | --- |
| `crates/core/src/hid.rs` | Win32 HID enumeration, caps dump, overlapped read and write |
| `crates/core/src/proto.rs` | Report framing, command words, key decode, hex helpers |
| `crates/core/src/model.rs` | Key table, button and display split, image sizes, rotation |
| `crates/core/src/image.rs` | JPEG decode, rotate, scale, crop, encode under the limit |
| `crates/core/src/text.rs` | 5x7 bitmap font and word wrap |
| `crates/core/src/animation.rs` | GIF decode, compositing, delay handling, frame sampling |
| `crates/core/src/tile.rs` | Tile renderer: numbers, labels, frames, noise |
| `crates/core/src/device.rs` | High level `Device`: session, commands, image retry, events |
| `crates/core/src/session.rs` | Reconnect and hot plug over `Device` |
| `crates/core/src/error.rs` | Error type |
| `crates/store/src/profile.rs` | Profile, page, key, image and action model, validation |
| `crates/store/src/file_store.rs` | One JSON file per profile, atomic save, zip export and import |
| `crates/render/src/lib.rs` | Key tiles from a profile, contact sheet |
| `crates/render/src/text.rs` | TrueType labels with wrap and shrink, bitmap fallback |
| `crates/render/src/svg.rs` | SVG rendering through resvg |
| `crates/render/src/icons.rs` | Bundled icon set |
| `crates/render/assets` | Roboto font (Apache 2.0) and the icon SVGs |
| `crates/actions/src/lib.rs` | Hotkey, text, media, launch, multi, page actions |
| `crates/ipc/src/lib.rs` | Loopback HTTP server, token, shared state, commands |
| `crates/ipc/src/http.rs` | Bounded HTTP request parse and response write |
| `crates/ipc/src/ui.html` | The editor page |
| `crates/daemon/src/main.rs` | `streamdockd`: paint, poll, act, repaint, serve the editor |
| `crates/daemon/src/panel.rs` | Panel thread: device owner, command queue, key reader |
| `crates/daemon/src/painter.rs` | Tile encoding, animation clock, animation cache |
| `crates/daemon/src/tray.rs` | Tray icon and its menu on its own thread |
| `crates/daemon/src/single.rs` | Named mutex, one daemon per machine |
| `crates/cli/src/autostart.rs` | Per-user Run key install and remove |
| `crates/cli/src/main.rs` | Command line and command handlers |

Commands: `list`, `desc`, `init`, `watch`, `capture`, `brightness`, `led`,
`clear`, `image`, `pattern`, `probe`, `bench`, `profile`, `preview`, `send`.

The crate is a workspace since M1. `streamdock-core` holds the protocol and
the device. `streamdock-store` holds the profile model. `streamdock-render`
draws tiles. `streamdock-actions` injects input. `streamdockd` is the daemon.
`streamdock` is the command line tool.

## 8. Build

The machine has no MSVC Build Tools. Use the GNU target.
Rust 1.99.1 `x86_64-pc-windows-gnu` is installed.

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;C:\Users\plesm\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained;$env:Path"
cargo build --release
```

1. WinLibs provides the linker and the runtime libraries.
2. The rustup `self-contained` directory holds the compiler builtins.
3. Put both directories before the rest of `PATH`.

Cargo reports exit code 1 in this shell because it writes its progress to
stderr. Read the `Finished` line, not the exit code.

## 9. Pitfalls found the hard way

1. `windows-sys` 0.61:
   * `CreateFileW` needs the feature `Win32_Security`.
   * `GENERIC_READ` and `GENERIC_WRITE` live in `Win32::Foundation`.
   * `HANDLE` is `*mut c_void`.
   * HID functions return `bool`.
   * `PHIDP_PREPARSED_DATA` is `isize`.
   * `SetupDiGetClassDevsW` returns `isize`. Compare with
     `INVALID_HANDLE_VALUE as isize`.
   * `HidD_GetProductString` takes a length in bytes.
   * `HIDP_STATUS_SUCCESS` is `0x0011_0000`, not `0`.
2. Windows denies user-mode access to the keyboard collection, even for read.
   Do not try to use `MI_01`. Key events come from the vendor collection.
3. A plain `ReadFile` blocks forever when the device is idle. Use overlapped
   I/O with a timeout event.
4. `CancelIoEx` followed by `WaitForSingleObject(INFINITE)` can hang on this
   driver. Use a bounded wait of 2000 ms and return an error when it expires.
   An earlier build hung for 20 minutes because of this.
5. `IOCTL_HID_GET_REPORT_DESCRIPTOR` (`0x000B_0007`) returns "Incorrect
   function" from user mode on this driver. The registry value
   `HIDDeviceReportDescriptor` does not exist either. Use `HidP_GetCaps`,
   `HidP_GetValueCaps` and `HidP_GetButtonCaps` instead.
6. The panel sends no key report until the host sends a session command.
   `capture` sends `CONNECT` first and repeats it every 8 seconds.
7. The panel rotates every image. Rotate 270 degrees clockwise before the JPEG
   encode. Rotation by 90 degrees looks correct at a glance but is upside-down.

## 10. Environment notes

1. The working directory of the old session was
   `C:\Users\plesm\OneDrive\Desktop\CHAT`. That folder holds unrelated projects.
2. The crate now lives in `C:\Users\plesm\OneDrive\Desktop\streamdock-rs`.
3. Test tiles are in `%TEMP%\sd-tiles`. They are generated files and can be
   deleted.
4. A C# P/Invoke probe, used before the Rust crate existed, is at
   `%TEMP%\hidprobe.cs`. It is not part of the project.

## 11. Open items

1. ~~Press strip keys 16 to 18 and record their IDs.~~ Closed. The side strip
   has no buttons. It is three 80x80 displays and sends no key report.
2. Measure the LED array and LED brightness commands. They write fine, but the
   effect is not verified. This unit may have no LED ring.
3. Check the brightness range. The vendor driver maps a percentage with
   `pow(percent/100, 0.75) * 100`. The crate implements both forms:
   `proto::cmd::brightness` sends the raw value, and
   `proto::cmd::brightness_curved` applies the curve. Compare them at 25, 50,
   75 and 100.
4. ~~Split the crate into a library and a thin binary.~~ Closed in M1.
5. ~~Add unit tests for `proto::frame`, `proto::parse_input` and
   `image::rotate`.~~ Closed. 39 tests, no device needed.
6. ~~Add a retry path for a failed image upload.~~ Closed. `Device::send_image`
   tries three times with a 20 ms pause.
7. If the vendor application is ever installed, compare its traffic with this
   protocol. It is not installed now.
8. Measure sleep and resume. Unplug and replug is verified. A system sleep was
   not tested.
9. Measure the exact image size boundary. 17868 bytes works only as the first
   upload of a session. Two or more images above 10 KB corrupt the later keys.
   The safe rule is 10 KB per image, which is verified for all 15 keys.

## 12. Safety rules for the next agent

1. Read commands are harmless. Write commands change the panel state.
2. The `send` command needs `--i-understand`. Keep this guard.
3. Do not run the vendor installer to learn the protocol. The protocol is
   already known and verified.
4. Power cycle the device if it stops to respond.
5. The user wants short, plain sentences. Keep the ASD-STE100 style of the
   existing files.

## 13. The target: a full replacement

The user wants a replacement that works for daily use, and then becomes a
retail product. This section states the gap between the current crate and that
target.

### 13.1 Evidence about the vendor application

All facts below come from the embedded file catalog of the installer. The
application code was not decompressed.

| Evidence in the package | What it implies |
| --- | --- |
| `StreamDockAJAZZ.exe`, Qt5 modules, `libcef.dll`, `QCefView.dll` | The user interface is a CEF web view inside Qt |
| `node20.exe`, `plugin.js`, `manifest.json`, `.sdPlugin` | A plugin host in JavaScript |
| Plugins for OBS, Photoshop, ETS2, Discord, Twitch, YouTube, Spotify, vMix | An action plugin for each integration |
| `robotjs.node`, `libnut2.node` | Keystroke injection and screen capture from JavaScript |
| `ffmpeg6.dll`, `7z.dll` | Media decode, animated images, archive handling |
| `ScreenCaptureTool.exe` | Window and screen capture for key images |
| `FirmwareUpgradeTool.exe`, `upgcmdHid.exe` | Device firmware update over HID |
| `Watcher.exe`, `UpDateToolV3.exe`, `SplashScreen.exe` | Background update helper and splash |
| 17 `*.streamDockProfile` files | Profile system, plus per-application profiles |
| Qt `Svg` and `Multimedia` modules | SVG icons and video or audio preview |
| 28 UI languages in the bootstrapper | Localization |

### 13.2 Parity table

| Feature | Vendor | Current crate | Work needed |
| --- | --- | --- | --- |
| Device discovery and hot plug | yes | yes, no hot plug | Watch `WM_DEVICECHANGE`, reopen loop |
| Key events | yes | yes, verified | Publish as an API, not only a CLI |
| Per key image | yes | yes, verified | None for the transfer itself |
| Brightness | yes | writes only | Measure the curve on hardware |
| LED and strip colour | unknown | writes only | Measure. The strip may be one display |
| Pages, folders, multi actions | yes | none | Data model and action engine |
| Per application profiles | yes, 17 profiles | none | Foreground window watcher |
| Keystroke and text injection | yes, robotjs | none | `SendInput`, layout aware text |
| Media keys, launch app, open URL | yes | none | Action types |
| Plugin host | yes, node20 | none | WASM host, or a node sidecar |
| Icon library, SVG, text on key | yes, Qt Svg | JPEG only | Renderer with `resvg` and a text shaper |
| Animated images | yes, ffmpeg | none | `gif` crate, frame timer in the daemon |
| Screen and window capture | yes | none | Windows Graphics Capture |
| Profile import and export | yes | none | JSON or ZIP with assets |
| Tray icon and autostart | yes | none | Daemon responsibilities |
| Firmware update | yes | none | **Out of scope.** High risk |
| Auto update | yes | none | Own signed manifest |

### 13.3 What "usable" needs beyond the table

1. A daemon that owns the device. The panel must keep working when the window
   is closed.
2. Atomic image update. The vendor driver retries three times. This crate must
   do the same and must not leave a half written key.
3. Reconnect after sleep, resume and unplug. The panel is a keyboard for the
   user, so it must come back without a restart.
4. Bounded queues. One image is 1025 bytes per report. An animation can starve
   key events if the writer blocks the reader.
5. A single instance guard, and a clear exit that blanks the panel.

## 14. Frontend application

### 14.1 Choice of stack

Recommended: **Tauri 2** with a Rust daemon and a web UI.

Reasons:

1. The backend is already Rust. The device crate plugs in directly.
2. A web UI gives fast iteration for a drag and drop editor.
3. Tauri uses the system web view. No 200 MB CEF bundle, unlike the vendor app.
4. The installer is small, and Tauri ships an MSIX or NSIS path.

Alternatives, and why they lose:

1. Qt and C++: matches the vendor, but a second language and a heavy toolchain.
2. `egui`: pure Rust, but a poor fit for a rich editor and for retail polish.
3. Electron: easy, but large and needs Node at run time.

### 14.2 Process layout

```
+---------------------+        WebSocket over 127.0.0.1        +-------------+
|  streamdockd         | <------------------------------------> |  Tauri UI   |
|  tray, device,       |        JSON events and commands         |  (web view) |
|  action engine,      |                                        +-------------+
|  profile store       |
+----------+----------+
           | USB HID
    AJAZZ AKP-153E
```

Rules:

1. The daemon starts at logon and owns the HID handle.
2. The UI is only a client. Closing it must not stop the keys.
3. The socket binds to loopback on a random port. A token file in
   `%LOCALAPPDATA%\StreamDock\token` authenticates each client.
4. A second UI instance attaches to the running daemon. It does not start a
   second device owner.

### 14.3 Workspace layout

```
streamdock/
  crates/core       protocol, device, reconnect        (today's crate)
  crates/render     tile renderer: text, SVG, GIF, JPEG
  crates/actions    action engine and action types
  crates/daemon     tray, IPC, store, scheduler        (binary: streamdockd)
  apps/desktop      Tauri shell and web UI
```

Keep `crates/core` free of UI and of policy. It exposes:

```rust
pub struct Device { .. }
impl Device {
    pub fn open_first() -> Result<Self>;
    pub fn send_image(&mut self, key: u8, jpeg: &[u8]) -> Result<()>;
    pub fn set_brightness(&mut self, percent: u8) -> Result<()>;
    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<KeyEvent>>;
}
```

### 14.4 Data model

Store profiles as one JSON file per profile plus an assets folder. Use it
before a database. It is easy to back up, diff and import. Add a database only
when the data grows.

```json
{
  "version": 1,
  "profile": "Default",
  "device": { "pid": "0x3010", "layout": "akp153e" },
  "pages": [
    {
      "name": "Main",
      "keys": {
        "1": {
          "label": "Mute",
          "image": { "kind": "icon", "name": "mic-off", "background": "#202020" },
          "actions": [ { "type": "hotkey", "keys": ["ctrl", "shift", "m"] } ]
        }
      }
    }
  ]
}
```

### 14.5 Renderer order of operations

Measured requirement: rotate by 270 degrees at the end, not at the start.

1. Compose the tile at 96x96 or 80x80 in RGB.
2. Draw the background, the icon and the text upright.
3. Rotate the finished tile 270 degrees clockwise.
4. Encode as JPEG with a quality search, from 90 down, until the result is
   10240 bytes or less.
5. Cache the encoded JPEG by the hash of the tile description.

### 14.6 UI scope for the first release

1. A grid that matches the panel: 5 columns by 3 rows plus 3 strip keys.
2. Click a key to edit it: icon, text, colour, actions.
3. Drag an image file onto a key.
4. Drag a key onto another key to swap.
5. Live preview: the panel updates while the user edits.
6. A page list, with add, copy, rename and delete.
7. Brightness slider and a panel test pattern.
8. Import and export of a profile as one ZIP file, with assets.

### 14.7 Action types for parity

| Type | Parameters | Notes |
| --- | --- | --- |
| `hotkey` | modifier list, key name | `SendInput`, layout aware |
| `text` | string | Type the string, optional delay |
| `media` | play, next, previous, volume | Consumer control usage |
| `launch` | path or URL | `ShellExecuteW` |
| `multi` | ordered list, delays | Runs the list in sequence |
| `page` | target page | Switch page |
| `profile` | target profile | Switch profile |
| `http` | method, URL, headers, body | Local integrations |
| `command` | program, arguments | Opt in, warn the user |
| `plugin` | plugin ID, action, settings | Later, WASM |

## 15. Retail checklist

1. Installer: per user, no admin rights for HID access. MSIX or NSIS.
2. Code signing: an EV certificate. SmartScreen reputation takes time.
3. Privacy statement. The app can inject keystrokes, so state this clearly.
4. No telemetry by default. Opt in only, and document it.
5. Crash log to `%LOCALAPPDATA%\StreamDock\logs`. Rotation, not endless files.
6. Autostart at logon, tray icon, and a visible "quit" action.
7. Localization from the start. The vendor ships 28 languages.
8. Third party licenses: `resvg`, `image`, `jpeg-decoder`, `jpeg-encoder`,
   `gif`, `tauri`. All permissive. Keep a NOTICE file.
9. Update check against a signed manifest. Verify the signature before install.
10. Known limitation for the docs: injected keystrokes can be blocked by games
    that use kernel level input filtering.

## 16. Milestones

| Milestone | Content | Done when |
| --- | --- | --- |
| M0 (done) | Protocol, CLI, hardware verification | Keys, images, brightness tested |
| M1 (done) | Split into `crates/core`, add reconnect and hot plug, add unit tests | Unplug and replug recovers with no restart |
| M2 (done without IPC) | Daemon, profile store, action engine for hotkey, text, launch, multi | Keys act while the UI is closed |
| M3 (part done) | Key grid, key editor, live preview in a browser page | A user builds a 15 key page without a CLI |
| M3 | Tauri UI with grid, key editor, drag and drop, live preview | A user builds a 15 key page without a CLI |
| M4 (done) | Text, images, SVG, icons, GIF, phases, themes, zip export and import | A page with icons and one animation runs |
| M5 | Per application profiles, HTTP actions, plugin host | A profile switches when the user changes application |
| M6 | Signing, installer, update check, docs, privacy text | A stranger can install and use it |

## 17. Open hardware questions

Measure these on the unit before the design is frozen.

1. ~~Are strip keys 16 to 18 separate displays or one display?~~ Answered.
   Three separate 80x80 displays. A full 80x80 frame shows with all four sides.
2. ~~Is the strip touch sensitive?~~ Answered. No. It sends no report.
3. Does this unit have an LED ring? `LBLIG` and `SETLB` write without error,
   but no visible effect was confirmed.
4. What brightness curve does the firmware use? Compare 25, 50, 75 and 100.
5. ~~What is the real image size limit?~~ Answered. 10 KB per image is safe for
   all 15 keys. A single 17868 byte image works as the first upload only.
6. ~~How fast can reports be sent back to back?~~ Measured. 30 uploads of a
   1748 byte tile take 18 ms in total, about 0.6 ms per upload. The API accepts
   the data faster than the panel can show it, so this is not a frame rate.
7. ~~Does the panel keep the last image after a power cycle?~~ Answered. No.
   The panel shows only its start logo after a replug.
8. Does the panel come back after sleep and resume without a reopen? Not
   tested. Unplug and replug is verified.

## 18. Risk list

1. Firmware update is out of scope. A bad image can brick the device.
2. The 10 KB limit forces low quality for photo icons. A renderer that
   simplifies artwork matters more than raw quality.
3. Key injection can conflict with games, remote desktop clients and other
   macro tools.
4. A background daemon that injects keystrokes looks like malware to some
   security products. Sign the binaries and keep the behaviour documented.
5. The vendor protocol is a family protocol. Another firmware version of the
   same model may use other words. Probe with `desc` and `capture` first.
6. The panel keeps no image across a power loss. A daemon must keep every tile
   in memory and repaint after `Connected`, or the panel shows its start logo.

## 19. Session 2: M1 and new hardware evidence

Date: 2026-11-01. This session did the following work.

1. Split the crate into the workspace `crates/core` and `crates/cli`.
   The binary is still `streamdock`.
2. Added `Device` with `open_first`, `start_session`, `send_image` (three
   tries), `set_brightness`, `set_brightness_curved`, `clear_key`, `blank`,
   `poll_event`, `read_raw` and `write_report`.
3. Added `Session` with reconnect. It drops a dead handle and reopens the
   panel. Reopen tries are limited to one every 500 ms. A failed call is
   retried once after the reopen.
4. Added 39 unit tests for the pure functions.
5. Added the `watch`, `pattern`, `probe`, `bench`, `led` and `clear`
   commands.
6. Fixed the digit size in the test tile. 1 to 9 and 10 to 18 now use the
   same glyph scale.
7. Added `--fit crop|stretch|none` and the bilinear `resize` and `fit_cover`
   functions. A dropped photo now fills the key without distortion.

Hardware results from this session:

| Test | Result |
| --- | --- |
| Strip is a button | No. No report arrives. |
| Strip shape | Three 80x80 displays, all four frame sides visible |
| Panel order and rotation | Correct, marker at the top left on every key |
| Unplug and replug | `Disconnected`, then `Connected` within one poll |
| Images after replug | Lost. Only the start logo is shown. |
| 15 keys at 9264 bytes | All complete |
| 4 keys at 17868 bytes | Only the first is complete |
| Same, with a 300 ms pause | Only the first is complete |

The last two rows are the important new rule. The panel does not keep large
images for more than one key. A daemon must hold a copy of every tile and
must repaint after `Connected`.

Two more notes for the next session:

1. `image::encode_quality` uses the crate default subsampling. The JPEG size
   jumps between quality 89 (9264 bytes) and 90 (17868 bytes) because the
   encoder switches sampling at 90. Keep this in mind when you tune quality.
2. `probe --test image-limit --qualities ... [--pause MS]` is the tool for
   more image tests. It sends the same noise tile to keys 1..N.

## 20. Session 2 part 2: M2 daemon, store and actions

Date: 2026-11-01. M2 is done apart from the IPC channel. The IPC channel moves
to M3, because the UI is the first client that needs it.

New crates:

| Crate | Content |
| --- | --- |
| `streamdock-store` | Profile, page, key, image and action model. One JSON file per profile. Atomic save. Validation. |
| `streamdock-render` | Tiles from a profile: colour, text, image file. One text scale per page. Contact sheet. |
| `streamdock-actions` | Hotkey, text, media, launch, multi, page, brightness and command actions. |
| `streamdockd` | Owns the panel. Paints a page, polls keys, runs actions, repaints, reconnects, blanks on exit. |
| `streamdock` | Added `profile list/init/demo/show/validate` and `preview`. |

Measured on hardware:

| Test | Result |
| --- | --- |
| First paint of 18 keys | 235 ms after `panel connected` |
| Page switch repaint | 20 ms from the key report |
| Single instance guard | The second `streamdockd` exits with code 1 |
| Blank on exit | Keys clear after the stop |
| Label size, whole page | One scale for every label. Confirmed "all even". |
| Reconnect repaint | The daemon repaints after `Connected` |

Design decisions:

1. The renderer picks one glyph scale for a whole page, so `1` and `10` match.
   The scale is capped at 6 and the text keeps an 8 pixel side margin.
2. A render fault draws a red `ERR` tile, so a broken image file does not
   leave a stale key.
3. `page_scale` ignores keys without text, so a photo key does not shrink the
   labels.
4. The daemon runs the actions of a press only. It ignores the release, so an
   action fires once.
5. `Action::Command` runs a program. The daemon accepts it because the profile
   is user data. A release build must ask for an explicit opt in first.

Open work that this session added:

1. ~~Test hotkey and text injection in a real application.~~ Closed. Notepad
   showed the typed text, `ctrl+a` selected it, and a key launch opened
   Notepad. One press in the first test was not logged, so a press can be
   lost while the daemon paints. A worker thread fixes this.
2. Add the IPC channel: loopback socket, token file, JSON events and commands.
3. Add a tray icon and autostart. Both belong to the daemon.
4. Cache encoded tiles by the hash of the key description. A repaint now
   re-renders and re-encodes all 18 keys.
5. Watch the profile file for changes, so `streamdock profile` edits show
   without a daemon restart.
6. Send `CONNECT` on a timer even while the daemon paints. A long repaint
   blocks the poll loop, but 18 keys take 235 ms, so this is not a problem
   today. An animation will need a worker thread.

## 21. Session 2 part 3: IPC server and browser editor

Date: 2026-11-01. This session added the `streamdock-ipc` crate and an editor
page. The editor is a normal web page, not a Tauri window.

Why a web page first:

1. Tauri 2 on Windows usually needs the MSVC toolchain. This machine has the
   GNU toolchain only, so a Tauri build is a risk with no gain yet.
2. The page is one HTML file with no build step and no runtime.
3. The same page can go into a Tauri shell later, when the toolchain exists.
4. The daemon already owns the device and the store, so the page needs only a
   small HTTP API.

Measured on hardware:

| Test | Result |
| --- | --- |
| Daemon starts the server | Prints the URL, writes `runtime.json` |
| `GET /api/state` | Profile, page, connection and brightness |
| Page switch over HTTP | Panel repaints the other page |
| Brightness over HTTP | Panel dims |
| Save a key over HTTP | Profile written to disk, panel repaints |
| Request without the token | 401 |
| Editor page in the browser | Grid and editor shown |
| Edit a key in the page | Panel shows the new label within a second |

API:

| Method | Path | Body |
| --- | --- | --- |
| GET | `/` | token in the query, serves the page |
| GET | `/api/state` | none |
| PUT | `/api/profile` | the whole profile as JSON, `X-Rev` to refuse a stale save |
| POST | `/api/page` | `{"name": "Main"}` or `{"index": 0}` |
| POST | `/api/brightness` | `{"value": 70}` |
| GET | `/api/tile` | token in the query, a JPEG of one upright tile |
| POST | `/api/asset` | raw image bytes, name in the query, returns the path |
| POST | `/api/reload` | `{}` |

Rules:

1. The listener binds `127.0.0.1` with port 0, so the port is random.
2. The token is 32 hex characters in `%LOCALAPPDATA%\StreamDock\runtime.json`.
3. The page needs the token in the query, the API needs it in `X-Token`.
4. A connection serves one request and closes.
5. The body is limited to 1 MiB and the header to 64 KiB.
6. Only the daemon thread touches the device. The server sends a command and
   waits up to 5 seconds for the answer.
7. A save replaces the whole profile. The store validates it before the write.
8. A save never leaves a half written file, because the store uses a temp file
   and a rename.

Open work for the editor:

1. ~~Drag and drop an image file onto a key.~~ Closed. A JPEG or PNG drop is
   copied to `assets` and becomes the key image.
2. ~~Draw the real tile in the browser, not an approximation.~~ Closed. The
   grid loads `/api/tile`, which renders the same tile as the panel.
3. ~~Undo, and a warning when a second client saves over a change.~~ Closed in
   section 24.
4. Show the render fault of a key in the grid.
5. Add the strip key names to the grid labels.
6. Keep the last known state when the daemon is offline, instead of a blank
   grid.
7. ~~Add the tray icon and autostart to the daemon.~~ Closed in section 23.
8. Wrap the page in Tauri when the MSVC toolchain is available.

## 22. Session 2 part 4: real tiles and drag and drop

Date: 2026-11-01. Two editor features are now measured on hardware.

| Test | Result |
| --- | --- |
| Grid shows real tiles | The page draws `/api/tile` for every key |
| Drop a PNG on a key | File copied to `assets`, panel shows the image |
| Tile endpoint | 96x96 JPEG of the upright tile |
| Asset endpoint | Rejects a file that is not an image |
| Relative asset path | Resolved against the profile folder |

Design notes:

1. The tile endpoint draws the upright tile. The panel rotation is added only
   on the way to the device.
2. A dropped file is decoded before it is stored, so a renamed text file never
   reaches a profile.
3. The stored name is `<stem>-<hash8>.<ext>`, so the same file always lands on
   the same name and the browser cache stays correct.
4. The asset name is sanitized to lowercase letters, digits and single dashes.
   A path separator cannot escape the `assets` folder.
5. The body limit is 8 MiB. The PNG side limit is 4096 pixels.
6. `image::decode` now reads PNG as well as JPEG. An indexed PNG is rejected
   with a clear message.
7. The editor fetches a tile again only when the profile, the page or the
   connection state changes. A poll does not reload 18 images.

## 23. Session 2 part 5: tray icon, autostart and a clean exit

Date: 2026-11-01. The daemon can now run at logon with no console and no
window, and the user can stop it from the tray.

| Test | Result |
| --- | --- |
| Tray icon | Visible, tooltip `Stream Dock` |
| Tray menu | Open editor, Reload profile, Quit |
| Open editor | Opened the page in the default browser |
| Quit | Daemon stopped, keys cleared, icon removed |
| `autostart status` | `not set` before install |
| `autostart install` | Wrote the Run value |
| `autostart status` | Showed the quoted path |
| `autostart remove` | Deleted the value, status back to `not set` |

Design notes:

1. The icon lives on its own thread with a hidden window and a message loop.
   The device loop keeps its single owner.
2. The menu is a popup menu with `TPM_RETURNCMD`, so no `WM_COMMAND` handler is
   needed.
3. Quit sets the same shutdown flag as Ctrl+C. The loop then drops the tray,
   which posts `WM_CLOSE`, the tray thread removes the icon and joins, and the
   keys are cleared.
4. The icon uses `IDI_APPLICATION` for now. A product build needs its own icon
   resource.
5. Autostart uses `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. The
   value is the quoted daemon path, plus `--profile NAME` when given.
6. The command finds `streamdockd.exe` next to `streamdock.exe`, so both must
   ship in one folder.

Open work that this session added:

1. A product icon and a signed binary.
2. A tray item for autostart, so the user does not need the CLI.
3. A single instance check before the tray starts, so a second start does not
   flash an icon.
4. Hide the console window in a release build. The current binary is a console
   program.

## 24. Session 2 part 6: undo and save conflicts

Date: 2026-11-01. Two editor safety features are now measured on hardware.

| Test | Result |
| --- | --- |
| Undo after a key save | The panel returned to the previous label |
| Ctrl+Z outside a text field | Same as the Undo button |
| Two tabs, stale save | The second tab showed the conflict warning |
| Second press of Apply | Overwrites on purpose |
| Save without `X-Rev` | Accepted, so a force save still works |
| Stale rev over HTTP | 409 with the current state, daemon logs the refusal |

Design notes:

1. `State.rev` counts profile changes. A page switch does not change it.
2. The editor remembers the revision that it loaded the key editor with. A
   save after another window changed the profile is refused.
3. The daemon compares the revision, not the HTTP layer, so the check cannot
   race with a paint.
4. `Reply` is now an enum: `Ok(state)`, `Conflict(state)` and `Error(message)`.
   The HTTP layer maps `Conflict` to 409 with the current state.
5. A save without `X-Rev` overwrites. The editor uses that only after it shows
   the warning and the user presses the button again.
6. The undo stack keeps 30 profiles in the browser. Undo sends the previous
   profile as a normal save, so it is checked and repainted like any change.
7. The stack is in the page, not on disk. A reload of the page clears it.

Open work that this session added:

1. A merge view for a conflict, instead of reload or overwrite.
2. Redo.
3. Name the page and the key in the conflict message.

## 25. Session 2 part 7: animated GIF keys and the panel thread

Date: 2026-11-01. A key can now run a GIF animation, and the device has its
own thread so the heartbeat and the key reader never stop.

| Test | Result |
| --- | --- |
| Four frame GIF on key 1 | Red, green, blue and white cycle |
| Key press while the key animates | The page switched at once |
| Stale save during animation | Still refused with a conflict |
| GIF upload through the editor | Accepted, stored as `assets/<name>-<hash>.gif` |

Architecture:

1. The panel thread owns the `Session`. It drains a bounded command queue for
   at most 40 ms, then reads key reports for a 20 ms slice.
2. The coordinator thread keeps the profile, the actions, the editor server
   and the animation clocks. It encodes images and queues them.
3. A frame is pre-encoded once per key size and cached as an `Arc<[u8]>`, so a
   frame tick is only a queue write plus a reference count.
4. The panel thread drops writes while it is disconnected, so an animation
   cannot trigger a reconnect storm.

Limits and rules:

1. At most 60 frames per animation. A longer GIF is sampled down, and the
   delays of skipped frames are added to the frames that stay, so the loop
   duration is unchanged.
2. A frame delay is clamped to 20 ms or more, and to 2000 ms or less.
3. The animation cache holds 64 entries and is cleared when it is full.
4. A transparent GIF pixel shows black, because the frames are composited
   onto black.
5. The command queue holds 1024 writes. When it is full, a frame is dropped
   and the next tick tries again.
6. The editor preview and the contact sheet show frame 0 only.

Open work that this session added:

1. Give each key its own phase, so ten animated keys do not change together.
2. Cap the total animation memory across keys, not only the frame count.
3. Run slow actions (a long text action) on a worker, so the coordinator keeps
   the animation clocks exact.
4. Show a moving frame in the editor, not only frame 0.
5. Measure the real frame rate limit on the panel with many animated keys.

## 26. Session 2 part 8: font, SVG, animation phases and a memory budget

Date: 2026-11-01. The renderer now uses a real font and draws SVG icons.
Animations start at their own phase, and their memory has a budget.

| Test | Result |
| --- | --- |
| TrueType label | `MUTE` and `Scene 2` are readable, mixed case kept |
| SVG icon | A microphone icon on key 3 |
| Animation phase | Keys 4 and 5 run the same GIF out of step |
| Frame budget | A large animation is merged until it fits |

Design notes:

1. Text uses `ab_glyph` with a system font: `segoeui.ttf`, then `arial.ttf`,
   `tahoma.ttf` and `verdana.ttf`. `STREAMDOCK_FONT` overrides the choice.
2. The page picks one pixel size for its labels. The size is the smallest
   value that fits every label on one line, capped at 42 percent of the key.
   That keeps `1` and `10` the same size.
3. A label that still does not fit wraps, and then the size shrinks.
4. The 5x7 bitmap font is the fallback when no font file is found.
5. An `icon` reads `<profile folder>/icons/<name>.svg`, then falls back to the
   name as text.
6. SVG is rendered with `resvg` into a pixmap that is first filled with the
   key background, so the result is opaque.
7. Each animation starts at `(key - 1) * 120 ms` into its loop unless the
   profile sets `phase_ms`.
8. An encoded animation is merged frame by frame until it fits 1.5 MB. The
   loop time does not change.
9. The animation cache is cleared when it holds 64 entries or 24 MB.

Open work that this session added:

1. Bundle a font with an open licence, and an icon set, so a product build is
   not tied to Windows fonts.
2. Let the editor upload an icon into `icons/` with a name.
3. Add a "fit" choice for SVG (contain or cover).
4. Support SVG text. `usvg` needs a font database for it.

## 27. Session 2 part 9: bundled font and icons, themes, zip import and export

Date: 2026-11-01. The renderer now ships its own font and icons, a profile can
carry a theme, and a profile moves between machines as one zip file.

| Test | Result |
| --- | --- |
| Bundled font | Roboto labels on the panel |
| Bundled icons | play, gear, check, volume, camera drawn on key 6 to 10 |
| Profile icon | `icons/mic.svg` still wins over the bundled set |
| Theme | Purple background and yellow text from the profile |
| Editor theme pickers | Show the profile colours, a change repaints the panel |
| Icon suggestions | The icon field lists the bundled names |
| Icon upload | A named SVG lands in `<profiles>/icons/<name>.svg` |
| Export | Zip holds the profile, the GIF and `icons/mic.svg` |
| Import | The zip loads under a new name with its assets |

Design notes:

1. The font is `crates/render/assets/Roboto-Variable.ttf`, Apache 2.0. The
   licence text is next to it and must ship with a release.
2. `STREAMDOCK_FONT` still overrides the bundled font, and the system fonts are
   the last fallback.
3. The icon lookup order is: profile `icons/<name>.svg`, bundled icon, then
   the name as text.
4. `Theme` is part of the profile. It sets the background and the text colour
   for keys that do not set their own.
5. `Store::export` writes `<name>.json` plus every relative path that the
   profile refers to.
6. `Store::import` writes only entries under `assets/` and `icons/`, and it
   rejects a parent directory step, so an archive cannot escape the folder.
7. The editor uploads an icon with `POST /api/icon?name=`, and lists names
   with `GET /api/icons`.

Open work that this session added:

1. Import and export buttons in the editor, not only in the CLI.
2. A theme editor for named themes, not only two colours.
3. Bundle a few more icons, and a small preview of each in the editor.
4. Check the Apache 2.0 notice in the installer and the about box.

