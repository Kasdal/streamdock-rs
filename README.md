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
| Bundled font and icons | verified on hardware: Roboto labels, built-in icon set |
| Themes | verified on hardware: purple and yellow profile theme |
| Profile export and import | verified: zip with the profile, assets and icons |
| Editor export and import | verified over HTTP: download and upload |
| Release package | verified: install and uninstall into a temp folder |
| NSIS installer | verified: 7.9 MB setup with the window, silent install and uninstall |
| Per-app profiles | verified on hardware: chrome.exe switched the profile |
| HTTP actions | tested against a local server |
| Plugin host | tested with a PowerShell plugin |
| Signed update check | verified with a local signed manifest |
| Preloaded tools profile | verified on hardware: media, pages, Notepad |
| Desktop window | verified: one instance, native save dialog, window place |
| Side strip meters | verified on hardware: CPU, GPU and memory load |
| Side strip editor | verified: live pane, three widgets, weather lookup |
| Unit tests | 211 tests, no device needed |
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
The right side holds one tall display, 96 by 460 pixels.
The side strip has no buttons and sends no key event.

| Key | ID | Size | Button |
| --- | --- | --- | --- |
| 1 to 15 | `0x0D 0x0A 0x07 0x04 0x01` / `0x0E 0x0B 0x08 0x05 0x02` / `0x0F 0x0C 0x09 0x06 0x03` | 96x96 | yes |
| 16 | `0x10` | 96x460, the whole strip | no, display only |
| 17, 18 | `0x11 0x12` | part of the same strip | no, display only |

The strip is one display, not three. An image sent to key 16 covers it. The
device draws an image at its own size and squeezes an image that is taller
than the strip. Measured: 480 pixels cut the top label, 460 show everything.

The daemon owns the strip and draws the CPU, GPU and memory meters there. A
page paints keys 1 to 15 only.

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
| `scripts` | version scripts for the release workflow |
| `.github/workflows` | CI and release pipelines |
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

A release build has no console window. It writes to
`%LOCALAPPDATA%\StreamDock\logs\daemon.log`, and rotates that file at 1 MB.
A debug build also prints to the console.

The editor is one HTML page with a key grid and a key editor. Apply a key and
the panel changes within a second. The page needs no build step and no extra
runtime.

The grid shows the real tile that the daemon draws, not an approximation.
Drag a JPEG, PNG, GIF or SVG file onto a key to set its image. A GIF becomes
an animation. The file is copied into the `assets` folder next to the profile,
so the profile stays portable.

The header holds the two theme colours and the brightness slider. For an icon
key, type a name, choose an SVG file and the editor stores it in `icons/`.
The name field suggests the bundled icon names.

The Export button downloads the active profile as one zip. The Import button
replaces the active profile from a zip. Both use the same format as
`streamdock profile export` and `streamdock profile import`.

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

Measured: a page switch repaints the page in about 20 ms.

The side strip is one display with three widgets, one per third. The daemon
draws them, samples once a second and writes a new image only when something
changed.

| Widget | Shows | Needs |
| --- | --- | --- |
| System meter | CPU, GPU, memory or disk, with a name, a percentage and a bar | nothing |
| Clock | the time, the date, or both | nothing |
| Weather | place, temperature and a short condition | a network request, see `PRIVACY.md` |
| Empty | nothing | nothing |

| Source | Call | Notes |
| --- | --- | --- |
| CPU | `GetSystemTimes`, delta between two samples | whole machine |
| GPU | PDH `\GPU Engine(*)\Utilization Percentage` | busiest engine; `--` when the machine has no such counter |
| Memory | `GlobalMemoryStatusEx` | used of total physical memory |
| Disk | `GetDiskFreeSpaceExW` | used of the Windows drive |
| Clock | `GetLocalTime` | local time |
| Weather | Open-Meteo over WinHTTP | every 15 minutes, behind the widget |

Every source is user mode. No administrator rights and no driver are needed.

The editor shows the same picture the device gets. Click the strip in the
device picture, pick a widget for each of the three slots, then press Apply
strip.

The STRIP panel also has a **Text size** slider, from 60 % to 160 %. The
profile stores it as `screen.text_scale`. The band layout grows with the
setting, so 150 % is really bigger and not only a larger font inside the same
box. If the three lines need more room than a band has, the text shrinks to
fit.

`streamdock preview --name Demo --out sheet.jpg` writes a picture of one page
without the panel. Use it to check a profile on any machine.

## Profiles

One profile is one JSON file. A page holds keys by number.

