# streamdock-rs

Minimal Rust controller for the AJAZZ AKP-153E keypad.
The crate talks to the device through the Win32 HID API.
It needs no C compiler, no vendor driver, and no vendor software.

## Status

| Part | State |
| --- | --- |
| Device enumeration | verified on hardware |
| Interface capabilities | verified on hardware |
| Command writes | verified on hardware |
| Key events (read) | verified on hardware |
| Image upload | verified on hardware |
| Image rotation | measured on hardware: 270 degrees |

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

## Key layout

Keys 1 to 15 fill the grid row by row, left to right.
Keys 16 to 18 are the strip keys. The key IDs come from the Companion
driver and match the reports of this unit.

| Key | ID | Size |
| --- | --- | --- |
| 1 to 15 | `0x0D 0x0A 0x07 0x04 0x01` / `0x0E 0x0B 0x08 0x05 0x02` / `0x0F 0x0C 0x09 0x06 0x03` | 96x96 |
| 16 to 18 | `0x10 0x11 0x12` | 80x80 |

## Build

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;<mingw64>\bin;<rustup self-contained>\;$env:Path"
cargo build --release
```

`<mingw64>` is the WinLibs `bin` directory.
`<rustup self-contained>` helps the linker find the matching runtime.

## Commands

```powershell
streamdock.exe list
streamdock.exe desc
streamdock.exe init
streamdock.exe brightness --value 60
streamdock.exe image --key 1 --file test-key1.jpg
streamdock.exe capture --seconds 30
streamdock.exe send --hex 4352540000444953 --i-understand
```

* `list` shows every HID interface of the vendor ID.
* `desc` shows the report IDs, sizes and usages of each interface.
* `init` sends wake, heartbeat and refresh.
* `brightness` sets panel brightness from 0 to 100.
* `image` sends one JPEG to one key. Keys 1 to 15 take 96x96 pixels,
  keys 16 to 18 take 80x80 pixels. The command rotates the image by 270
  degrees, compresses it below 10 KB and uploads it. Use `--rotate 0` to
  stop the rotation, or `--rotate 90` and `--rotate 180` to turn it further.
* `capture` dumps input reports and decodes key events. It sends the
  heartbeat first, because the panel sends no key reports until the host
  starts a session. It repeats the heartbeat every 8 seconds.
* `send` writes one raw payload. It refuses to write without `--i-understand`.

## Limits

The key table in `src/model.rs` comes from the Companion driver. The IDs of
keys 1 to 15 match the reports that this unit sent. Keys 16 to 18 are not
pressed during the test.

The vendor application was never installed or executed.
The installer payload is not decompressed.
See `installer-analysis.md` for the static analysis, and `handoff/HANDOFF.md`
for the full session record.

## Safety

Read commands are harmless.
Write commands change device state.
Power cycle the device if it stops to respond.
