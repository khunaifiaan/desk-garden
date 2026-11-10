#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Desk Garden v0.1 — a living zen-garden wallpaper behind the desktop icons.
//!
//! Threads: one UI/render thread (this file) plus short-lived worker threads for
//! Open-Meteo requests. Rendering is Direct3D 11; see scene.rs and garden.hlsl.

mod citydlg;
mod config;
mod desktop;
mod garden;
mod gfx;
mod pause;
mod scene;
mod tray;
mod weather;

use std::cell::RefCell;
use std::io::Write;
use std::time::{Duration, Instant};

use config::{Config, TimeMode, WeatherMode};
use garden::Sky;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_POINTERS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
use windows::Win32::UI::WindowsAndMessaging::*;

// tray commands
const CMD_TIME: u32 = 200; // + TimeMode index
const CMD_WEATHER: u32 = 300; // + WeatherMode index
const CMD_DRINK: u32 = 400;
const CMD_PAUSE: u32 = 401;
const CMD_STARTUP: u32 = 402;
const CMD_CITY: u32 = 403;
const CMD_QUIT: u32 = 499;

const WM_WEATHER: u32 = WM_APP + 2; // lParam: 0 clear, 1 cloudy, 2 rain, -1 failed
const WM_PLACE: u32 = WM_APP + 3; // lParam: Box<Option<weather::Place>>
const TIMER_CHECK: usize = 1; // pause rules + host health, 1 s
const TIMER_WEATHER: usize = 2; // weather refresh, 30 min
const HOTKEY_DRINK: i32 = 1;

const WEATHER_REFRESH_S: u64 = 30 * 60;
const BASIN_DRAIN_S: f32 = 3.0 * 3600.0;

static mut START: Option<Instant> = None;

pub fn log(msg: &str) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("deskgarden.log");
    if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let ms = unsafe { (*std::ptr::addr_of!(START)).map(|s| s.elapsed().as_millis()).unwrap_or(0) };
        let _ = writeln!(f, "[{ms:>8} ms] {msg}");
    }
    #[cfg(debug_assertions)]
    eprintln!("{msg}");
}

pub fn config_dir() -> std::path::PathBuf {
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(|| ".".into());
    base.join("KHYR").join("DeskGarden")
}

/// Debug switch for step-by-step measurements: DG_UPTO=N enables features up to step N
/// (3 time, 4 weather, 5 rake, 6 basin, 7 pause rules, 8 full tray). Default: everything.
fn upto() -> u32 {
    static mut V: u32 = 0;
    unsafe {
        if V == 0 {
            V = std::env::var("DG_UPTO").ok().and_then(|v| v.parse().ok()).unwrap_or(99);
        }
        V
    }
}

struct App {
    ctrl: HWND,
    render: HWND,
    host: Option<desktop::Host>,
    gpu: gfx::Gpu,
    scene: scene::Scene,
    garden: garden::Garden,
    rake_mask: scene::RakeMaskCpu,
    cfg: Config,
    last_frame: Instant,
    monitor: RECT,
    quit: bool,
    first_frame_logged: bool,
    taskbar_created: u32,
    icon: HICON,
    // pause state
    locked: bool,
    pause_reason: Option<pause::Reason>,
    // cursor
    last_cursor: POINT,
    last_cursor_move: Instant,
    // weather
    auto_sky: Option<Sky>,
    fetching: bool,
    place_label: String,
    reattach_failures: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn primary_monitor_rect() -> RECT {
    unsafe {
        let m = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut mi);
        mi.rcMonitor
    }
}

// ---------------- start with Windows (HKCU Run key) ----------------
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

fn startup_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, w!("DeskGarden"), RRF_RT_REG_SZ, None, None, None).is_ok() }
}

