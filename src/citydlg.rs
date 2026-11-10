//! "Set city..." — a tiny native input box (no settings UI in v0.1).
//! TODO(v0.2 settings window): replace with the on-demand settings window.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT, HBRUSH, COLOR_WINDOW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

const ID_EDIT: i32 = 10;
const ID_OK: i32 = 1; // IDOK: Enter
const ID_CANCEL: i32 = 2; // IDCANCEL: Esc

struct State {
    edit: HWND,
    result: Option<String>,
    done: bool,
}

static mut STATE: Option<State> = None;

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let st = &mut *std::ptr::addr_of_mut!(STATE);
    match msg {
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as i32;
            if id == ID_OK || id == ID_CANCEL {
                if let Some(s) = st.as_mut() {
                    if id == ID_OK {
                        let mut buf = [0u16; 128];
                        let n = GetWindowTextW(s.edit, &mut buf) as usize;
                        s.result = Some(String::from_utf16_lossy(&buf[..n]));
                    }
                    s.done = true;
                }
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(s) = st.as_mut() {
                s.done = true;
            }
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Blocks until the box is closed. Returns the typed city (may be empty), or None if cancelled.
pub fn ask(current: &str) -> Option<String> {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: hinst.into(),
            lpszClassName: w!("DeskGardenCity"),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let (ww, wh) = (380, 160);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_DLGMODALFRAME,
            w!("DeskGardenCity"),
            w!("Desk Garden: set city"),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            (sw - ww) / 2,
            (sh - wh) / 2,
            ww,
            wh,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .ok()?;
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let mk = |cls: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, x: i32, y: i32, w_: i32, h: i32, id: i32| {
            let t: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let c = CreateWindowExW(ex, cls, PCWSTR(t.as_ptr()), WS_CHILD | WS_VISIBLE | style, x, y, w_, h, Some(hwnd), Some(HMENU(id as isize as *mut _)), Some(hinst.into()), None).unwrap_or_default();
            SendMessageW(c, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            c
        };
        mk(w!("STATIC"), "City for the weather (empty = no weather lookups):", WINDOW_STYLE(0), WINDOW_EX_STYLE(0), 14, 12, 340, 20, 0);
        let edit = mk(w!("EDIT"), current, WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WINDOW_EX_STYLE(0), 14, 36, 336, 24, ID_EDIT);
        mk(w!("BUTTON"), "OK", WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), WINDOW_EX_STYLE(0), 184, 76, 80, 28, ID_OK);
        mk(w!("BUTTON"), "Cancel", WS_TABSTOP, WINDOW_EX_STYLE(0), 270, 76, 80, 28, ID_CANCEL);
        STATE = Some(State { edit, result: None, done: false });
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(edit));
        SendMessageW(edit, 0x00B1 /* EM_SETSEL */, Some(WPARAM(0)), Some(LPARAM(-1)));
        let mut msg = MSG::default();
        while !(*std::ptr::addr_of!(STATE)).as_ref().map(|s| s.done).unwrap_or(true) {
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                PostQuitMessage(0);
                break;
            }
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        (*std::ptr::addr_of_mut!(STATE)).take().and_then(|s| s.result)
    }
}
