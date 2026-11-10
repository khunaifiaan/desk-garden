//! Placing a window between the wallpaper and the desktop icons.
//!
//! Sending the undocumented message 0x052C to Progman asks Explorer to split the
//! desktop into an icon layer (SHELLDLL_DefView) and a separate wallpaper host
//! (WorkerW). Where those windows end up depends on the Windows build:
//!
//! * Classic layout (Windows 10, Windows 11 before 24H2): DefView moves into a
//!   top-level WorkerW, and the wallpaper WorkerW is the next top-level WorkerW
//!   in z-order. We become a child of that wallpaper WorkerW.
//! * 24H2 layout: DefView stays a child of Progman and the wallpaper WorkerW is
//!   also a child of Progman. We become a layered child of Progman placed between
//!   DefView (icons, on top) and the WorkerW (wallpaper, below).
//!
//! The layout is detected at runtime after 0x052C is sent.

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{MapWindowPoints, RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Classic,
    Raised24H2,
}

#[derive(Clone, Copy, Debug)]
pub struct Host {
    pub layout: Layout,
    pub progman: HWND,
    pub parent: HWND,
    pub defview: HWND,
    pub workerw: HWND,
}

unsafe fn find_child(parent: HWND, after: HWND, class: PCWSTR) -> HWND {
    FindWindowExW(Some(parent), Some(after), class, PCWSTR::null()).unwrap_or_default()
}

/// Ask Explorer to create the wallpaper WorkerW and report where everything is.
pub fn find_host() -> Option<Host> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let mut res = 0usize;
        // wParam 0xD / lParam 0x1 is what recent Explorer builds expect; plain 0/0 is the older form.
        let _ = SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(0x1), SMTO_NORMAL, 1000, Some(&mut res));
        let _ = SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));

        // 24H2: icons and wallpaper host are both children of Progman.
        let defview = find_child(progman, HWND::default(), w!("SHELLDLL_DefView"));
        if !defview.is_invalid() {
            let workerw = find_child(progman, HWND::default(), w!("WorkerW"));
            return Some(Host { layout: Layout::Raised24H2, progman, parent: progman, defview, workerw });
        }

        // Classic: find the top-level WorkerW holding DefView, then the WorkerW after it.
        struct Found {
            defview: HWND,
            icon_host: HWND,
        }
        unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
            let f = &mut *(lp.0 as *mut Found);
            let dv = FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()).unwrap_or_default();
            if !dv.is_invalid() {
                f.defview = dv;
                f.icon_host = hwnd;
                return BOOL(0);
            }
            BOOL(1)
        }
        let mut f = Found { defview: HWND::default(), icon_host: HWND::default() };
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut f as *mut _ as isize));
        if f.defview.is_invalid() {
            return None;
        }
        let workerw = FindWindowExW(None, Some(f.icon_host), w!("WorkerW"), PCWSTR::null()).unwrap_or_default();
        if workerw.is_invalid() {
            return None;
        }
        Some(Host { layout: Layout::Classic, progman, parent: workerw, defview: f.defview, workerw })
    }
}

/// Re-parent `hwnd` into the wallpaper slot and size it to `screen_rect` (screen coordinates).
pub fn attach(hwnd: HWND, host: &Host, screen_rect: RECT) -> bool {
    unsafe {
        let layered = std::env::var("DG_LAYERED").map(|v| v != "0").unwrap_or(true);
        crate::log(&format!("layered={}", host.layout == Layout::Raised24H2 && layered));
        if host.layout == Layout::Raised24H2 && layered {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
            let _ = SetLayeredWindowAttributes(hwnd, Default::default(), 255, LWA_ALPHA);
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let style = (style & !(WS_POPUP.0 as isize | WS_CAPTION.0 as isize | WS_THICKFRAME.0 as isize))
            | WS_CHILD.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        if SetParent(hwnd, Some(host.parent)).is_err() {
            return false;
        }
        // Screen -> parent client coordinates.
        let mut pts = [
            windows::Win32::Foundation::POINT { x: screen_rect.left, y: screen_rect.top },
            windows::Win32::Foundation::POINT { x: screen_rect.right, y: screen_rect.bottom },
        ];
        MapWindowPoints(None, Some(host.parent), &mut pts);
        let (x, y) = (pts[0].x, pts[0].y);
        let (w, h) = (pts[1].x - pts[0].x, pts[1].y - pts[0].y);
        match host.layout {
            Layout::Classic => {
                let _ = SetWindowPos(hwnd, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
            Layout::Raised24H2 => {
                // Directly below the icon view...
                let _ = SetWindowPos(hwnd, Some(host.defview), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                // ...and the wallpaper WorkerW below us.
                if !host.workerw.is_invalid() {
                    let _ = SetWindowPos(host.workerw, Some(hwnd), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
        }
        true
    }
}

/// Repaint the normal wallpaper. Safe to call any time, including from crash handlers.
pub fn restore_wallpaper() {
    unsafe {
        let mut buf = [0u16; 512];
        let _ = SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        // Re-setting the current wallpaper (without writing the ini) forces Explorer to repaint it.
        let _ = SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(buf.as_mut_ptr() as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
            let _ = RedrawWindow(Some(progman), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN);
        }
    }
}

/// True if `hwnd` belongs to the desktop (icons, wallpaper host, or our own window).
pub fn is_desktop_window(hwnd: HWND, ours: HWND) -> bool {
    unsafe {
        let mut cur = hwnd;
        for _ in 0..6 {
            if cur.is_invalid() {
                return false;
            }
            if cur == ours {
                return true;
            }
            let mut cls = [0u16; 64];
            let n = GetClassNameW(cur, &mut cls) as usize;
            let name = String::from_utf16_lossy(&cls[..n]);
            if name == "Progman" || name == "WorkerW" || name == "SHELLDLL_DefView" {
                return true;
            }
            cur = GetParent(cur).unwrap_or_default();
        }
        false
    }
}