fn set_startup(on: bool) {
    unsafe {
        if on {
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
            let v: Vec<u16> = format!("\"{exe}\"").encode_utf16().chain(Some(0)).collect();
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("DeskGarden"), REG_SZ.0, Some(v.as_ptr() as *const _), (v.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("DeskGarden"));
        }
    }
}

// ---------------- weather ----------------
impl App {
    fn effective_sky(&self) -> Sky {
        if upto() < 4 {
            return Sky::Clear;
        }
        match self.cfg.weather_mode {
            WeatherMode::Clear => Sky::Clear,
            WeatherMode::Cloudy => Sky::Cloudy,
            WeatherMode::Rain => Sky::Rain,
            // No city yet / offline: last known weather, else clear.
            WeatherMode::Auto => self.auto_sky.or(self.cfg.last_sky).unwrap_or(Sky::Clear),
        }
    }

    fn target_hour(&self) -> f32 {
        if upto() < 3 {
            return 10.0;
        }
        self.cfg.time_mode.fixed_hour().unwrap_or_else(config::local_hour)
    }

    fn water(&self) -> f32 {
        let elapsed = config::now_unix().saturating_sub(self.cfg.last_drink_unix) as f32;
        (1.0 - elapsed / BASIN_DRAIN_S).clamp(0.0, 1.0)
    }

    fn refresh_weather(&mut self, force: bool) {
        if upto() < 4 || self.fetching || self.cfg.weather_mode != WeatherMode::Auto {
            return;
        }
        let (Some(lat), Some(lon)) = (self.cfg.latitude, self.cfg.longitude) else { return };
        let fresh = config::now_unix().saturating_sub(self.cfg.last_weather_unix) < WEATHER_REFRESH_S;
        if fresh && !force && self.cfg.last_sky.is_some() {
            return;
        }
        self.fetching = true;
        let ctrl = self.ctrl.0 as isize;
        std::thread::spawn(move || {
            let code = match weather::current(lat, lon) {
                Some(Sky::Clear) => 0,
                Some(Sky::Cloudy) => 1,
                Some(Sky::Rain) => 2,
                None => -1,
            };
            unsafe {
                let _ = PostMessageW(Some(HWND(ctrl as *mut _)), WM_WEATHER, WPARAM(0), LPARAM(code));
            }
        });
    }

    fn tooltip(&self) -> String {
        let sky = match self.effective_sky() {
            Sky::Clear => "clear",
            Sky::Cloudy => "cloudy",
            Sky::Rain => "rain",
        };
        let where_ = if self.place_label.is_empty() { String::new() } else { format!(" in {}", self.place_label) };
        let paused = match self.pause_reason {
            Some(r) => format!(" (paused: {r:?})"),
            None => String::new(),
        };
        format!("Desk Garden: {sky}{where_}, basin {}%{paused}", (self.water() * 100.0).round())
    }

    fn update_tip(&self) {
        tray::set_tip(self.ctrl, &self.tooltip());
    }
}

// ---------------- pause rules ----------------
fn check_pause(app: &mut App) {
    let reason = if app.cfg.paused {
        Some(pause::Reason::User)
    } else if upto() < 7 {
        None
    } else if app.locked {
        Some(pause::Reason::Locked)
    } else if pause::battery_saver() {
        Some(pause::Reason::BatterySaver)
    } else if pause::fullscreen(app.monitor) {
        Some(pause::Reason::Fullscreen)
    } else if pause::covered(pause::work_area(), app.render) {
        Some(pause::Reason::Covered)
    } else {
        None
    };
    if reason != app.pause_reason {
        log(&format!("pause: {:?} -> {:?}", app.pause_reason, reason));
        app.pause_reason = reason;
        if reason.is_none() {
            app.last_frame = Instant::now();
        }
        app.update_tip();
    }
}

