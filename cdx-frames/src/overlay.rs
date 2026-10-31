//! The Windows side: finding the windows to decorate, one transparent overlay for each, the icon in the notification
//! area, and what a click on the frame does.
//!
//! An overlay is a layered window placed just above the window it decorates. Its pixels are the CDX frame (see paint.rs);
//! where it is transparent, the mouse goes through to the window. The window keeps running as it was: moving and sizing
//! are handed back to it with the same system commands a native title bar uses.

use crate::config;
use crate::paint::{draw_frame, palette, Canvas, Font, Hit, Layout, RING};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;
use windows::core::{w, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Console::{FreeConsole, SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The thread that runs the message loop (the console handler asks it to stop).
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

struct Tracked {
    overlay: HWND,
    layout: Layout,
    active: bool,
    title: String,
    hover: Hit,
    /// Where the overlay is (screen).
    origin: (i32, i32),
}

struct App {
    programs: Vec<String>,
    /// The list comes from the command line, not from the file.
    fixed: bool,
    list_time: Option<SystemTime>,
    paused: bool,
    font: Font,
    tracked: HashMap<isize, Tracked>,
    names: HashMap<u32, Option<String>>,
    ticks: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// What the mouse needs, kept apart from `APP` so that a click that arrives while a tick is working is never lost:
    /// overlay -> the window it decorates and the layout drawn; overlay -> the button under the mouse.
    static OWNERS: RefCell<HashMap<isize, (HWND, Layout)>> = RefCell::new(HashMap::new());
    static HOVER: RefCell<HashMap<isize, Hit>> = RefCell::new(HashMap::new());
    /// The move or size in progress, if the mouse button is down on the frame.
    static DRAG: Cell<Option<Drag>> = const { Cell::new(None) };
}

/// A move (`mode` 0) or a size by one edge or corner (`mode` as the WMSZ_ values: 1 left, 2 right, 3 top, 4 top left,
/// 5 top right, 6 bottom, 7 bottom left, 8 bottom right), done by this program: the move loop of Windows does not start
/// for a window of another process.
#[derive(Clone, Copy)]
struct Drag {
    target: HWND,
    mode: u32,
    start: POINT,
    rect: RECT,
}

const DWMWA_CORNER: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
const DWMWA_BORDER: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(34);
const COLOR_NONE: u32 = 0xFFFF_FFFE;
const COLOR_DEFAULT: u32 = 0xFFFF_FFFF;

const WM_TRAY: u32 = WM_APP + 1;
const ID_PAUSE: usize = 1;
const ID_LIST: usize = 2;
const ID_AUTOSTART: usize = 3;
const ID_QUIT: usize = 4;

fn run_key() -> PCWSTR {
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
}
fn run_name() -> PCWSTR {
    w!("CDX-Frames")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn run(cli_programs: Vec<String>, keep_console: bool) {
    unsafe {
        // one at a time: a second launch would draw a second frame on every window
        let mutex = CreateMutexW(None, true, w!("Local\\CdxFramesSingleInstance"));
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            eprintln!("cdx-frames tourne déjà (voir l'icône dans la zone de notification).");
            return;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hinst: HINSTANCE = GetModuleHandleW(None).expect("module").into();
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("CdxFrame"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let tray_class = WNDCLASSEXW { lpfnWndProc: Some(tray_proc), lpszClassName: w!("CdxFramesTray"), style: CS_HREDRAW, ..class };
        RegisterClassExW(&tray_class);
        let tray = CreateWindowExW(WS_EX_TOOLWINDOW, w!("CdxFramesTray"), w!("CDX-Frames"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None).unwrap_or_default();

        let fixed = !cli_programs.is_empty();
        let (programs, list_time) = if fixed { (cli_programs, None) } else { config::load() };
        MAIN_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true);
        println!("cdx-frames : cadre CDX autour des fenêtres de {} (Ctrl+C, ou l'icône de la zone de notification, pour tout remettre comme avant)", programs.join(", "));
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                programs,
                fixed,
                list_time,
                paused: false,
                font: Font::load(include_str!("../assets/ui-font.json")),
                tracked: HashMap::new(),
                names: HashMap::new(),
                ticks: 0,
            });
        });
        add_tray_icon(tray);
        if !keep_console {
            let _ = FreeConsole(); // started by a double click or at logon: no console window left behind
        }
        SetTimer(None, 1, 16, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.hwnd.0.is_null() {
                tick();
            } else {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        cleanup();
        remove_tray_icon(tray);
    }
}

unsafe extern "system" fn ctrl_handler(kind: u32) -> BOOL {
    if kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT || kind == CTRL_CLOSE_EVENT {
        let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_QUIT, WPARAM(0), LPARAM(0));
        // the main thread puts everything back; give it a moment before the process is closed
        std::thread::sleep(std::time::Duration::from_millis(1500));
        return BOOL(1);
    }
    BOOL(0)
}

// ------------------------------------------------------------------ the icon of the notification area

/// A 16 x 16 icon: a small CDX window, red title bar.
unsafe fn make_icon() -> HICON {
    let mut c = Canvas::new(16, 16);
    c.rect(0, 0, 16, 16, 0xFF000000);
    c.rect(1, 1, 14, 14, 0xFF880000);
    c.rect(1, 1, 14, 4, 0xFFCC0000);
    c.rect(1, 4, 14, 1, 0xFF440000);
    c.rect(2, 5, 12, 9, 0xFF333333);
    c.rect(3, 6, 10, 7, 0xFF000000);
    c.rect(4, 7, 5, 1, 0xFF00FF00);
    c.rect(4, 9, 7, 1, 0xFF00FF00);
    let mut bgra = Vec::with_capacity(16 * 16 * 4);
    for p in &c.px {
        bgra.extend_from_slice(&[*p as u8, (*p >> 8) as u8, (*p >> 16) as u8, 0xFF]);
    }
    let mask = [0u8; 16 * 2];
    CreateIcon(None, 16, 16, 1, 32, mask.as_ptr(), bgra.as_ptr()).unwrap_or_default()
}

unsafe fn tray_data(tray: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: tray, uID: 1, ..Default::default() };
    let tip = wide("CDX-Frames : cadre CDX autour des fenêtres choisies");
    let n = tip.len().min(127);
    nid.szTip[..n].copy_from_slice(&tip[..n]);
    nid
}

