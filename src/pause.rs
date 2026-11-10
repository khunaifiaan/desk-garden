//! When to stop drawing: fullscreen apps, a fully covered desktop, Battery Saver,
//! and a locked session (the lock state arrives as WM_WTSSESSION_CHANGE).

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Fullscreen,
    Covered,
    BatterySaver,
    Locked,
    User,
}

fn class_of(h: HWND) -> String {
    let mut cls = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut cls) } as usize;
    String::from_utf16_lossy(&cls[..n])
}

fn is_shell(h: HWND) -> bool {
    matches!(class_of(h).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

/// A fullscreen app on the primary monitor (games, video, presentations).
pub fn fullscreen(monitor: RECT) -> bool {
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_BUSY || state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || is_shell(fg) {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        // Covers the whole monitor including the taskbar area = fullscreen (maximized windows stop at the taskbar).
        r.left <= monitor.left && r.top <= monitor.top && r.right >= monitor.right && r.bottom >= monitor.bottom
    }
}

/// True when no point of a 9x6 grid over the work area shows the desktop.
pub fn covered(work: RECT, ours: HWND) -> bool {
    let (cols, rows) = (9, 6);
    for j in 0..rows {
        for i in 0..cols {
            let x = work.left + (work.right - work.left) * (2 * i + 1) / (2 * cols);
            let y = work.top + (work.bottom - work.top) * (2 * j + 1) / (2 * rows);
            let h = unsafe { WindowFromPoint(POINT { x, y }) };
            if crate::desktop::is_desktop_window(h, ours) {
                return false;
            }
        }
    }
    true
}

pub fn battery_saver() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).is_ok() && s.SystemStatusFlag == 1 }
}

pub fn work_area() -> RECT {
    unsafe {
        let m = windows::Win32::Graphics::Gdi::MonitorFromPoint(POINT { x: 0, y: 0 }, windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTOPRIMARY);
        let mut mi = windows::Win32::Graphics::Gdi::MONITORINFO { cbSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32, ..Default::default() };
        let _ = windows::Win32::Graphics::Gdi::GetMonitorInfoW(m, &mut mi);
        mi.rcWork
    }
}
