# Stream Dock

<img width="1383" height="926" alt="Image" src="https://github.com/user-attachments/assets/e3d5af0f-6ae9-43ac-a65e-224ede3ea96e" />

Stream Dock controls the AJAZZ AKP-153E keypad on Windows. It uses the Win32
HID API. It needs no vendor driver and no vendor software.

The keypad has 15 keys and one side display. Each key shows a 96x96 pixel
image. The side display is 96x460 pixels and shows three widgets.

## Features

| Feature | Detail |
| --- | --- |
| 15 keys | JPEG, PNG, GIF, SVG, icon and text tiles |
| Side display | system meter, clock and weather widgets |
| Profiles | JSON files with pages, themes, images and actions |
| Actions | hotkey, text, media, launch, multi, page, brightness, command, HTTP and plugin |
| Per-app profiles | a process name switches the profile |
| Editor | native window and browser page |
| Tray | show editor, reload profile, quit |
| Autostart | per-user Run key, no administrator rights |
| Portability | export and import a profile as one zip |

## Requirements

| Item | Detail |
| --- | --- |
| System | Windows 10 or Windows 11, 64-bit |
| Device | AJAZZ AKP-153E, VID `0x0300`, PID `0x3010` |
| Runtime | WebView2 for the desktop window |
| Rights | none, the program runs as the current user |

WebView2 comes with Windows 11 and with most Windows 10 updates. The browser
editor needs no WebView2.

## Install

### Installer

1. Download `StreamDock-<version>-setup.exe` from the Releases page.
2. Run the file.
3. Open **Stream Dock** from the Start menu.

The installer is per user. It needs no administrator rights. It writes the
program to `%LOCALAPPDATA%\StreamDock\bin`, adds a Start menu shortcut, adds an
entry in **Apps and features**, and sets the per-user autostart value.

To remove the program, use **Apps and features**. The uninstaller keeps your
profiles.

### Portable package

1. Download `StreamDock-<version>.zip`.
2. Unpack the file.
3. Run `install.ps1` in PowerShell.

`install.ps1` copies the program to `%LOCALAPPDATA%\StreamDock\bin`. These
options change the result:

| Option | Effect |
| --- | --- |
| `-Target PATH` | install into another folder |
| `-NoAutostart` | do not add the Run key value |
| `-NoShortcut` | do not create the Start menu shortcut |

`uninstall.ps1` removes the program. Add `-KeepProfiles` to keep the profiles
and the settings.

### First start

Open **Stream Dock** from the Start menu. The window starts the daemon if it is
not running. A new install gets a `Default` profile with four pages: Media,
System, Apps and Tools.

## Use

### The window

The window shows the device as it looks. Click a key to edit it. Press
**Apply key** to send the change to the panel.

| Task | How |
| --- | --- |
| Set an image | Drag a JPEG, PNG, GIF or SVG file onto a key |
| Set a label | Type the text in the key editor |
| Set an action | Pick a type and fill the fields |
| Change the theme | Use the two colour pickers in the header |
| Change brightness | Use the slider in the header |
| Undo | Press **Undo** or Ctrl+Z |
| Export | Press **Export** and pick a file |
| Import | Press **Import** and pick a zip |

The close button hides the window. The program keeps running in the tray.

### The side display

Click the strip in the device picture. The STRIP panel shows three slots. Pick
a widget for each slot and press **Apply strip**.

| Widget | Shows |
| --- | --- |
| System meter | CPU, GPU, memory or disk, with a name, a percentage and a bar |
| Clock | the time, the date, or both |
| Weather | a place, the temperature and a condition |
| Empty | nothing |

The **Text size** slider sets the scale from 60 % to 160 %. The profile stores
the value.

A weather widget sends the coordinates of the place to Open-Meteo every
15 minutes. Remove the widget to stop the traffic. See `PRIVACY.md`.

### The tray icon

| Item | Effect |
| --- | --- |
| Show editor | Opens the editor |
| Reload profile | Reads the profile file again and repaints |
| Quit | Stops the daemon and clears the keys |

### The browser editor

`streamdock ui` opens the editor in the default browser. The daemon must run.
The server listens on `127.0.0.1` only and needs the token from
`%LOCALAPPDATA%\StreamDock\runtime.json`.

## Profiles

A profile is one JSON file in `%LOCALAPPDATA%\StreamDock\profiles`. A page
holds keys by number.

```json
{
  "version": 1,
  "name": "Default",
  "pages": [
    {
      "name": "Main",
      "keys": {
        "1": {
          "label": "Mute",
          "actions": [ { "type": "hotkey", "keys": ["ctrl", "shift", "m"] } ]
        }
      }
    }
  ]
}
```

