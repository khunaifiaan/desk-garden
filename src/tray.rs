//! Notification-area icon (plain Shell_NotifyIcon) and its popup menu.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

pub struct MenuItem {
    pub id: u32,
    pub label: String,
    pub checked: bool,
    pub radio: bool,
}

pub enum Entry {
    Item(MenuItem),
    Sub(String, Vec<MenuItem>),
    Sep,
}

/// Draw a small raked-gravel icon (concentric rings on moss) at runtime, so no .ico resource is needed.
pub fn make_icon() -> HICON {
    const N: i32 = 32;
    let mut px = vec![0u32; (N * N) as usize];
    for y in 0..N {
        for x in 0..N {
            let dx = x as f32 + 0.5 - 16.0;
            let dy = y as f32 + 0.5 - 16.0;
            let r = (dx * dx + dy * dy).sqrt();
            let a = ((15.5 - r).clamp(0.0, 1.0) * 255.0) as u32;
            let (cr, cg, cb) = if r < 4.5 {
                (0x6E, 0x74, 0x66) // stone
            } else if r < 13.0 {
                if ((r - 4.5) / 2.8).fract() < 0.5 { (0xD8, 0xD9, 0xC6) } else { (0x9A, 0xA0, 0x8C) } // raked sand
            } else {
                (0x3F, 0x7A, 0x4A) // moss
            };
            // premultiplied BGRA
            let pm = |c: u32| c * a / 255;
            px[(y * N + x) as usize] = (a << 24) | (pm(cr) << 16) | (pm(cg) << 8) | pm(cb);
        }
    }
    unsafe {
        let color = CreateBitmap(N, N, 1, 32, Some(px.as_ptr() as *const _));
        let mask_bits = vec![0u8; (N * N / 8) as usize];
        let mask = CreateBitmap(N, N, 1, 1, Some(mask_bits.as_ptr() as *const _));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn fill_tip(dst: &mut [u16], s: &str) {
    for (i, c) in s.encode_utf16().take(dst.len() - 1).enumerate() {
        dst[i] = c;
    }
}

pub fn add(hwnd: HWND, icon: HICON, tip: &str) {
    unsafe {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..Default::default()
        };
        fill_tip(&mut nid.szTip, tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn append(menu: HMENU, it: &MenuItem) {
    unsafe {
        let mut flags = MF_STRING;
        if it.checked {
            flags |= MF_CHECKED;
        }
        let w = wide(&it.label);
        let _ = AppendMenuW(menu, flags, it.id as usize, PCWSTR(w.as_ptr()));
        if it.radio && it.checked {
            let _ = CheckMenuRadioItem(menu, it.id, it.id, it.id, MF_BYCOMMAND.0);
        }
    }
}

/// Show the menu at the cursor; returns the chosen command id (0 = none).
pub fn popup(hwnd: HWND, entries: &[Entry]) -> u32 {
    unsafe {
        let menu = CreatePopupMenu().unwrap();
        let mut subs = Vec::new();
        for e in entries {
            match e {
                Entry::Item(it) => append(menu, it),
                Entry::Sep => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                Entry::Sub(label, items) => {
                    let sub = CreatePopupMenu().unwrap();
                    for it in items {
                        append(sub, it);
                    }
                    let w = wide(label);
                    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(w.as_ptr()));
                    subs.push(sub);
                }
            }
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(menu); // also destroys attached submenus
        cmd.0 as u32
    }
}

pub fn set_tip(hwnd: HWND, tip: &str) {
    unsafe {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_TIP | NIF_SHOWTIP,
            ..Default::default()
        };
        fill_tip(&mut nid.szTip, tip);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}