```json
{
  "version": 1,
  "name": "Default",
  "device": { "pid": "0x3010", "layout": "akp153e" },
  "theme": { "background": "#202020", "foreground": "#f0f0f0" },
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

The theme sets the background and the text colour for keys that do not set
their own colour. The editor has two colour pickers for it.

`apps` holds process names. When one of them owns the foreground window, the
daemon switches to that profile. It returns to the profile it started with
when no rule matches. `--no-auto-switch` turns this off.

Action types: `hotkey`, `text`, `media`, `launch`, `multi`, `page`,
`brightness`, `command`, `http`, `plugin`. An `http` action needs an `http://`
URL; HTTPS is not supported yet. A `plugin` action calls
`<profiles>\plugins\<id>`; see the plugin section.

A `file` path may be absolute, or relative to the profile folder. The editor
stores a dropped file as `assets/<name>-<hash>.<ext>`. JPEG, PNG, GIF and SVG
are accepted.

An `icon` uses `<profile folder>/icons/<name>.svg` when that file exists, then
a bundled icon, then the name as text. Bundled names: `camera`, `check`,
`cross`, `folder`, `gear`, `home`, `mic`, `mic-off`, `minus`, `mute`, `next`,
`pause`, `play`, `plus`, `prev`, `refresh`, `volume`, `volume-down`.

An `animation` is a GIF. Each frame is fitted to the key, rotated and encoded
once. The daemon keeps at most 60 frames per animation and samples a longer
GIF down while keeping the loop time. A frame delay is clamped to 20 ms or
more. Each key starts at its own point in the loop unless `phase_ms` is set.

A `file` path may be absolute, or relative to the profile folder. The editor
stores a dropped file as `assets/<name>-<hash>.<ext>`. JPEG, PNG, GIF and SVG
are accepted.

Text uses the bundled Roboto font (Apache 2.0), so every installation looks the
same. Set `STREAMDOCK_FONT` to a font file to override it. The 5x7 bitmap font
is the fallback when no font can be loaded.

A `command` action runs a program, so a profile with one needs an explicit opt
in before a release build accepts it.

## Starter profile

A new install gets a `Default` profile with four pages. A first run writes it,
and `streamdock profile tools` writes it again at any time. The bottom row of
every page switches page.

| Page | Keys |
| --- | --- |
| Media | previous, play/pause, next, stop, mute, volume down, volume up |
| System | lock (Win+L), show desktop (Win+D), task manager, screenshot (Win+Shift+S), copy, paste, undo, redo, save, select all |
| Apps | Explorer, Notepad, Calculator, Paint, Settings, Command Prompt, browser, mail |
| Tools | panel brightness 30, 70 and 100, screenshot, a text demo, a two-step multi action |

The app shortcuts use `explorer.exe`, `notepad.exe`, `calc.exe`,
`mspaint.exe`, `ms-settings:`, `cmd.exe`, a web page and `mailto:`. Edit any
key in the editor to change it.

## Plugins

A plugin is a folder with a `manifest.json` and a program:

```json
{ "command": "plugin.exe", "args": [] }
```

The daemon starts the program, writes one JSON line and reads one JSON line:

```
out: {"action":"play","settings":{"volume":20}}
in:  {"ok":true}          or  {"error":"why"}
```

The plugin has 5 seconds to answer. Any language can write a plugin, and no
code runs inside the daemon. Plugins live in `<profiles>\plugins\<id>`.

## Update check

`streamdock update check --url URL` fetches `latest.json` and
`latest.json.sig`, checks the Ed25519 signature and compares the version. It
downloads nothing.

```json
{ "version": "0.3.1", "url": "https://example/StreamDock-0.3.1.zip",
  "sha256": "…", "notes": "Faster paint" }
```

The build ships the RFC 8032 test key as a placeholder. Replace
`PUBLIC_KEY_HEX` in `crates/cli/src/update.rs` with your release key.
`streamdock update sign --file latest.json --secret HEX` prints the signature.

## Privacy

`PRIVACY.md` states what the program stores, when it uses the network, and
what a key can do. The data lives in `%LOCALAPPDATA%\StreamDock`.

## Desktop app

`streamdock-desktop.exe` is the native window for the editor. It has no browser
chrome and its own task bar entry.

The editor page is bundled in the program, so the window opens even when the
daemon is down. The window finds the daemon through `runtime.json`. When the
daemon is not running, it starts `streamdockd.exe` next to itself and waits for
the API.

| Part | Behavior |
| --- | --- |
| Window | 1320 by 860 on the first run, dark Elgato-style editor |
| Window place | Comes back where it was, at the same size and state |
| Close button | Hides the window; the app keeps running in the tray |
| Tray menu | Show editor, Reload profile, Quit |
| Second start | The running window comes forward; the second copy exits |
| Export | Native save dialog, with the profile name as the suggested file |
| API | The page calls the daemon API with the token, from a CORS allowlist |

One copy of the window runs at a time. The app holds a named mutex, so a
second start hands its command line to the first copy and exits. The first
copy shows and focuses its window.