unsafe fn add_tray_icon(tray: HWND) {
    let mut nid = tray_data(tray);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = make_icon();
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn remove_tray_icon(tray: HWND) {
    let nid = tray_data(tray);
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

/// Does logon start the program? (the Run key of the user)
unsafe fn autostart_on() -> bool {
    let mut key = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, run_key(), None, KEY_READ, &mut key).is_err() {
        return false;
    }
    let found = RegQueryValueExW(key, run_name(), None, None, None, None).is_ok();
    let _ = RegCloseKey(key);
    found
}

unsafe fn set_autostart(on: bool) {
    let mut key = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, run_key(), None, KEY_WRITE, &mut key).is_err() {
        return;
    }
    if on {
        if let Ok(exe) = std::env::current_exe() {
            let value = wide(&format!("\"{}\"", exe.display()));
            let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
            let _ = RegSetValueExW(key, run_name(), None, REG_SZ, Some(bytes));
        }
    } else {
        let _ = RegDeleteValueW(key, run_name());
    }
    let _ = RegCloseKey(key);
}

unsafe fn show_tray_menu(tray: HWND) {
    let paused = APP.with(|c| c.try_borrow().ok().and_then(|a| a.as_ref().map(|a| a.paused))).unwrap_or(false);
    let Ok(menu) = CreatePopupMenu() else { return };
    let item = |id: usize, text: &str, checked: bool| {
        let t = wide(text);
        let _ = AppendMenuW(menu, if checked { MF_STRING | MF_CHECKED } else { MF_STRING }, id, PCWSTR(t.as_ptr()));
    };
    item(ID_PAUSE, "Suspendre les cadres", paused);
    item(ID_LIST, "Modifier la liste des programmes…", false);
    item(ID_AUTOSTART, "Démarrer avec Windows", autostart_on());
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    item(ID_QUIT, "Quitter", false);
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let _ = SetForegroundWindow(tray);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN, p.x, p.y, Some(0), tray, None);
    let _ = PostMessageW(Some(tray), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    match cmd.0 as usize {
        ID_PAUSE => APP.with(|c| {
            if let Some(app) = c.borrow_mut().as_mut() {
                app.paused = !app.paused;
                if app.paused {
                    let keys: Vec<isize> = app.tracked.keys().copied().collect();
                    for k in keys {
                        untrack(app, k);
                    }
                }
            }
        }),
        ID_LIST => {
            if let Some(path) = config::path() {
                let _ = config::load(); // make sure it exists
                let p = wide(&path.display().to_string());
                ShellExecuteW(None, w!("open"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
            }
        }
        ID_AUTOSTART => set_autostart(!autostart_on()),
        ID_QUIT => PostQuitMessage(0),
        _ => {}
    }
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_TRAY {
        let event = (lparam.0 & 0xffff) as u32;
        if event == WM_RBUTTONUP || event == WM_LBUTTONUP || event == WM_CONTEXTMENU {
            show_tray_menu(hwnd);
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ------------------------------------------------------------------ finding the windows

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    BOOL(1)
}

fn exe_name(app: &mut App, w: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(w, Some(&mut pid)) };
    if let Some(n) = app.names.get(&pid) {
        return n.clone();
    }
    let name = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok().and_then(|h| {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]).rsplit(['\\', '/']).next().unwrap_or("").to_lowercase())
        })
    };
    app.names.insert(pid, name.clone());
    name
}

