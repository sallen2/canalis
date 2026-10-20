# How Canalis works

Notes on the TRYX Panorama and TURRIS 620 screens and on KANALI, for anyone working on Canalis.

## Files

| | Linux | Windows |
|---|---|---|
| Settings | `~/.config/canalis/settings.json` | `%APPDATA%\canalis\settings.json` |
| Live status (service → window) | `$XDG_RUNTIME_DIR/canalis/status.json` | `%LOCALAPPDATA%\canalis\status.json` |
| Service log | `journalctl --user -u canalis -f` | `%LOCALAPPDATA%\canalis\service.log` |
| Copies of your media, for the preview | `~/.config/canalis/media/` | `%APPDATA%\canalis\media\` |

## The screen

This was worked out from KANALI 1.2.1 with REA, plus the screen's own serial log, which records every message with its raw bytes.

The screen is a small Android 11 device. It connects over USB as `18d1:2d03`, which gives two connections:

| Connection | Device | Used for |
|---|---|---|
| CDC serial | `/dev/ttyACM*` (`/dev/tryx-panorama`) on Linux, `COM*` on Windows | settings and live stats |
| ADB | `adb` | copying media to `/sdcard/pcMedia/`, and reading the theme videos in `/system/media/video/` |

Each serial frame looks like this:

```
0x5A | len_hi len_lo | payload | checksum | 0x5A
```

- `len`: size of the whole unescaped frame, including both `0x5A` bytes.
- `checksum`: sum of the length and payload bytes, modulo 256.
- Escaping: `0x5A` becomes `5B 01`, and `0x5B` becomes `5B 02`.

The payload is a text message like an HTTP request: `POST config 1\r\nSeqNumber=2\r\nDate=…\r\nContentType=json\r\nContentLength=606\r\n\r\n{…}`. The screen answers each message with `1 200` and `AckNumber=SeqNumber+1`.

Built-in themes: the PANORAMA has 14, and choosing one sends `{"id": "Pre-set 12: Cyber Bunker", "settings": {"color", "align", "filter": {"value": "Rain" or "Smoke" or null, "opacity"}, "badges": ["CPU Badge", "GPU Badge"]}, "sysinfoDisplay": [...]}` (captured from KANALI's log). The custom theme uses the same `settings` object. A badge is only sent while a stat for that processor is shown, as in KANALI. "Mirror Mode" is not a separate setting: it sets the rotation to 90° (on) or 270° (off). The carousel pictures come from the screen itself: it keeps a video of each theme in `/system/media/video/`, named after the theme, and ffmpeg takes a still out of a copy of one (Canalis doesn't ship TRYX's artwork). Failing that they come from an installed KANALI's `app.asar`, or from live view screenshots (`adb exec-out screencap -p`). The card in the centre plays the theme's own video, decoded by ffmpeg a frame at a time; only the themes actually watched are kept locally, since all fourteen together run to a few hundred MB.

Split screen ("Screen Splitting") shows two halves side by side, each with its own file and stats. The screen's own app (HomeUI, read with JADX) takes `{"id": "Customization", "screenMode": "Screen Splitting", "media": [left, right], "settings": [left, right], "sysinfoDisplay": [[…], […]]}`: one entry per half, by position, and it ignores `playMode` and `ratio` in this mode. Each half lists its stats top-down in the order chosen, and a right-aligned half puts its badges in the top-right corner.

Custom media is copied to `/sdcard/pcMedia/` over adb, and the screen stacks the live stats from the bottom up (the first one chosen is drawn lowest). KANALI previews media from its own copy (`%APPDATA%\kanali\panorama\media`, with a PNG thumbnail for videos), and crops each file to 2:1, or to 1:1 for its split-screen mode, before sending it (with `MediaStation.dll`: OpenCV for pictures, ffmpeg for videos). Canalis keeps its own copies the same way, and pulls a file back from the screen the first time it needs one for the preview. It crops too: picked files open a crop window in the shape they will be shown in, the panel's exact 2240×1080 or 1120×1080 (`canalis custom --crop` and `canalis split --crop` crop evenly from both sides). Images and GIFs are cropped by Canalis itself; videos need ffmpeg, and without it they are copied as they are.

The startup sequence copies KANALI: `POST conn`, then `POST config` (all settings as one JSON body), then `POST waterBlockScreen`. After that the service sends `STATE all` with a hardware snapshot every second, and re-sends `config` whenever the settings file changes. The tests in `src/protocol.rs` check the encoder against real frames captured from the screen.

KANALI reads its stats through the HWiNFO SDK: `lib\BYMonitor64.dll` is HWiNFO's own client library (8.22, signed by REALiX), loaded by a small native addon. The stats keep HWiNFO's sensor names and units, e.g. `Current DL rate` in KB/s and drive `Read Rate` in MB/s. That SDK is licensed to TRYX and Windows-only, so Canalis reads the same values itself, in the same units:
- **Linux:** `/proc`, `/sys/class/hwmon`, `nvidia-smi` and amdgpu.
- **Windows:** the OS for load, clocks, memory, disk and network (via `sysinfo`), `nvidia-smi` for the GPU, and LibreHardwareMonitor for temperatures, fans and CPU power.

| Source file | What it does |
|---|---|
| `protocol.rs` | framing, escaping, checksum, message format |
| `link.rs` | request/response over the serial port; finds the device |
| `device_config.rs` | settings → the `POST config` body |
| `sensors/` | hardware readings for Linux and Windows |
| `service.rs` | the background loop |
| `service_control/` | start/stop and start at login (systemd, or the Windows Run key) |
| `tray/` | the service's tray icon (Windows notification area, or StatusNotifierItem on Linux) |
| `ui/` | the window |
| `turris/` | the TURRIS 620's screen (Linux): framing, messages, the stats overlay, and its USB printer device |

## The TURRIS 620's screen

The TURRIS 620 air cooler's screen (USB `391a:2011`, 5" 1280×720, product name "Turris") works differently from the Panorama's. It is a small Linux device (kernel 5.10, screen app v1.0.2) that connects as a bidirectional USB printer: the kernel's usblp driver makes it `/dev/usb/lp*`, and the udev rule adds `/dev/tryx-turris` and gives the desktop user access. Canalis reads and writes that device, so no adb, serial port or USB library is involved; it is supported on Linux only. The protocol follows the notes in [DXVSI/Tryx-Linux-GUI](https://github.com/DXVSI/Tryx-Linux-GUI) (MIT licence), checked against a TURRIS 620.

Each message is `TRYX`, the payload's size (u32, little-endian), and the payload: a protobuf request or response with a header (a version, and a track id that the answer repeats) and one field whose number says what the message is:

| Field | Message |
|---|---|
| 10 | ping, answered with a pong; Canalis pings every 2 s |
| 100, 103, 104 | queries for the device information, the stored media and the user configuration |
| 200 | user configuration: the media it plays, backlight, brightness and mirror mode (not answered) |
| 201 | overlay layout: groups of labels with their place, alignment, font, size and colour (not answered) |
| 300 | stat values: new texts for the layout's labels (not answered) |
| 402 | end of a file transfer; sent with no file, it restarts playback |

- **Themes:** the screen stores five, under `preset/`: Cosmos, Erebos, Festival, MechanicalArm and Meteorite. Choosing one writes the user configuration, then the layout, then the end of a file transfer with no file, which restarts playback, as DXVSI's `restartTurrisPlayback` does. Canalis edits a copy of the configuration the screen reports and writes all of it back, so what it doesn't use (the power-on and standby media, the filter) is kept, and it only writes when the theme or a display setting changed.
- **Stats:** the layout is sent once, then only the changing texts, every second. It follows KANALI's Turris layout as DXVSI recorded it, with KANALI's group and label ids: a row per stat, its title above the value and unit, rows spread evenly down the screen, and the CPU and GPU badges along the top. Canalis's three stats that KANALI doesn't offer on the Turris (CPU and GPU voltage, motherboard temperature) continue the numbering. The text is larger than KANALI's 15 for titles and 120 for values, which makes titles about 1.3 mm tall at the screen's 294 PPI: one or two rows use 42 / 180 / 50 (title / value / unit) with badges of 36, the size picked by comparing several on the screen, and three rows use 32 / 136 / 38 with badges of 30 so they don't overlap.
- "Black screen while the PC sleeps" and the fan settings belong to the Panorama, so the window hides them while a Turris is connected.

`src/turris/messages.rs` tests the decoders against answers captured from the screen.

## Tested

On the real screen (firmware V1.0.11):
- **Windows:** installed with `install-windows.ps1` in place of KANALI; the installer sets up adb, ffmpeg and LibreHardwareMonitor (with PawnIO) on its own; the service starts hidden with its tray icon, connects on COM3, applies settings and streams live CPU, memory, disk, network and GPU stats plus CPU, motherboard and NVMe temperatures, CPU power and fan speeds; start at login is registered; command-line output works.
- **Linux (Omarchy):** the service runs from systemd `--user`, connects on `/dev/ttyACM0`, applies settings and keeps the screen fed from the hwmon sensor readers. The carousel fills in from the screen's own theme videos, and its centre card plays one. Only one program can hold the serial port, so another controller for the same screen has to be stopped first.
- Brightness changes made from the command line reach the screen within a second, and invalid values are refused and not saved.
- The preview pulls the GIF already on the screen over adb and plays it, with the stats in the same order and layout as the screen. Live view shows real screenshots of the screen, even with another (offline) adb device attached. The carousel takes all 14 theme pictures off the screen, and from KANALI 1.2.1 when one is installed; the card in the centre plays that theme's video. The built-in-theme message matches the one KANALI sent for Pre-set 12 with Rain, a CPU badge and Date & Time, which the screen accepted.

On a TURRIS 620 (screen app v1.0.2) on Fedora 44 (KDE Plasma 6, Wayland): the service finds the screen on `/dev/usb/lp0`, and the window shows it as connected and lists its five themes. Switching between the five themes, brightness, screen off and on, and mirror mode all reach the screen. Live stats show in one, two or three rows, left- or right-aligned, with the CPU and GPU badges, in °C or °F, including the three stats KANALI doesn't offer on the Turris. Settings changed in the window or from the command line apply within a second, and the service uses about 0.5% of one CPU core and 12 MB of memory. Centre alignment hasn't been checked on the screen.

`cargo test` runs the unit tests on Windows, and the code type-checks for `x86_64-unknown-linux-gnu`.

Not tested yet: `install.sh` end to end, and the Waybar tray icon.

## Known gaps

- **"turboPump" warning:** the firmware logs `config JSONException--No value for turboPump`. KANALI's own `false` value causes a similar error. Settings still apply.
- **Stats that need special access:** CPU voltage and memory speed are sent as 0. On Linux, CPU power is sent as 0 unless RAPL is readable; on Windows, temperatures, fans and CPU power need LibreHardwareMonitor.
- **Windows command line:** `canalis.exe` is a windowed program, so `cmd` doesn't wait for it and its exit code isn't reported. Output still appears.
- **TURRIS 620:** your own images, GIFs and videos can't be sent to it yet (its file transfer isn't implemented, and the Customization tab says so), and it works on Linux only.
- **Not implemented:** firmware updates, the KANALI cloud connection, "turbo pump" mode, and these KANALI PANORAMA features: screen recording to make new media, the Low/Mid/High/Full fan speed profiles (Canalis offers their curves as starting points instead), and playing a theme's video full screen on hover (the carousel's centre card plays it in place instead).
