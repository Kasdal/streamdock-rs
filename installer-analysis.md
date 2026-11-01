# Analysis: Stream-Dock-AJAZZ-Installer_Windows_global2.0.exe

Date: 2026-10-31
Analyst: REA static inspection (no execution)

## Verdict

This file is a legitimate, digitally signed installer from the Stream Dock / MiraBox / AJAZZ vendor.
I found no malware indicators in the inspected sections.

## Identity

| Field | Value |
|---|---|
| SHA-256 | D6FB46511E17D5FCB9A3A4553C5F0DF251FD1E3D2B30FDAE49EBECFE479F7B9A |
| Size | 653,635,048 bytes (623 MiB) |
| Type | PE32 (x86) executable + overlay |
| ProductName | Stream Dock AJAZZ Global |
| FileVersion | 3.10.202.0711 |
| CompanyName | HotSpot |
| ProductCode (en-US) | {4C83B5CA-0D61-4E8C-BF4A-A41E79F0570A} |
| Install path | [AppDataFolder]\HotSpot\Stream Dock AJAZZ Global 3.10.202.0711\install |

## Signature

- Status: **Valid**. The Authenticode hash covers the whole 653 MB file. No third party can change the payload without breaking the signature.
- Signer: `CN="Shenzhen An Rui Xin Technology Co., Ltd."`, Certum Extended Validation Code Signing 2021 CA.
- Valid: 2025-11-14 to 2026-11-14. Timestamped by GlobalSign R45 TSA.
- `Shenzhen An Rui Xin Technology` is the manufacturer of MiraBox and Stream Dock hardware.
  The company also supplies the AJAZZ NACODEX devices. Source: MiraBox and Alibaba vendor pages.

## Structure (how it works)

1. **Stub, offset 0x0 to 0x1A9400.** An Advanced Installer (Caphyon) bootstrapper, x86, 5 sections.
   Strings confirm `@Advanced Installer`, `Caphyon.AI.ExtUI`, `BootstrapperProgressImpl`.
2. **Resource segments, offset 0x1A9400 to about 0x4B0800.** 30 small resource-only PE images
   (RTF and image theme resources of the setup wizard).
3. **Package metadata, offset 0x4B0800 to about 0x7D0000.** Plain-text setup data:
   MSI tables, Directory and File keys, ProductCode per language, wizard text.
4. **Compressed payload, offset 0x7D0000 to 0x26F549D8.** High entropy (8.0 bits/byte)
   LZMA-style compressed file data. This is the main application.
5. **Authenticode certificate table, offset 0x26F549D8 to end.**

At run time the bootstrapper does this sequence:

1. Shows the localized wizard (28 UI languages).
2. Extracts the MSI package to `%APPDATA%\HotSpot\Stream Dock AJAZZ Global <ver>\install`.
3. Calls `msiexec.exe` for that MSI.
4. MSI custom actions `AI_ExtractFiles` and `AI_ExtractLzmaRM` unpack the application files.
5. The MSI writes shortcuts, an uninstall entry, and device driver files.

## Application contents (from the embedded file catalog)

The application is a Qt 5 desktop program with an embedded Chromium view.

- `StreamDockAJAZZ.exe` main program, `SplashScreen.exe`, `Watcher.exe` (update helper)
- `UpDateToolV3.exe`, `FirmwareUpgradeTool.exe`, `upgcmdHid.exe`, `ScreenCaptureTool.exe`
- `CefViewWing.exe`, `QCefView.dll`, `libcef.dll` (CEF 109), `chrome_elf.dll`, `v8_context_snapshot.bin`
- Qt 5 modules: Core, Gui, Widgets, Network, WebSockets, Multimedia, Svg
- `node20.exe` with JavaScript plugins (`plugin.js`, `manifest.json`, `.sdPlugin`)
- `hidapi.dll`, `libusb-1.0.dll`, `libusbK.sys` (USB device access)
- `7z.dll`, `7za.exe`, `ffmpeg6.dll`, `robotjs.node`, `libnut2.node`
- Plugins: OBS Studio, Photoshop CEP, ETS2/ATS, Discord, Twitch, YouTube, Spotify, vMix
- 17 device profiles (`Info.Board.streamDockProfile`, `OBS.streamDockProfile`, and others)

This file set matches the published product feature list.

## Checks that came back negative

- No `CurrentVersion\Run` key, no scheduled task, no service creation in the package metadata.
- No miner, keylogger, wallet, stealer, webhook, or `powershell -enc` strings.
- No extra executable with an unexpected name.
- No packed or encrypted second stage after the signature was added.

## Limits of this analysis

- I did not decompress the LZMA payload, so I did not inspect the application code byte for byte.
  I read the file catalog, not the file contents.
- I did not run the installer. No dynamic or behavior analysis.
- A signed program can still collect telemetry. The program needs USB HID access and network access.

## Why it looks suspicious, and why that is expected

- 653 MB because CEF, Qt, node and the plugin store are bundled.
- The signer name `Shenzhen An Rui Xin` does not match the product name `AJAZZ`.
- The application starts in the background and has an update helper.
- The device is a macro keypad, so the program reads keystrokes and controls audio.
