# Desk Garden — v0.1 prototype brief (for Claude Code)

Read this file, then `assets/scene.json`, then open `reference/feature_mockup.html` in a browser and watch `reference/motion_reference.mp4` and `reference/rain_reference.mp4`. The mockup and videos are the approved look and behaviour. `reference/anim_reference.py` is the Python that rendered the videos; use it to understand the math, not as code to ship.

## What Desk Garden is
A living wallpaper for Windows 10 and 11 by KHYR Labs ("Small tools. Steady days."). A top-down Japanese zen garden sits **behind the desktop icons** and quietly follows the person's day:
- the real time of day (morning, midday, orange evening, night with fireflies)
- the real weather (clear, cloudy, rain)
- the cursor rakes the sand when it moves over the empty desktop
- a stone water basin that slowly empties and refills when the person logs a drink
- an azalea that blooms more as the person focuses (later version)
- the wind stops when it is time for a break (later version)

## Why this prototype exists
A competitor (Deskland, a living wallpaper that grows around desktop icons) just went viral with 233K views. The most common question under it was "how heavy is it?" and the answer was "20 MB RAM, works offline". **Desk Garden must feel just as light.** v0.1 exists to prove the wallpaper runs behind the icons with very low RAM and CPU before we build more features.

## v0.1 scope
1. **Behind the icons**: render a borderless window that sits between the wallpaper and the desktop icons, on the primary monitor. Use the WorkerW / Progman technique (send message 0x052C to Progman, then parent the window correctly). Windows 11 24H2 changed the desktop window structure (icons and the wallpaper host moved under Progman), so detect the structure at runtime and handle both the old WorkerW layout and the 24H2 layout. Restore the normal wallpaper cleanly on exit or crash (Explorer restart must not leave a black desktop). Do not copy code from other wallpaper projects; reimplement from the Windows APIs.
2. **Scene**: draw `base`, then the time-of-day grade, then `fern` and `canopy` overlays with a gentle sway (see scene.json), then rain or night extras, then falling leaves. Cover-fit the 1920x1080 design to the monitor.
3. **Time of day**: follow the local clock, interpolating the grade keys in scene.json. Transitions are slow (minutes), never sudden.
4. **Weather**: Auto uses Open-Meteo (free, no API key) for the city saved in config; refresh every 30 minutes; offline keeps the last known weather. Map weather codes to clear, cloudy (overcast grade) and rain (rain grade, streaks, drop rings). Manual override from the tray: Auto / Clear / Cloudy / Rain.
5. **Cursor rake**: poll the cursor position (no hooks) at 30 Hz only while the desktop is visible; where the cursor passes over `bed_mask` and not `object_mask`, draw expanding ripple rings that fade in about 1.6 s.
6. **Water basin**: the water level drains from full to about 35% over 3 hours; the tray item "I drank water" (and the hotkey Ctrl+Alt+W) refills it with a few splash rings at the spout point. The spout keeps dripping with small rings in the basin.
7. **Pause rules**: stop rendering (0 fps, keep the last frame) when any app is fullscreen on that monitor (use SHQueryUserNotificationState and a foreground-window bounds check), when the desktop is fully covered, when Battery Saver is on, and when the session is locked.
8. **Tray icon and menu**: Time (Auto, Morning, Midday, Evening, Night), Weather (Auto, Clear, Cloudy, Rain), I drank water, Pause/Resume, Start with Windows (on/off), Set city..., Quit. Settings live in `%APPDATA%\KHYR\DeskGarden\config.json`.
9. **Frame budget**: 24 fps cap when animating, drop to 8 fps when nothing but sway is moving and the cursor is still for 10 s.

Not in v0.1: focus tracking for the azalea, break reminder, multi-monitor, settings window UI, auto-update, installer. Leave clear TODO hooks for them.

## Tech choice (decide with measurements)
- Preferred: **Rust + wgpu** (or Direct3D 11 / Direct2D through the `windows` crate) drawing textured quads. Do the time-of-day grade, rain grade, sway (UV offset around the pivot) and ripples in shaders, so the CPU stays near idle. Keep the decoded layer textures on the GPU and drop CPU copies after upload.
- Avoid a WebView for the wallpaper itself: a WebView2 process alone usually costs far more RAM than our target. A WebView is acceptable later for a settings window that is only open on demand.
- Tray: the `tray-icon` crate or plain Win32 Shell_NotifyIcon.

## Targets (measure on the owner's laptop and report)
| Metric | Target |
|---|---|
| Private working set (Task Manager "Memory") | **≤ 40 MB** (stretch: ≤ 25 MB) |
| CPU while animating at 24 fps | ≤ 2% on a typical laptop |
| CPU while paused (fullscreen app) | ~0% |
| GPU | low; report the GPU % from Task Manager |
| Startup to first frame | < 1 s |
Report the measured numbers in `MEASUREMENTS.md` with the machine specs (CPU, GPU, RAM, Windows version, resolution).

## Look rules
- Calm, never flashy. No UI drawn on the wallpaper itself.
- Ripples, rain and leaves follow the reference videos exactly (sizes, counts, alpha in scene.json).
- No network calls except Open-Meteo. No telemetry.

## Deliverables
- Source in this repo, `cargo build --release` produces `DeskGarden.exe`.
- `README.md` with how to run, the tray menu, where the config lives, and how to quit and restore the normal wallpaper.
- `MEASUREMENTS.md` with the numbers above.
- A 15–20 s screen recording of the desktop with icons on top of the garden (morning, then a manual switch to evening and rain, cursor raking the sand, "I drank water" refilling the basin). This is the next Threads post.

## Done when
- The garden draws behind the icons on Windows 11 (and 10 if available), survives an Explorer restart, and restores the normal wallpaper on quit.
- Time, weather (auto + manual), cursor ripples and the water basin work as described.
- Fullscreen, lock and Battery Saver pause it.
- Memory and CPU targets are met or the gap is explained with a plan.
