# Desk Garden v0.1

A living zen-garden wallpaper for Windows 10 and 11 by KHYR Labs. The garden draws **behind the desktop icons**, follows the time of day and the local weather, ripples where the cursor crosses the raked gravel, and has a stone basin that slowly empties until you log a drink of water.

It is a single native executable (Rust + Direct3D 11). There is no WebView, no installer, no telemetry, and the only network calls go to Open-Meteo.

## Build and run

You need Rust (stable, MSVC toolchain) and the Visual Studio Build Tools.

```
cargo build --release
target\release\DeskGarden.exe
```

The garden appears behind your desktop icons on the primary monitor, and a round raked-sand icon appears in the notification area (tray). Starting it a second time does nothing, because only one instance runs.

## Tray menu

Right-click (or left-click) the tray icon:

| Item | What it does |
|---|---|
| **Time** → Auto / Morning / Midday / Evening / Night | Auto follows your clock. The others hold one mood. Changes fade over a few seconds. |
| **Weather** → Auto / Clear / Cloudy / Rain | Auto uses Open-Meteo for your city, refreshed every 30 minutes. Offline, it keeps the last known weather. With no city set, Auto shows clear. |
| **I drank water** (Ctrl+Alt+W) | Refills the basin with a splash. The basin drains from full to about 35% over 3 hours. |
| **Pause / Resume** | Stops drawing (0 fps, the last frame stays). |
| **Start with Windows** | Adds or removes `DeskGarden` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. |
| **Set city...** | Type a city name. It is looked up once with Open-Meteo's geocoding API. Leave it empty to turn weather lookups off. |
| **Quit** | Removes the garden and restores your normal wallpaper. |

Hovering over the tray icon shows the current weather, the basin level, and why the garden is paused, if it is.

## It pauses by itself when

- an app is fullscreen on the primary monitor (`SHQueryUserNotificationState`, plus a check of the foreground window's bounds)
- the desktop is completely covered by windows
- Battery Saver is on
- the session is locked

While paused it draws nothing and uses about 0% CPU. It also runs at 24 fps while something is moving (rain, ripples, a splash, a mood change) and drops to 8 fps once only the gentle sway is left and the cursor has rested for 10 s.

## Settings and logs

- Settings: `%APPDATA%\KHYR\DeskGarden\config.json` (time and weather mode, city and coordinates, last weather, last drink time, paused)
- Log: `%APPDATA%\KHYR\DeskGarden\deskgarden.log` (desktop layout detected, time to first frame, pauses, weather results)

## Quitting and getting your wallpaper back

- Use **Quit** in the tray menu, or run `DeskGarden.exe --quit`.
- If the app crashes, it repaints the normal wallpaper from its panic and unhandled-exception handlers.
- If Explorer restarts, Desk Garden attaches itself to the new desktop windows.
- **Manual recovery:** if the desktop ever looks wrong (black, or garden stuck), open Task Manager, right-click **Windows Explorer** and choose **Restart**.

## How it sits behind the icons

Sending message `0x052C` to `Progman` makes Explorer split the desktop into an icon layer (`SHELLDLL_DefView`) and a wallpaper host (`WorkerW`). Desk Garden detects where those windows ended up:

- **Classic layout** (Windows 10, Windows 11 before 24H2): the icons live in a top-level `WorkerW`. Desk Garden becomes a child of the next `WorkerW`, which is the wallpaper host.
- **24H2 layout** (Windows 11 24H2 / 25H2): the icons and the wallpaper host are both children of `Progman`. Desk Garden becomes a layered child of `Progman`, placed below `SHELLDLL_DefView` and above the `WorkerW`.

It draws with a flip-model DXGI swap chain in both layouts. A blt-model swap chain shows nothing in the layered 24H2 child.

## Code map

| File | Role |
|---|---|
| `src/main.rs` | Controller window, tray commands, pause rules, cursor polling, frame pacing |
| `src/desktop.rs` | Finding the wallpaper slot, attaching, restoring the wallpaper |
| `src/scene.rs`, `src/garden.hlsl` | GPU resources and shaders (grade, weather, sway, shadows, ripples, basin, particles) |
| `src/garden.rs` | Simulation: time-of-day keys, weather blend, leaves, fireflies, rain, ripples, basin |
| `src/weather.rs` | Open-Meteo over WinHTTP |
| `src/pause.rs` | Fullscreen, covered, Battery Saver checks |
| `src/config.rs` | `config.json` |
| `src/tray.rs`, `src/citydlg.rs` | Tray icon and menu, "Set city..." box |
| `build.rs` | Compiles the HLSL at build time, so no shader compiler is loaded at runtime |
| `scripts/measure.ps1` | Memory, CPU and GPU sampling for `MEASUREMENTS.md` |

## Not in v0.1

Focus tracking for the azalea, the break reminder (wind stops), multi-monitor, a settings window, auto-update and an installer. The code has `TODO(v0.2 ...)` markers where these attach.