Action types: `hotkey`, `text`, `media`, `launch`, `multi`, `page`,
`brightness`, `command`, `http` and `plugin`.

An `http` action needs an `http://` URL. HTTPS is not supported yet.

A `command` action runs a program without a confirmation. Only use profiles
that you trust.

### Export and import

**Export** writes one zip with the profile, its images and its icons. **Import**
replaces the active profile from such a zip. The command line does the same:

```powershell
streamdock profile export --name Default --out Default.zip
streamdock profile import --file Default.zip --name Copy
```

## Command line

`streamdock.exe` talks to the panel without the daemon.

| Command | Effect |
| --- | --- |
| `list` | show the HID interfaces |
| `desc` | show the report sizes and usages |
| `watch` | print key events |
| `image --key 1 --file photo.jpg` | send one image to one key |
| `brightness --value 60` | set the panel brightness |
| `clear` | clear every key |
| `profile tools` | write the four-page starter profile |
| `preview --out sheet.jpg` | write a picture of one page |
| `ui` | open the editor of the running daemon |
| `autostart install` | start the daemon at logon |
| `update check --url URL` | check a signed update manifest |

Run `streamdock.exe` with no command to see the full list.

## Where the data lives

All data stays in `%LOCALAPPDATA%\StreamDock`:

| Path | Content |
| --- | --- |
| `profiles\*.json` | your profiles |
| `profiles\assets` | images that you dropped on a key |
| `profiles\icons` | icons that you added |
| `profiles\plugins` | plugins that you installed |
| `logs\daemon.log` | the local log, rotated at 1 MB |
| `runtime.json` | the editor port and token of the running daemon |

Delete that folder to remove everything.

## Build from source

You need Rust stable and the Visual Studio Build Tools with the C++ workload.
The desktop window uses Tauri 2 and needs the MSVC toolchain.

```powershell
cargo build --release
cargo test --release
```

The tests need no device.

Build the window, the portable package and the installer:

```powershell
.\build-desktop.ps1
.\build-release.ps1
.\build-nsis.ps1
```

| Script | Output |
| --- | --- |
| `build-desktop.ps1` | `crates\desktop\target\x86_64-pc-windows-msvc\release\streamdock-desktop.exe` |
| `build-release.ps1` | `dist\StreamDock-0.3.2.zip` |
| `build-nsis.ps1` | `dist\StreamDock-0.3.2-setup.exe` |

`build-nsis.ps1` needs `makensis`. Install NSIS, or pass `-MakensisPath PATH`.

## Project layout

| Path | Content |
| --- | --- |
| `crates/core` | HID protocol, device handle, image pipeline, reconnect |
| `crates/store` | profile model and JSON store |
| `crates/render` | key tile renderer |
| `crates/actions` | hotkey, text, media, launch and other actions |
| `crates/ipc` | loopback HTTP server, token, editor page |
| `crates/daemon` | the `streamdockd` binary |
| `crates/cli` | the `streamdock` binary |
| `crates/desktop` | the Tauri window, built outside the workspace |
| `scripts` | version scripts for the release workflow |
| `.github/workflows` | CI and release pipelines |

## Continuous integration

| Workflow | When | What it does |
| --- | --- | --- |
| CI | pull request, push to another branch | build, test, package |
| Release | push to `main`, or manual | bump the version, build, tag, publish |

The release workflow reads the commit messages after the last tag and picks a
semantic version bump. A `feat:` commit bumps the minor number, a `fix:`
commit bumps the patch number, and a `!` or a `BREAKING CHANGE:` footer bumps
the major number. The workflow writes the new version into every file, builds
the zip and the setup, and publishes both.

Run the version check before you push:

```powershell
.\scripts\Set-Version.ps1 -Check
```

## Limits

| Limit | Detail |
| --- | --- |
| Keys | 15. The side display has no buttons |
| Network actions | `http://` only, no HTTPS |
| Temperature | no sensor, so no temperature widget |
| System | Windows only |

## Safety

Read commands are harmless. Write commands change the device state. Power cycle
the device if it stops to respond.

`streamdock send` writes one raw report. It refuses to run without
`--i-understand`.

## Privacy

Stream Dock sends no telemetry. The weather widget is the only automatic
network use. See `PRIVACY.md`.

## Licence

MIT OR Apache-2.0.

The renderer bundles the Roboto font under the Apache 2.0 licence. The licence
text is in `crates/render/assets/Roboto-OFL.txt`.