/// The visible bounds, the height of the native title bar (from the top of the visible bounds), and the scale.
struct Geometry {
    vis: RECT,
    caption: i32,
    scale: i32,
}

unsafe fn geometry(w: HWND) -> Option<Geometry> {
    let mut vis = RECT::default();
    DwmGetWindowAttribute(w, DWMWA_EXTENDED_FRAME_BOUNDS, &mut vis as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).ok()?;
    let mut info = TITLEBARINFO { cbSize: std::mem::size_of::<TITLEBARINFO>() as u32, ..Default::default() };
    GetTitleBarInfo(w, &mut info).ok()?;
    if info.rgstate[0] & 0x8000 != 0 {
        return None; // STATE_SYSTEM_INVISIBLE: no title bar to replace
    }
    let caption = info.rcTitleBar.bottom - vis.top;
    if caption <= 0 || vis.right - vis.left < 120 {
        return None;
    }
    let dpi = GetDpiForWindow(w).max(96);
    Some(Geometry { vis, caption, scale: ((dpi as f32 / 96.0).round() as i32).clamp(1, 3) })
}

fn eligible(app: &mut App, w: HWND) -> Option<Geometry> {
    unsafe {
        if !IsWindowVisible(w).as_bool() || IsIconic(w).as_bool() || IsZoomed(w).as_bool() {
            return None;
        }
        let style = GetWindowLongW(w, GWL_STYLE) as u32;
        let ex = GetWindowLongW(w, GWL_EXSTYLE) as u32;
        if style & WS_CAPTION.0 != WS_CAPTION.0 || ex & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(w, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        if cloaked != 0 {
            return None;
        }
        let name = exe_name(app, w)?;
        if !app.programs.iter().any(|p| *p == name) {
            return None;
        }
        geometry(w)
    }
}

fn title_of(w: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(w, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

// ------------------------------------------------------------------ one tick: follow the windows

fn tick() {
    APP.with(|cell| {
        let Ok(mut guard) = cell.try_borrow_mut() else { return };
        let Some(app) = guard.as_mut() else { return };
        app.ticks = app.ticks.wrapping_add(1);
        // the list file is read again when it changes (looked at once a second)
        if !app.fixed && app.ticks % 60 == 0 {
            let m = config::modified();
            if m != app.list_time {
                let (programs, time) = config::load();
                app.programs = programs;
                app.list_time = time;
            }
        }
        if app.paused {
            return;
        }
        let mut all: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut all as *mut _ as isize));
        }
        let fg = unsafe { GetForegroundWindow() };
        let mut seen: HashSet<isize> = HashSet::new();
        for w in all {
            let Some(g) = eligible(app, w) else { continue };
            seen.insert(w.0 as isize);
            follow(app, w, &g, w == fg);
        }
        let gone: Vec<isize> = app.tracked.keys().filter(|k| !seen.contains(k)).copied().collect();
        for k in gone {
            untrack(app, k);
        }
        // the button under the mouse, as the frame's window procedure noted it
        let lit: Vec<(isize, Hit)> = app
            .tracked
            .iter()
            .map(|(k, t)| (*k, HOVER.with(|h| h.borrow().get(&(t.overlay.0 as isize)).copied().unwrap_or(Hit::None))))
            .filter(|(k, h)| app.tracked.get(k).is_some_and(|t| t.hover != *h))
            .collect();
        for (k, h) in lit {
            if let Some(t) = app.tracked.get_mut(&k) {
                t.hover = h;
            }
            repaint(app, k);
        }
    });
}

