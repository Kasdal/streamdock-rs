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
| `src/hid.rs` | Win32 HID enumeration, caps dump, overlapped read and write |
| `src/proto.rs` | Report framing, command words, key decode, hex helpers |
| `src/model.rs` | Key table, image sizes, rotation value |
| `src/image.rs` | JPEG decode, rotate, shrink under 10 KB |
| `src/main.rs` | Command line and command handlers |

Commands: `list`, `desc`, `init`, `brightness`, `image`, `capture`, `send`.

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

1. Press strip keys 16 to 18 and record their IDs. The table says `0x10` to
   `0x12`. Reports for those IDs were not seen.
2. Measure the LED array and LED brightness commands. They write fine, but the
   effect is not verified. This unit may have no LED ring.
3. Check the brightness range. The vendor driver maps a percentage with
   `pow(percent/100, 0.75) * 100`. This crate sends the raw value.
4. Split the crate into a library and a thin binary if the next step is a
   service or a desktop application.
5. Add unit tests for `proto::frame`, `proto::parse_input` and
   `image::rotate`. These are pure functions and need no device.
6. Add a retry path for a failed image upload. The vendor driver retries three
   times with a 20 ms pause.
7. If the vendor application is ever installed, compare its traffic with this
   protocol. It is not installed now.

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
| M1 | Split into `crates/core`, add reconnect and hot plug, add unit tests | Unplug and replug recovers with no restart |
| M2 | Daemon, IPC, profile store, action engine for hotkey, text, launch, multi | Keys act while the UI is closed |
| M3 | Tauri UI with grid, key editor, drag and drop, live preview | A user builds a 15 key page without a CLI |
| M4 | Renderer with text, SVG, GIF, icon crop, themes, import and export | A page with icons and one animation runs |
| M5 | Per application profiles, HTTP actions, plugin host | A profile switches when the user changes application |
| M6 | Signing, installer, update check, docs, privacy text | A stranger can install and use it |

## 17. Open hardware questions

Measure these on the unit before the design is frozen.

1. Are strip keys 16 to 18 separate displays or one display? Press each strip
   area and record the key ID.
2. Is the strip touch sensitive? Read reports while a finger slides along it.
3. Does this unit have an LED ring? `LBLIG` and `SETLB` write without error,
   but no visible effect was confirmed.
4. What brightness curve does the firmware use? Compare 25, 50, 75 and 100.
5. What is the real image size limit? Try 10 KB, 20 KB and 40 KB JPEG.
6. How fast can reports be sent back to back? Measure the time for 30 image
   sends. This sets the animation frame rate.
7. Does the panel keep the last image after a power cycle?
8. Does the panel come back after sleep and resume without a reopen?

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

