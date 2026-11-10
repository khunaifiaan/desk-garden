# Desk Garden v0.1 — measurements

## Machine
| | |
|---|---|
| CPU | Intel Core i7-8665U @ 1.90 GHz (4 cores / 8 threads) |
| GPU | Intel UHD Graphics 620 (integrated) |
| RAM | 16 GB |
| Windows | Windows 11 Pro 25H2 (10.0.26200). Desktop layout detected: 24H2 (`Raised24H2`) |
| Resolution | 1920 × 1080 at 125% scaling, primary monitor, taskbar auto-hide |
| Power | On AC power, battery low (3%) during the later runs |

## Method
`scripts/measure.ps1` samples `DeskGarden.exe` once per second for 20 s (12 s for the paused runs):
- **Memory** = `\Process\Working Set - Private`, the number in Task Manager's "Memory" column.
- **CPU** = `\Process\% Processor Time` ÷ logical cores, as in Task Manager's CPU column. The later runs also report process CPU time ÷ wall time ÷ cores as a cross-check; the two agree.
- **GPU** = sum of `\GPU Engine(pid_<id>_*)\Utilization Percentage`, the same source as Task Manager's GPU column.
- **First frame** = process start to the first `Present`, from `%APPDATA%\KHYR\DeskGarden\deskgarden.log`.

Steps 3–7 were measured with the debug switch `DG_UPTO=<step>`, which turns on features cumulatively, using the final code. During "desktop visible" runs all windows were minimized. During "rake" runs a script moved the cursor in a figure-eight over the gravel.

## Targets vs. result (full app, step 8)
| Metric | Target | Measured | |
|---|---|---|---|
| Private working set | ≤ 40 MB (stretch ≤ 25 MB) | **27.2–27.5 MB** | Met. Stretch missed by about 2.5 MB; see the plan below. |
| CPU while animating at 24 fps | ≤ 2% | **0.08–0.30%** (max 1.2%) | Met |
| CPU while paused (fullscreen app) | ~0% | **0.008%** | Met |
| GPU | low | **~4–7%** idle sway (8 fps), **11–16%** rain + raking (24 fps), **~1.3%** paused | See notes |
| Startup to first frame | < 1 s | **0.20–0.36 s** | Met |
| Quit (tray or `--quit`) to process gone | — | **0.11 s** | |

## Results by step
| Step | What's on | Conditions | Memory | CPU avg (max) | GPU avg | First frame |
|---|---|---|---|---|---|---|
| 1 | Base layer behind the icons, 24 fps | other windows open | 13.1 MB | 0.02% (0.19) | 4.1% | 168 ms |
| 2a | + fern/canopy sway with soft shadows, leaves, grade, vignette | shaders compiled at runtime | 56.5 MB | 0.02% (0.19) | 9.9% | 894 ms |
| 2b | same | shaders precompiled in `build.rs`, CPU-built mip chains | 40.1 MB | 0.58% (1.55) | 9.8% | 370 ms |
| 2c | same | overlay textures cropped to their content, rake mask at half size | **27.1 MB** | 0.02% (0.39) | 10.4% | 364 ms |
| 3 | + local-clock time of day (09:40, morning) | desktop visible | 27.3 MB | 0.00% (0.00) | 3.9% | 240 ms |
| 3 | Night mood, fireflies | desktop visible, 8 s | 27.1 MB | 0.00% (0.00) | 5.3% | 226 ms |
| 4 | + weather: Rain (grade, 420 streaks, drop rings) | desktop visible | 27.3 MB | 0.00% (0.00) | 11.1% | 196 ms |
| 5 | + cursor rake ripples (30 Hz polling) | desktop visible, raking | 27.3 MB | 0.15% (0.97) | 16.2% | 202 ms |
| 6 | + water basin (drain, spout rings, Ctrl+Alt+W) | desktop visible, raking | 27.3 MB | 0.13% (0.58) | 16.3% | 195 ms |
| 7 | + pause rules, cursor resting (drops to 8 fps) | desktop visible | 26.9 MB | 0.05% (0.58) | 4.6% | 449 ms |
| 7 | Paused: maximized window over the desktop | 12 s | 27.2 MB | 0.02% (0.19); CPU time 0.008% | 1.5% | — |
| 7 | Paused: borderless fullscreen window | 12 s | 27.2 MB | 0.02% (0.19); CPU time 0.008% | 1.3% | — |
| 8 | Full tray menu, Evening + Rain, raking | desktop visible | 27.2 MB | 0.08% (0.39) | 16.3% | 226 ms |
| 8 | Final build, cursor resting (8 fps) | desktop visible | 27.4 MB | 0.09% (0.39); CPU time 0.079% | 6.8% | 263 ms |
| 8 | Final build, Rain + raking (24 fps) | desktop visible | 27.5 MB | 0.27% (1.16); CPU time 0.296% | 11.5% | 263 ms |

"0.00%" means less than the counter's resolution. The process uses well under a millisecond of CPU per frame.

## What moved the numbers
- **Runtime shader compilation cost ~16 MB and ~0.5 s.** Loading `d3dcompiler_47.dll` and compiling at startup left its heap behind. The HLSL is now compiled by `build.rs` and embedded as bytecode.
- **On this integrated GPU, texture memory counts toward the private working set.** Every MB of texture showed up in Task Manager. Cropping the fern (bottom strip) and canopy (top strip) to their visible pixels and storing the gravel mask at half resolution saved about 13 MB.
- **Swap-chain buffers don't count against the process.** Step 1 drew a 1920×1080 frame with two back buffers at 13 MB total.
- **The DXGI blt model (`DISCARD`) shows nothing** in the 24H2 layered child of `Progman`. The flip model (`FLIP_DISCARD`) works, so both layouts use it.
- **Quitting took ~3.5 s** in the Intel driver's DLL teardown. After the wallpaper is restored the app now ends the process directly: 0.11 s.

## Gaps and plan
- **Stretch target (≤ 25 MB): 27.3 MB today.** The base layer is the largest piece left (1920×1080 RGBA = 8 MB). Options, in order:
  1. Store it as BC1/BC7 (block compression) generated at build time: −6 to −7 MB, with a visual check on the sand rings.
  2. Use a 16-bit format (B5G6R5): −4 MB, with some risk of banding on soft moss gradients.
  3. Drop the overlay mip levels below the ones used for shadows: −1 MB.
- **GPU 11–16% while raining or raking on UHD 620.** Most of that is the full-screen pass at 24 fps: base, two rotated overlays, two blurred shadow lookups and up to 32 ripple rings for every pixel. Plan:
  - Pre-composite the static parts (base + grade) into one texture when the mood changes, instead of every frame.
  - Run ripples only in a scissor rectangle around active rings.
  - Draw rain streaks at half resolution.
  
  Expected result: under 5% while raining. When idle at 8 fps the garden already uses about 4–7%, and paused about 1%.
- **Battery Saver and session lock** follow the same code path as the fullscreen pause, which was verified. They could not be triggered from a script without changing system settings or locking the machine. Please check them by hand: lock with Win+L, or turn on Battery Saver in Settings, then look for `pause:` lines in the log.