/// Draws the frame of a tracked window again and puts it on the screen.
fn repaint(app: &mut App, key: isize) {
    let Some(t) = app.tracked.get(&key) else { return };
    let mut canvas = Canvas::new(t.layout.w, t.layout.h);
    draw_frame(&mut canvas, &t.layout, &palette(t.active), &t.title, &app.font, t.hover);
    unsafe { present(t.overlay, &canvas, t.origin) };
}

fn follow(app: &mut App, w: HWND, g: &Geometry, active: bool) {
    let key = w.0 as isize;
    let (vw, vh) = (g.vis.right - g.vis.left, g.vis.bottom - g.vis.top);
    let layout = Layout::around(vw, vh, g.caption, g.scale);
    let origin = (g.vis.left - RING * g.scale, g.vis.top - g.scale);
    let title = title_of(w);
    if !app.tracked.contains_key(&key) {
        unsafe {
            let corner: u32 = 1; // DWMWCP_DONOTROUND: square corners, like CDX
            let _ = DwmSetWindowAttribute(w, DWMWA_CORNER, &corner as *const _ as *const _, 4);
            let none = COLOR_NONE;
            let _ = DwmSetWindowAttribute(w, DWMWA_BORDER, &none as *const _ as *const _, 4);
            let hinst = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
            let overlay = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("CdxFrame"),
                w!(""),
                WS_POPUP,
                origin.0,
                origin.1,
                layout.w,
                layout.h,
                None,
                None,
                Some(hinst),
                None,
            );
            let Ok(overlay) = overlay else { return };
            app.tracked.insert(key, Tracked { overlay, layout: Layout { w: 0, ..layout }, active: !active, title: String::new(), hover: Hit::None, origin: (i32::MIN, 0) });
        }
    }
    let t = app.tracked.get_mut(&key).expect("tracked");
    let redraw = t.layout != layout || t.active != active || t.title != title;
    if redraw {
        t.layout = layout;
        t.active = active;
        t.title = title;
        t.origin = origin;
        let overlay = t.overlay;
        OWNERS.with(|o| o.borrow_mut().insert(overlay.0 as isize, (w, layout)));
        repaint(app, key);
        unsafe {
            let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
        }
    } else if t.origin != origin {
        unsafe {
            let _ = SetWindowPos(t.overlay, None, origin.0, origin.1, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        t.origin = origin;
    }
    let t = app.tracked.get(&key).expect("tracked");
    // directly above the window it decorates (in the topmost band too, for a window that stays on top)
    unsafe {
        let topmost = GetWindowLongW(w, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
        let overlay_topmost = GetWindowLongW(t.overlay, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
        if topmost != overlay_topmost {
            let band = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
            let _ = SetWindowPos(t.overlay, Some(band), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        if GetWindow(t.overlay, GW_HWNDNEXT).ok() != Some(w) {
            let above = GetWindow(w, GW_HWNDPREV).ok();
            let after = match above {
                Some(a) if a != t.overlay => a,
                Some(_) => return,
                None => HWND_TOP,
            };
            let _ = SetWindowPos(t.overlay, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn untrack(app: &mut App, key: isize) {
    if let Some(t) = app.tracked.remove(&key) {
        OWNERS.with(|o| o.borrow_mut().remove(&(t.overlay.0 as isize)));
        HOVER.with(|h| h.borrow_mut().remove(&(t.overlay.0 as isize)));
        unsafe {
            let w = HWND(key as *mut _);
            let _ = DestroyWindow(t.overlay);
            // put the window back as it was
            let corner: u32 = 0;
            let _ = DwmSetWindowAttribute(w, DWMWA_CORNER, &corner as *const _ as *const _, 4);
            let border = COLOR_DEFAULT;
            let _ = DwmSetWindowAttribute(w, DWMWA_BORDER, &border as *const _ as *const _, 4);
        }
    }
}

fn cleanup() {
    APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            let keys: Vec<isize> = app.tracked.keys().copied().collect();
            for k in keys {
                untrack(app, k);
            }
        }
    });
}

// ------------------------------------------------------------------ putting the pixels on the screen

unsafe fn present(overlay: HWND, canvas: &Canvas, origin: (i32, i32)) {
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: canvas.w,
        biHeight: -canvas.h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    if let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(canvas.px.as_ptr(), bits as *mut u32, canvas.px.len());
            let old = SelectObject(mem, bmp.into());
            let pos = POINT { x: origin.0, y: origin.1 };
            let size = SIZE { cx: canvas.w, cy: canvas.h };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(overlay, Some(screen), Some(&pos), Some(&size), Some(mem), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
            SelectObject(mem, old);
        }
        let _ = DeleteObject(bmp.into());
    }
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
}

// ------------------------------------------------------------------ what the frame does when it is clicked

fn owner_of(overlay: HWND) -> Option<(HWND, Layout)> {
    OWNERS.with(|o| o.borrow().get(&(overlay.0 as isize)).copied())
}

/// The button under the mouse changed: noted here, the next tick draws the frame again with it lit.
fn set_hover(overlay: HWND, hover: Hit) {
    let is_button = matches!(hover, Hit::Close | Hit::Menu | Hit::Zoom | Hit::Shade);
    HOVER.with(|h| h.borrow_mut().insert(overlay.0 as isize, if is_button { hover } else { Hit::None }));
}

fn cursor_for(hit: Hit) -> PCWSTR {
    match hit {
        Hit::Edge(1) | Hit::Edge(2) => IDC_SIZEWE,
        Hit::Edge(3) | Hit::Edge(6) => IDC_SIZENS,
        Hit::Edge(4) | Hit::Edge(8) => IDC_SIZENWSE,
        Hit::Edge(5) | Hit::Edge(7) => IDC_SIZENESW,
        _ => IDC_ARROW,
    }
}

unsafe fn system_menu(target: HWND, overlay: HWND, x: i32, y: i32) {
    let menu = GetSystemMenu(target, false);
    if menu.is_invalid() {
        return;
    }
    let _ = SetForegroundWindow(overlay);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY, x, y, Some(0), overlay, None);
    let _ = PostMessageW(Some(overlay), WM_NULL, WPARAM(0), LPARAM(0));
    if cmd.0 > 0 {
        let _ = PostMessageW(Some(target), WM_SYSCOMMAND, WPARAM(cmd.0 as usize), LPARAM(0));
    }
}

unsafe fn begin_drag(overlay: HWND, target: HWND, mode: u32) {
    if IsZoomed(target).as_bool() {
        return;
    }
    let mut start = POINT::default();
    let mut rect = RECT::default();
    let _ = GetCursorPos(&mut start);
    let _ = GetWindowRect(target, &mut rect);
    DRAG.set(Some(Drag { target, mode, start, rect }));
    SetCapture(overlay);
}

unsafe fn continue_drag() {
    let Some(d) = DRAG.get() else { return };
    let mut now = POINT::default();
    let _ = GetCursorPos(&mut now);
    let (dx, dy) = (now.x - d.start.x, now.y - d.start.y);
    let (mut l, mut t, mut r, mut b) = (d.rect.left, d.rect.top, d.rect.right, d.rect.bottom);
    if d.mode == 0 {
        (l, t, r, b) = (l + dx, t + dy, r + dx, b + dy);
    } else {
        if matches!(d.mode, 1 | 4 | 7) {
            l += dx;
        }
        if matches!(d.mode, 2 | 5 | 8) {
            r += dx;
        }
        if matches!(d.mode, 3 | 4 | 5) {
            t += dy;
        }
        if matches!(d.mode, 6 | 7 | 8) {
            b += dy;
        }
    }
    let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
    if d.mode == 0 {
        flags |= SWP_NOSIZE;
    }
    let _ = SetWindowPos(d.target, None, l, t, (r - l).max(1), (b - t).max(1), flags);
}

unsafe fn toggle_zoom(target: HWND) {
    let _ = ShowWindow(target, if IsZoomed(target).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let point = || ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE if DRAG.get().is_some() => {
            continue_drag();
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            if DRAG.take().is_some() && msg == WM_LBUTTONUP {
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point();
            if let Some((_, l)) = owner_of(hwnd) {
                set_hover(hwnd, l.hit(x, y));
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
            }
            LRESULT(0)
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            set_hover(hwnd, Hit::None);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = ScreenToClient(hwnd, &mut p);
            if let Some((_, l)) = owner_of(hwnd) {
                if let Ok(c) = LoadCursorW(None, cursor_for(l.hit(p.x, p.y))) {
                    SetCursor(Some(c));
                    return LRESULT(1);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            let (x, y) = point();
            let Some((target, l)) = owner_of(hwnd) else { return LRESULT(0) };
            let hit = l.hit(x, y);
            if hit == Hit::None {
                return LRESULT(0);
            }
            let _ = SetForegroundWindow(target);
            match hit {
                Hit::Close => drop(PostMessageW(Some(target), WM_CLOSE, WPARAM(0), LPARAM(0))),
                Hit::Zoom => toggle_zoom(target),
                Hit::Shade => drop(ShowWindow(target, SW_MINIMIZE)),
                Hit::Menu => {
                    let mut p = POINT { x, y };
                    let _ = ClientToScreen(hwnd, &mut p);
                    system_menu(target, hwnd, p.x, p.y);
                }
                Hit::Title if msg == WM_LBUTTONDBLCLK => toggle_zoom(target),
                Hit::Title => begin_drag(hwnd, target, 0),
                Hit::Edge(d) => begin_drag(hwnd, target, d as u32),
                Hit::None => {}
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (x, y) = point();
            if let Some((target, l)) = owner_of(hwnd) {
                if l.hit(x, y) == Hit::Title {
                    let mut p = POINT { x, y };
                    let _ = ClientToScreen(hwnd, &mut p);
                    system_menu(target, hwnd, p.x, p.y);
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
