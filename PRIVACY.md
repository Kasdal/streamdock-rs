# Privacy

Stream Dock runs on your computer. It has no account and no cloud service.

## What the program stores

All data stays in `%LOCALAPPDATA%\StreamDock`:

| Path | Content |
| --- | --- |
| `profiles\*.json` | Your profiles: keys, labels, images, actions |
| `profiles\assets` | Images that you dropped on a key |
| `profiles\icons` | Icons that you added |
| `profiles\plugins` | Plugins that you installed |
| `logs\daemon.log` | A local log, rotated at 1 MB |
| `runtime.json` | The editor port and token of the running daemon |

Delete that folder to remove everything.

## Keystrokes

A key can send keystrokes, text, media keys and mouse-free input to the
foreground application. You define these actions. The program sends nothing
until you press a key that you configured.

## Network

The program sends no telemetry.

Network traffic happens only in these cases:

1. The editor listens on `127.0.0.1` with a random port and a token. It does
   not accept a connection from another machine.
2. An `http` action sends the request that you wrote in the profile.
3. A `plugin` action runs your plugin. The plugin decides what it does.
4. If you run `streamdock update check`, it fetches one manifest file and its
   signature. It sends no identifier.
5. A weather widget on the side strip asks Open-Meteo for the weather of the
   place you picked. See below.

## Weather widget

A weather widget is the only automatic network use. It is off unless you add
one to the side strip.

| Question | Answer |
| --- | --- |
| Who gets the request | `api.open-meteo.com` and `geocoding-api.open-meteo.com` |
| What is sent | the coordinates of the place, nothing else |
| What comes back | the temperature and a weather code |
| How often | every 15 minutes while a weather widget exists |
| Account or key | none |
| Is it stored | no, the answer stays in memory |

The place lookup runs only when you press Look up in the editor. Remove the
weather widget and the traffic stops.

## Update check

The update check is manual. It reads a manifest, checks the signature, and
compares the version. It does not install anything.

## Contact

This is an open project. Report a privacy problem in the project issue
tracker.