// ---------------- tray menu ----------------
fn menu(app: &App) -> Vec<tray::Entry> {
    use tray::{Entry, MenuItem};
    let item = |id, label: &str, checked, radio| MenuItem { id, label: label.into(), checked, radio };
    let times = [TimeMode::Auto, TimeMode::Morning, TimeMode::Midday, TimeMode::Evening, TimeMode::Night];
    let weathers = [WeatherMode::Auto, WeatherMode::Clear, WeatherMode::Cloudy, WeatherMode::Rain];
    if upto() < 8 {
        return vec![Entry::Item(item(CMD_QUIT, "Quit", false, false))];
    }
    let city = if app.cfg.city.is_empty() { "Set city...".to_string() } else { format!("Set city... ({})", app.cfg.city) };
    let auto_w = if app.cfg.city.is_empty() { "Auto (set a city first)".to_string() } else { "Auto".to_string() };
    vec![
        Entry::Sub("Time".into(), times.iter().enumerate().map(|(i, t)| item(CMD_TIME + i as u32, &format!("{t:?}"), app.cfg.time_mode == *t, true)).collect()),
        Entry::Sub(
            "Weather".into(),
            weathers
                .iter()
                .enumerate()
                .map(|(i, w_)| item(CMD_WEATHER + i as u32, &if i == 0 { auto_w.clone() } else { format!("{w_:?}") }, app.cfg.weather_mode == *w_, true))
                .collect(),
        ),
        Entry::Sep,
        Entry::Item(item(CMD_DRINK, "I drank water\tCtrl+Alt+W", false, false)),
        Entry::Item(item(CMD_PAUSE, if app.cfg.paused { "Resume" } else { "Pause" }, false, false)),
        Entry::Item(item(CMD_STARTUP, "Start with Windows", startup_enabled(), false)),
        Entry::Item(item(CMD_CITY, &city, false, false)),
        Entry::Sep,
        Entry::Item(item(CMD_QUIT, "Quit", false, false)),
    ]
}

fn drink(app: &mut App) {
    if upto() < 6 {
        return;
    }
    app.cfg.last_drink_unix = config::now_unix();
    app.cfg.save();
    app.garden.drink();
    app.last_cursor_move = Instant::now(); // animate at full rate for the splash
    app.update_tip();
}

fn command(hwnd: HWND, cmd: u32) {
    match cmd {
        CMD_QUIT => {
            with_app(|a| a.quit = true);
            unsafe { PostQuitMessage(0) };
        }
        c if (CMD_TIME..CMD_TIME + 5).contains(&c) => {
            with_app(|a| {
                a.cfg.time_mode = [TimeMode::Auto, TimeMode::Morning, TimeMode::Midday, TimeMode::Evening, TimeMode::Night][(c - CMD_TIME) as usize];
                a.cfg.save();
            });
        }
        c if (CMD_WEATHER..CMD_WEATHER + 4).contains(&c) => {
            with_app(|a| {
                a.cfg.weather_mode = [WeatherMode::Auto, WeatherMode::Clear, WeatherMode::Cloudy, WeatherMode::Rain][(c - CMD_WEATHER) as usize];
                a.cfg.save();
                a.refresh_weather(false);
                a.update_tip();
            });
        }
        CMD_DRINK => {
            with_app(drink);
        }
        CMD_PAUSE => {
            with_app(|a| {
                a.cfg.paused = !a.cfg.paused;
                a.cfg.save();
                check_pause(a);
            });
        }
        CMD_STARTUP => set_startup(!startup_enabled()),
        CMD_CITY => {
            let current = with_app(|a| a.cfg.city.clone()).unwrap_or_default();
            if let Some(city) = citydlg::ask(&current) {
                let city = city.trim().to_string();
                with_app(|a| {
                    a.cfg.city = city.clone();
                    a.cfg.latitude = None;
                    a.cfg.longitude = None;
                    a.cfg.last_sky = None;
                    a.auto_sky = None;
                    a.place_label.clear();
                    a.cfg.save();
                    a.update_tip();
                });
                if !city.is_empty() {
                    let ctrl = hwnd.0 as isize;
                    std::thread::spawn(move || {
                        let place = Box::new(weather::geocode(&city));
                        unsafe {
                            let _ = PostMessageW(Some(HWND(ctrl as *mut _)), WM_PLACE, WPARAM(0), LPARAM(Box::into_raw(place) as isize));
                        }
                    });
                }
            }
        }
        _ => {}
    }
}