The window comes back where it was. The app writes the place, the size and the
maximized state to `%LOCALAPPDATA%\StreamDock\window.json`, 500 ms after the
window stops moving. A saved place that no monitor shows any more is dropped,
and the window opens centred instead.

Export asks for a file with the Windows save dialog. The dialog opens in the
Downloads folder and suggests `<profile>.zip`. The export command fetches the
zip before it shows the dialog, so a failed export never opens an empty save
window. If no daemon answers, the page shows the error and no dialog appears.
Cancel writes no file.

The desktop window is a Tauri 2 app, so it needs the MSVC toolchain. The rest of
the project builds with GNU, so the crate sits outside the workspace.

```powershell
.\build-desktop.ps1
.\crates\desktop\target\x86_64-pc-windows-msvc\release\streamdock-desktop.exe
```

`build-desktop.ps1` loads `vcvars64.bat`, adds the `x86_64-pc-windows-msvc`
target and builds.

## Install

`build-release.ps1` builds both binaries, embeds the app icon and writes a
portable package plus a zip under `dist\`.

```powershell
.\build-release.ps1
.\dist\StreamDock-0.3.1\install.ps1
```

`install.ps1` copies the program to `%LOCALAPPDATA%\StreamDock\bin`, adds the
per-user autostart value, creates a Start menu shortcut and writes an entry in
"Apps and features". Use `-NoAutostart` or `-NoShortcut` to skip either step,
and `-Target PATH` for another folder. `uninstall.ps1` removes the program. It
keeps profiles when you pass `-KeepProfiles`.

`build-nsis.ps1` builds a per-user NSIS installer from `packaging\streamdock.nsi`.
It needs `makensis` in PATH or a portable copy with `-MakensisPath PATH`. The
result is `dist\StreamDock-0.3.1-setup.exe`, about 7.9 MB. The installer needs
no administrator rights and writes an "Apps and features" entry.

```powershell
.\build-nsis.ps1
.\dist\StreamDock-0.3.1-setup.exe
```

`sign.ps1` signs both binaries with `signtool`. Set `STREAMDOCK_CERT` to a
`.pfx` file and `STREAMDOCK_CERT_PASSWORD` to its password. Signing needs the
Windows SDK signing tools and a code signing certificate.

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

## Continuous integration

Two GitHub Actions workflows build the project on `windows-latest`:

| Workflow | When | What it does |
| --- | --- | --- |
| `CI` | pull request, push to another branch | build, test, package, upload the zip |
| `Release` | push to `main`, or manual | bump the version, build, tag, publish |

Both use the MSVC toolchain from the runner image. The desktop window needs it,
and the daemon builds with it too.

## Versioning

The version lives in one place per file, and `scripts\Set-Version.ps1` writes
it into all of them. Run the check before a commit:

```powershell
.\scripts\Set-Version.ps1 -Check
```

The `Release` workflow reads the commits after the last `vX.Y.Z` tag and picks
the bump:

| Commit message | Bump |
| --- | --- |
| `feat: ...` | minor |
| `fix: ...`, `perf: ...`, `revert: ...` | patch |
| any type with `!`, or a `BREAKING CHANGE:` footer | major |
| `docs: ...`, `chore: ...`, `ci: ...`, `test: ...`, `style: ...`, `build: ...` | none |
| any other message | patch |

A message can force a bump with `[release:major]`, `[release:minor]` or
`[release:patch]`. `[skip release]` in the newest commit stops a release for
that push only. A manual run can also pick `patch`, `minor` or `major` in the
dialog.

A release writes the new version into every file, commits it as
`chore(release): vX.Y.Z`, tags it, builds the zip and the installer, and
publishes both on the release page. The bump commit does not start another
release.

The workflow pushes the bump commit to `main`, so branch protection must allow
the GitHub Actions bot to push. Give the bot a bypass rule when `main` is
protected.

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
streamdock.exe profile <list|init|demo|show|validate|export|import> [--name N]
streamdock.exe profile export --name Demo --out Demo.zip
streamdock.exe profile import --file Demo.zip --name Copy
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
  writes a two page profile for a daemon test. `profile tools` writes the four
  page starter profile. `profile export` writes one zip with the profile, its
  assets and its icons. `profile import` reads it back, under a new name if
  you ask.
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

The renderer bundles the Roboto font under the Apache 2.0 licence. The licence
text is in `crates/render/assets/Roboto-OFL.txt`. Keep it with a release.

The vendor application was never installed or executed.
The installer payload is not decompressed.
See `installer-analysis.md` for the static analysis, and
`handoff/HANDOFF.md` for the full session record.

## Safety

Read commands are harmless.
Write commands change device state.
Power cycle the device if it stops to respond.