// ---------------- window procedures ----------------
unsafe extern "system" fn render_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_DESTROY => {
            // Our parent (Explorer) may have gone away; the controller re-attaches.
            with_app(|app| {
                if app.render == hwnd {
                    app.render = HWND::default();
                    app.gpu.unbind();
                }
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn ctrl_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        tray::WM_TRAY => {
            let ev = (lp.0 & 0xFFFF) as u32;
            if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU || ev == WM_LBUTTONUP {
                if let Some(entries) = with_app(|a| menu(a)) {
                    let cmd = tray::popup(hwnd, &entries);
                    command(hwnd, cmd);
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            command(hwnd, (wp.0 & 0xFFFF) as u32);
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wp.0 as i32 == HOTKEY_DRINK {
                with_app(drink);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            match wp.0 {
                TIMER_CHECK => {
                    with_app(|a| {
                        // Host health: re-attach if Explorer replaced the desktop windows.
                        let broken = a.render.is_invalid()
                            || !IsWindow(Some(a.render)).as_bool()
                            || a.host.map(|h| GetParent(a.render).unwrap_or_default() != h.parent).unwrap_or(true);
                        if broken && a.reattach_failures < 30 {
                            if !reattach(a) {
                                a.reattach_failures += 1;
                            }
                        }
                        check_pause(a);
                    });
                }
                TIMER_WEATHER => {
                    with_app(|a| a.refresh_weather(false));
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            const WTS_SESSION_LOCK: usize = 0x7;
            const WTS_SESSION_UNLOCK: usize = 0x8;
            with_app(|a| {
                if wp.0 == WTS_SESSION_LOCK {
                    a.locked = true;
                } else if wp.0 == WTS_SESSION_UNLOCK {
                    a.locked = false;
                }
                check_pause(a);
            });
            LRESULT(0)
        }
        WM_WEATHER => {
            with_app(|a| {
                a.fetching = false;
                let sky = match lp.0 {
                    0 => Some(Sky::Clear),
                    1 => Some(Sky::Cloudy),
                    2 => Some(Sky::Rain),
                    _ => None, // offline: keep the last known weather
                };
                log(&format!("weather: {sky:?}"));
                if let Some(s) = sky {
                    a.auto_sky = Some(s);
                    a.cfg.last_sky = Some(s);
                    a.cfg.last_weather_unix = config::now_unix();
                    a.cfg.save();
                }
                a.update_tip();
            });
            LRESULT(0)
        }
        WM_PLACE => {
            let place = Box::from_raw(lp.0 as *mut Option<weather::Place>);
            with_app(|a| {
                match *place {
                    Some(p) => {
                        log(&format!("city: {} ({:.3}, {:.3})", p.name, p.lat, p.lon));
                        a.place_label = p.name;
                        a.cfg.latitude = Some(p.lat);
                        a.cfg.longitude = Some(p.lon);
                        a.cfg.save();
                        a.refresh_weather(true);
                    }
                    None => {
                        log("city not found or offline");
                        a.place_label = format!("{} (not found)", a.cfg.city);
                    }
                }
                a.update_tip();
            });
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            with_app(|a| {
                reattach(a);
            });
            LRESULT(0)
        }
        WM_ENDSESSION | WM_CLOSE => {
            with_app(|a| a.quit = true);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            let tc = with_app(|a| a.taskbar_created).unwrap_or(0);
            if tc != 0 && msg == tc {
                log("TaskbarCreated: Explorer restarted, re-attaching");
                with_app(|a| {
                    tray::add(a.ctrl, a.icon, &a.tooltip());
                    a.reattach_failures = 0;
                    reattach(a);
                });
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

fn create_render_window(monitor: RECT) -> HWND {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap();
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("DeskGardenRender"),
            w!("Desk Garden"),
            WS_POPUP,
            monitor.left,
            monitor.top,
            monitor.right - monitor.left,
            monitor.bottom - monitor.top,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap_or_default()
    }
}

/// Find the desktop host, (re)create the render window inside it and bind the swap chain.
fn reattach(app: &mut App) -> bool {
    unsafe {
        if !app.render.is_invalid() && IsWindow(Some(app.render)).as_bool() {
            let _ = DestroyWindow(app.render);
        }
        app.render = HWND::default();
        app.gpu.unbind();
        let Some(host) = desktop::find_host() else {
            log("desktop host not found");
            return false;
        };
        log(&format!("desktop layout: {:?}", host.layout));
        // TODO(v0.2 multi-monitor): one render window per monitor.
        app.monitor = primary_monitor_rect();
        let hwnd = create_render_window(app.monitor);
        if hwnd.is_invalid() || !desktop::attach(hwnd, &host, app.monitor) {
            log("attach failed");
            if !hwnd.is_invalid() {
                let _ = DestroyWindow(hwnd);
            }
            return false;
        }
        let w = (app.monitor.right - app.monitor.left) as u32;
        let h = (app.monitor.bottom - app.monitor.top) as u32;
        // Flip model works for both layouts (tested on 25H2, layered child of Progman).
        // The blt model (DISCARD) shows nothing in a layered child, so it is only a debug option.
        let flip = std::env::var("DG_FLIP").map(|v| v != "0").unwrap_or(true);
        if let Err(e) = app.gpu.bind_window(hwnd, w, h, flip) {
            log(&format!("swap chain failed: {e}"));
            let _ = DestroyWindow(hwnd);
            return false;
        }
        app.render = hwnd;
        app.host = Some(host);
        app.reattach_failures = 0;
        app.last_frame = Instant::now();
        true
    }
}

fn install_crash_restore() {
    std::panic::set_hook(Box::new(|info| {
        log(&format!("panic: {info}"));
        desktop::restore_wallpaper();
    }));
    unsafe extern "system" fn seh(_: *const EXCEPTION_POINTERS) -> i32 {
        desktop::restore_wallpaper();
        0 // EXCEPTION_CONTINUE_SEARCH
    }
    unsafe {
        SetUnhandledExceptionFilter(Some(seh));
    }
}

fn main() {
    unsafe {
        START = Some(Instant::now());
    }
    install_crash_restore();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Remote control of a running instance (same as the tray menu), e.g. for scripts:
        //   --quit  --drink  --pause  --time auto|morning|midday|evening|night  --weather auto|clear|cloudy|rain
        let args: Vec<String> = std::env::args().skip(1).map(|a| a.to_lowercase()).collect();
        if !args.is_empty() {
            let running = FindWindowW(w!("DeskGardenCtrl"), PCWSTR::null()).ok();
            let pick = |list: &[&str], v: Option<&String>| v.and_then(|v| list.iter().position(|x| x == v)).map(|i| i as u32);
            let mut i = 0;
            let mut handled = false;
            while i < args.len() {
                let cmd = match args[i].as_str() {
                    "--quit" => Some(u32::MAX),
                    "--drink" => Some(CMD_DRINK),
                    "--pause" => Some(CMD_PAUSE),
                    "--time" => {
                        i += 1;
                        pick(&["auto", "morning", "midday", "evening", "night"], args.get(i)).map(|k| CMD_TIME + k)
                    }
                    "--weather" => {
                        i += 1;
                        pick(&["auto", "clear", "cloudy", "rain"], args.get(i)).map(|k| CMD_WEATHER + k)
                    }
                    _ => None,
                };
                i += 1;
                if let (Some(c), Some(h)) = (cmd, running) {
                    handled = true;
                    if c == u32::MAX {
                        let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
                    } else {
                        let _ = PostMessageW(Some(h), WM_COMMAND, WPARAM(c as usize), LPARAM(0));
                    }
                }
            }
            if handled || running.is_some() || args.iter().any(|a| a == "--quit") {
                return;
            }
        }
        let _mutex = CreateMutexW(None, true, w!("Local\\KHYR.DeskGarden.SingleInstance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let hinst = GetModuleHandleW(None).unwrap();
        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        let rc = WNDCLASSW { lpfnWndProc: Some(render_proc), hInstance: hinst.into(), lpszClassName: w!("DeskGardenRender"), hCursor: cursor, ..Default::default() };
        RegisterClassW(&rc);
        let cc = WNDCLASSW { lpfnWndProc: Some(ctrl_proc), hInstance: hinst.into(), lpszClassName: w!("DeskGardenCtrl"), ..Default::default() };
        RegisterClassW(&cc);
        // Hidden top-level window: owns the tray icon, timers and hotkey, and receives
        // TaskbarCreated / session-change broadcasts.
        let ctrl = CreateWindowExW(WS_EX_TOOLWINDOW, w!("DeskGardenCtrl"), w!("Desk Garden"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst.into()), None).unwrap();

        let gpu = match gfx::Gpu::new() {
            Ok(g) => g,
            Err(e) => {
                log(&format!("D3D11 device failed: {e}"));
                return;
            }
        };
        let (scene, rake_mask) = match scene::Scene::new(&gpu) {
            Ok(s) => s,
            Err(e) => {
                log(&format!("scene init failed: {e}"));
                return;
            }
        };
        let cfg = Config::load();
        let icon = tray::make_icon();
        let mut app = App {
            ctrl,
            render: HWND::default(),
            host: None,
            gpu,
            scene,
            garden: garden::Garden::new(10.0),
            rake_mask,
            place_label: cfg.city.clone(),
            cfg,
            last_frame: Instant::now(),
            monitor: primary_monitor_rect(),
            quit: false,
            first_frame_logged: false,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            icon,
            locked: false,
            pause_reason: None,
            last_cursor: POINT::default(),
            last_cursor_move: Instant::now(),
            auto_sky: None,
            fetching: false,
            reattach_failures: 0,
        };
        // Start already in the right mood (no fade-in at startup).
        app.garden.hour = app.target_hour();
        app.garden.sky = app.effective_sky();
        app.garden.basin_enabled = upto() >= 6;
        app.garden.water = app.water();
        app.garden.settle();
        tray::add(ctrl, icon, &app.tooltip());
        APP.with(|a| *a.borrow_mut() = Some(app));
        with_app(|a| {
            reattach(a);
            check_pause(a);
            a.refresh_weather(false);
        });

        SetTimer(Some(ctrl), TIMER_CHECK, 1000, None);
        SetTimer(Some(ctrl), TIMER_WEATHER, (WEATHER_REFRESH_S * 1000) as u32, None);
        let _ = WTSRegisterSessionNotification(ctrl, NOTIFY_FOR_THIS_SESSION);
        if upto() >= 6 && RegisterHotKey(Some(ctrl), HOTKEY_DRINK, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'W' as u32).is_err() {
            log("Ctrl+Alt+W is taken by another app");
        }
        // TODO(v0.2 focus tracking): feed focus minutes to an azalea bloom parameter here.
        // TODO(v0.2 break reminder): set garden.wind = 0 when a break is due.

        run_loop();

        log("quitting");
        with_app(|a| {
            tray::remove(a.ctrl);
            if !a.render.is_invalid() {
                let _ = DestroyWindow(a.render);
            }
        });
        log("render window destroyed");
        desktop::restore_wallpaper();
        log("quit, wallpaper restored");
    }
    // The wallpaper is restored and config is saved, so skip the slow graphics-driver
    // teardown on DLL detach (~3.5 s on Intel UHD 620); the OS frees everything.
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
}

/// Poll the cursor (no hooks). Spawns ripples where it moves over the visible raked gravel.
fn poll_cursor(a: &mut App) {
    let mut p = POINT::default();
    unsafe {
        if GetCursorPos(&mut p).is_err() {
            return;
        }
    }
    if p.x == a.last_cursor.x && p.y == a.last_cursor.y {
        return;
    }
    a.last_cursor = p;
    let over_desktop = unsafe { desktop::is_desktop_window(WindowFromPoint(p), a.render) };
    if !over_desktop {
        return;
    }
    a.last_cursor_move = Instant::now();
    let (scale, ox, oy) = scene::cover_fit((a.monitor.right - a.monitor.left) as f32, (a.monitor.bottom - a.monitor.top) as f32);
    let dx = ((p.x - a.monitor.left) as f32 - ox) / scale;
    let dy = ((p.y - a.monitor.top) as f32 - oy) / scale;
    if a.rake_mask.at(dx, dy) > 0.5 {
        a.garden.rake(dx, dy);
    }
}

fn run_loop() {
    unsafe {
        // High-resolution waitable timer for frame pacing (falls back to a normal one).
        let timer = CreateWaitableTimerExW(None, PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0)
            .or_else(|_| CreateWaitableTimerExW(None, PCWSTR::null(), 0, TIMER_ALL_ACCESS.0))
            .unwrap();
        let mut next_frame = Instant::now();
        let mut next_cursor = Instant::now();
        loop {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    let _ = CloseHandle(timer);
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let state = with_app(|a| {
                if a.quit {
                    return None;
                }
                let paused = a.pause_reason.is_some() || a.render.is_invalid();
                if paused {
                    return Some((true, 0.0));
                }
                let now = Instant::now();
                if upto() >= 5 && now >= next_cursor {
                    poll_cursor(a);
                    next_cursor = now + Duration::from_millis(33);
                }
                // 24 fps while anything beyond the sway moves, else 8 fps once the cursor rests 10 s.
                let busy = a.garden.busy() || a.last_cursor_move.elapsed() < Duration::from_secs(10);
                let fps = if busy { 24.0 } else { 8.0 };
                if now >= next_frame {
                    let dt = now.duration_since(a.last_frame).as_secs_f32();
                    a.last_frame = now;
                    a.garden.hour = a.target_hour();
                    a.garden.sky = a.effective_sky();
                    a.garden.water = a.water();
                    let (cb, parts) = a.garden.step(dt);
                    a.scene.draw(&a.gpu, cb, parts);
                    a.gpu.present();
                    if !a.first_frame_logged {
                        a.first_frame_logged = true;
                        log("first frame presented");
                    }
                    next_frame += Duration::from_secs_f64(1.0 / fps);
                    if next_frame < now {
                        next_frame = now;
                    }
                }
                Some((false, fps))
            });
            let Some(Some((paused, _fps))) = state else { break };
            if paused {
                // 0 fps: keep the last frame, wake only for messages (timers, tray, session).
                MsgWaitForMultipleObjects(None, false, INFINITE, QS_ALLINPUT);
                next_frame = Instant::now();
                continue;
            }
            let mut wake = next_frame;
            if upto() >= 5 && next_cursor < wake {
                wake = next_cursor;
            }
            let wait = wake.saturating_duration_since(Instant::now());
            let due = -((wait.as_nanos() / 100) as i64).max(1);
            let _ = SetWaitableTimer(timer, &due, 0, None, None, false);
            MsgWaitForMultipleObjects(Some(&[timer]), false, INFINITE, QS_ALLINPUT);
        }
        let _ = CloseHandle(timer);
    }
}
