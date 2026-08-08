//! The Windows harness: Win32 directly, because this crate has no dependencies.
//!
//! # What is different here, and why
//!
//! **There is no virtual display.** X11 gets its reproducibility partly for free:
//! `Xvfb` makes a screen nobody can see, at a size that does not depend on the
//! machine. Windows has no equivalent that a GPU will render into, so the window
//! is a real one. Two things follow, and both are deliberate:
//!
//! - The window is **sized by its client area**, not its outer frame, so a
//!   capture is the same pixels regardless of border and title-bar metrics. A
//!   shot taken here and a shot taken under `Xvfb` are the same size.
//! - It is parked **off the desktop entirely** — past the left edge of the
//!   virtual screen — and never activated (`SWP_NOACTIVATE`). So it does not
//!   appear on anyone's monitor, does not steal focus, and does not flash up
//!   mid-capture. DWM keeps composing an offscreen window, which is all the
//!   screenshot below needs.
//!
//! **Input is posted to the window, never to the desktop.** `xdotool` warps the
//! real pointer, which is fine on a display nobody is looking at and hostile on a
//! machine someone is using: the cursor jumps, and a click that lands a moment
//! late lands in whatever has focus. So this posts `WM_MOUSEMOVE` /
//! `WM_LBUTTONDOWN` / `WM_KEYDOWN` straight to the demo's own message queue. The
//! developer's pointer never moves, the demo cannot tell the difference, and a
//! capture running in the background is genuinely in the background.
//!
//! **The screenshot is tried the polite way first.** `PrintWindow` with
//! `PW_RENDERFULLCONTENT` asks the compositor for a window's contents without it
//! needing to be visible or on top — when it works, nothing appears on screen at
//! all. It does *not* reliably work for a GPU swap chain, which is exactly what
//! this engine presents, and the failure mode is a black image rather than an
//! error. So the result is checked for being uniformly black and redone from the
//! screen if it is. See [`shot`].

#![allow(non_snake_case)]

use std::path::Path;
use std::thread;
use std::time::Duration;

use super::png;

// --- The Win32 surface this needs, declared by hand -------------------------
//
// `windows-sys` would be one line in a Cargo.toml and is the obvious thing to
// reach for. It is declined for the reason stated at the top of `main.rs`: this
// crate has no dependencies, `fontbake` was split into its own workspace member
// purely to preserve that, and the twenty-odd functions below are the entire
// cost of keeping it true.

type Handle = isize;
type Bool32 = i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    clr_used: u32,
    clr_important: u32,
}

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(callback: extern "system" fn(Handle, isize) -> Bool32, param: isize) -> Bool32;
    fn GetWindowThreadProcessId(window: Handle, pid: *mut u32) -> u32;
    fn IsWindowVisible(window: Handle) -> Bool32;
    fn GetClientRect(window: Handle, rect: *mut Rect) -> Bool32;
    fn GetWindowRect(window: Handle, rect: *mut Rect) -> Bool32;
    fn ClientToScreen(window: Handle, point: *mut Point) -> Bool32;
    fn SetWindowPos(
        window: Handle,
        after: Handle,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> Bool32;
    fn PostMessageW(window: Handle, msg: u32, wparam: usize, lparam: isize) -> Bool32;
    fn GetDC(window: Handle) -> Handle;
    fn ReleaseDC(window: Handle, dc: Handle) -> i32;
    fn PrintWindow(window: Handle, dc: Handle, flags: u32) -> Bool32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    fn IsWindow(window: Handle) -> Bool32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(dc: Handle) -> Handle;
    fn CreateDIBSection(
        dc: Handle,
        info: *const BitmapInfoHeader,
        usage: u32,
        bits: *mut *mut u8,
        section: Handle,
        offset: u32,
    ) -> Handle;
    fn SelectObject(dc: Handle, object: Handle) -> Handle;
    fn BitBlt(
        dst: Handle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: Handle,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> Bool32;
    fn DeleteObject(object: Handle) -> Bool32;
    fn DeleteDC(dc: Handle) -> Bool32;
}

const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SM_XVIRTUALSCREEN: i32 = 76;
const SM_YVIRTUALSCREEN: i32 = 77;
const SRCCOPY: u32 = 0x00CC_0020;
const DIB_RGB_COLORS: u32 = 0;
const PW_CLIENTONLY: u32 = 0x0000_0001;
const PW_RENDERFULLCONTENT: u32 = 0x0000_0002;

const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_CHAR: u32 = 0x0102;
const MK_LBUTTON: usize = 0x0001;

// --- Finding the window -----------------------------------------------------

/// Where [`find_window`] accumulates its answer.
///
/// `EnumWindows` takes a plain function pointer and one `isize` of context, so
/// the callback cannot be a closure. A pointer to this, passed through that
/// parameter, is the standard way round it.
struct Search {
    pid: u32,
    best: Handle,
    best_area: i64,
}

extern "system" fn visit(window: Handle, param: isize) -> Bool32 {
    // SAFETY: `param` is the `&mut Search` handed to `EnumWindows` below, which
    // outlives the enumeration.
    let search = unsafe { &mut *(param as *mut Search) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid != search.pid || unsafe { IsWindowVisible(window) } == 0 {
        return 1;
    }

    // The **largest** visible top-level window, not the first. A process can own
    // several — on Windows a console-hosted binary owns its console too — and
    // taking the first one photographs the log instead of the demo.
    let mut rect = Rect::default();
    if unsafe { GetClientRect(window, &mut rect) } == 0 {
        return 1;
    }
    let area = (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64;
    if area > search.best_area {
        search.best_area = area;
        search.best = window;
    }
    1
}

fn find_window(pid: u32) -> Option<Handle> {
    let mut search = Search {
        pid,
        best: 0,
        best_area: 0,
    };
    unsafe { EnumWindows(visit, &mut search as *mut Search as isize) };
    (search.best != 0 && search.best_area > 0).then_some(search.best)
}

// --- The harness proper -----------------------------------------------------

pub struct Inner {
    window: Handle,
    size: (u32, u32),
    /// Where the pointer is *as far as the demo is concerned*. The real cursor
    /// is never touched, so this is the only record of it.
    cursor: (u32, u32),
    /// Set once the polite capture has been shown not to work, so the fallback
    /// is not re-litigated on every shot of the same run.
    force_screen: bool,
}

/// Nothing to install: this talks to Win32 directly.
pub fn require_tools() {}

pub fn start(w: u32, h: u32) -> Inner {
    Inner {
        window: 0,
        size: (w, h),
        cursor: (0, 0),
        force_screen: false,
    }
}

/// Nothing: the demo is launched normally and found afterwards by process id.
pub fn env(_inner: &Inner) -> Vec<(String, String)> {
    Vec::new()
}

pub fn attach(inner: &mut Inner, pid: u32) {
    // Poll rather than sleep a fixed amount: a cold start with a shader cache to
    // fill takes far longer than a warm one, and the first checkpoint cannot
    // arrive before the window does anyway.
    for _ in 0..200 {
        if let Some(window) = find_window(pid) {
            inner.window = window;
            place(inner);
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    eprintln!("error: the demo never opened a window");
    eprintln!("hint: run it directly to see why");
    std::process::exit(1);
}

/// Put the window **off** the desktop at exactly the requested **client** size,
/// without activating it.
///
/// Parked past the left edge of the virtual screen rather than on it, because a
/// window that flashes up on a monitor someone is using is the thing this
/// harness exists to avoid — and `PrintWindow` does not need it visible, only
/// un-minimised. [`on_screen`] brings it back if that turns out not to work here.
fn place(inner: &mut Inner) {
    let (w, h) = inner.size;
    let window = inner.window;

    // Size first, by measuring the frame rather than computing it. The outer
    // size that yields a given client area depends on the window's style, the
    // DPI and the Windows version, and `AdjustWindowRectEx` needs all three to
    // be passed correctly; asking the window how big it currently is, and how
    // big its client area currently is, gets the difference for free and cannot
    // disagree with reality.
    let (mut outer, mut client) = (Rect::default(), Rect::default());
    unsafe {
        GetWindowRect(window, &mut outer);
        GetClientRect(window, &mut client);
    }
    let chrome_w = (outer.right - outer.left) - (client.right - client.left);
    let chrome_h = (outer.bottom - outer.top) - (client.bottom - client.top);

    // `SM_XVIRTUALSCREEN` is the left edge of the whole virtual desktop, so a
    // window a full width further left than that is off every monitor. Windows
    // is happy to put it there; DWM keeps composing it, which is all
    // `PrintWindow` needs.
    let (x, y) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN) - (w as i32 + chrome_w) - 64,
            GetSystemMetrics(SM_YVIRTUALSCREEN) + 40,
        )
    };

    unsafe {
        SetWindowPos(
            window,
            0,
            x,
            y,
            w as i32 + chrome_w,
            h as i32 + chrome_h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    // The demo needs a frame or two to see the resize and rebuild its surface;
    // capturing through that produces a stretched or half-sized image.
    thread::sleep(Duration::from_millis(300));
}

pub fn shot(inner: &mut Inner, _root: &Path, file: &Path) {
    let (w, h) = client_size(inner);
    let Some(mut pixels) = capture(inner, w, h) else {
        eprintln!("error: could not photograph the demo's window");
        std::process::exit(1);
    };

    // A GPU swap chain frequently comes back black through `PrintWindow`, and
    // black is not an error the API reports — so it is detected here and the
    // shot is retaken from the screen, which needs the window visible and on
    // top. Sticky, because a driver that cannot do it once will not do it later.
    if !inner.force_screen && is_blank(&pixels) {
        eprintln!("note: this GPU will not hand over an offscreen window;");
        eprintln!("      showing it on the leftmost monitor to photograph it.");
        inner.force_screen = true;
        on_screen(inner);
        if let Some(second) = capture(inner, w, h) {
            pixels = second;
        }
    }

    if let Err(e) = png::write_rgb(file, w, h, &pixels) {
        eprintln!("error: cannot write {}: {e}", file.display());
        std::process::exit(1);
    }
}

fn client_size(inner: &Inner) -> (u32, u32) {
    let mut rect = Rect::default();
    unsafe { GetClientRect(inner.window, &mut rect) };
    let (w, h) = (
        (rect.right - rect.left).max(1) as u32,
        (rect.bottom - rect.top).max(1) as u32,
    );
    (w, h)
}

/// Copy the window's client area into RGB bytes.
///
/// Uses `PrintWindow` until [`Inner::force_screen`] says that does not work
/// here, then `BitBlt` from the screen at the client area's on-screen position.
fn capture(inner: &Inner, w: u32, h: u32) -> Option<Vec<u8>> {
    // A negative height asks GDI for a **top-down** DIB, so row 0 is the top
    // one. Bottom-up is the default and is a silently vertically-flipped image.
    let header = BitmapInfoHeader {
        size: std::mem::size_of::<BitmapInfoHeader>() as u32,
        width: w as i32,
        height: -(h as i32),
        planes: 1,
        bit_count: 32,
        compression: 0,
        size_image: 0,
        x_pels_per_meter: 0,
        y_pels_per_meter: 0,
        clr_used: 0,
        clr_important: 0,
    };

    unsafe {
        let screen = GetDC(0);
        let dc = CreateCompatibleDC(screen);
        let mut bits: *mut u8 = std::ptr::null_mut();
        let bitmap = CreateDIBSection(screen, &header, DIB_RGB_COLORS, &mut bits, 0, 0);
        if bitmap == 0 || bits.is_null() {
            ReleaseDC(0, screen);
            DeleteDC(dc);
            return None;
        }
        let previous = SelectObject(dc, bitmap);

        let ok = if inner.force_screen {
            // Where the client area actually is, so the title bar is not in the
            // picture and the image matches what X11's `import -window` gives.
            let mut origin = Point { x: 0, y: 0 };
            ClientToScreen(inner.window, &mut origin);
            BitBlt(
                dc, 0, 0, w as i32, h as i32, screen, origin.x, origin.y, SRCCOPY,
            ) != 0
        } else {
            PrintWindow(inner.window, dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT) != 0
        };

        let pixels = ok.then(|| {
            // GDI hands back BGRA; PNG wants RGB, and the alpha is meaningless
            // for a window capture.
            let count = (w as usize) * (h as usize);
            let src = std::slice::from_raw_parts(bits, count * 4);
            let mut out = Vec::with_capacity(count * 3);
            for pixel in src.chunks_exact(4) {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
            out
        });

        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
        ReleaseDC(0, screen);
        pixels
    }
}

/// Whether every pixel is black, which is what a failed GPU capture looks like.
///
/// No demo here renders a black frame — they all clear to a sky — so this cannot
/// be a false positive in practice, and a false *negative* just means the polite
/// path worked.
fn is_blank(pixels: &[u8]) -> bool {
    pixels.iter().all(|&byte| byte == 0)
}

/// Bring the window back onto the leftmost monitor and to the front, so the
/// screen actually holds its pixels.
///
/// Only reached when [`shot`] has found that the offscreen capture comes back
/// black, which is a property of the graphics driver rather than of anything
/// here. Still never *activated*: focus stays wherever the developer left it,
/// so a window appearing does not also mean keystrokes going somewhere new.
fn on_screen(inner: &Inner) {
    const HWND_TOP: Handle = 0;
    let (x, y) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN) + 40,
            GetSystemMetrics(SM_YVIRTUALSCREEN) + 40,
        )
    };
    unsafe {
        SetWindowPos(
            inner.window,
            HWND_TOP,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    thread::sleep(Duration::from_millis(250));
}

// --- Input, posted to the window --------------------------------------------

/// `x` and `y` packed the way a mouse message's `lParam` wants them.
fn coords(x: u32, y: u32) -> isize {
    (((y & 0xFFFF) << 16) | (x & 0xFFFF)) as isize
}

pub fn mouse_move(inner: &mut Inner, _root: &Path, x: u32, y: u32) {
    inner.cursor = (x, y);
    post(inner, WM_MOUSEMOVE, 0, coords(x, y));
}

pub fn click(inner: &mut Inner, _root: &Path) {
    let (x, y) = inner.cursor;
    // A move first, in case nothing has moved the pointer yet: a click carries a
    // position, but the demo tracks hover from motion and would otherwise see a
    // press at a place it never saw the cursor reach.
    post(inner, WM_MOUSEMOVE, 0, coords(x, y));
    post(inner, WM_LBUTTONDOWN, MK_LBUTTON, coords(x, y));
    thread::sleep(Duration::from_millis(30));
    post(inner, WM_LBUTTONUP, 0, coords(x, y));
}

pub fn wheel(inner: &mut Inner, _root: &Path, notches: i32) {
    // `WM_MOUSEWHEEL` is the one mouse message whose lParam is in **screen**
    // coordinates rather than client ones. Sending client coordinates puts the
    // pointer somewhere else entirely as far as the demo is concerned, and a
    // scroll area that hit-tests the pointer then ignores the notch — which
    // looks exactly like scrolling being broken.
    let (x, y) = inner.cursor;
    let mut point = Point {
        x: x as i32,
        y: y as i32,
    };
    unsafe { ClientToScreen(inner.window, &mut point) };
    let lparam = (((point.y & 0xFFFF) << 16) | (point.x & 0xFFFF)) as isize;

    // One `WHEEL_DELTA` per notch, in the high word, signed.
    let delta: i16 = if notches < 0 { -120 } else { 120 };
    // The low word is the modifier keys, and none are held.
    let wparam = (delta as u16 as usize) << 16;
    for _ in 0..notches.unsigned_abs() {
        post(inner, WM_MOUSEWHEEL, wparam, lparam);
        thread::sleep(Duration::from_millis(10));
    }
}

pub fn key(inner: &mut Inner, _root: &Path, name: &str) {
    let Some((vk, extended)) = virtual_key(name) else {
        eprintln!("warning: no Windows key named `{name}`");
        return;
    };

    // The scan code matters, and leaving it zero is a real bug rather than an
    // untidiness: winit identifies a physical key from the scan code in
    // `lParam`, so a message without one arrives as an unidentified key and the
    // demo silently ignores it.
    let scan = unsafe { MapVirtualKeyW(vk as u32, 0) } & 0xFF;
    let extend = if extended { 1 << 24 } else { 0 };
    let down = 1 | ((scan as isize) << 16) | extend;
    // Bits 30 and 31 mark "was down" and "is being released".
    let up = down | (0b11 << 30);

    post(inner, WM_KEYDOWN, vk as usize, down);
    // Windows synthesises `WM_CHAR` from `WM_KEYDOWN` in `TranslateMessage`,
    // which only runs for input that arrived through the real queue — so a
    // posted key produces no text unless the character is posted too. Text
    // fields read `WM_CHAR`; everything else reads the key.
    if let Some(ch) = character(name) {
        post(inner, WM_CHAR, ch as usize, down);
    }
    thread::sleep(Duration::from_millis(20));
    post(inner, WM_KEYUP, vk as usize, up);
}

fn post(inner: &Inner, msg: u32, wparam: usize, lparam: isize) {
    if inner.window == 0 || unsafe { IsWindow(inner.window) } == 0 {
        return;
    }
    unsafe { PostMessageW(inner.window, msg, wparam, lparam) };
}

/// Nothing to tear down — the demo's process is killed by the caller, and the
/// window goes with it.
pub fn stop(_inner: &mut Inner) {}

/// A capture script's key name, in `xdotool`'s vocabulary, as a virtual key and
/// whether it is an "extended" one.
///
/// Scripts are shared between platforms, so the names are `xdotool`'s and this
/// is the translation. The extended flag is not decoration: without it the arrow
/// keys carry numpad scan codes, and winit reports `Numpad4` where a script
/// meant `ArrowLeft`.
fn virtual_key(name: &str) -> Option<(u8, bool)> {
    let key = match name {
        "space" => (0x20, false),
        "Return" | "Enter" | "KP_Enter" => (0x0D, false),
        "Escape" => (0x1B, false),
        "Tab" => (0x09, false),
        "BackSpace" => (0x08, false),
        "Delete" => (0x2E, true),
        "Insert" => (0x2D, true),
        "Home" => (0x24, true),
        "End" => (0x23, true),
        "Page_Up" | "Prior" => (0x21, true),
        "Page_Down" | "Next" => (0x22, true),
        "Left" => (0x25, true),
        "Up" => (0x26, true),
        "Right" => (0x27, true),
        "Down" => (0x28, true),
        "minus" => (0xBD, false),
        "plus" | "equal" => (0xBB, false),
        "comma" => (0xBC, false),
        "period" => (0xBE, false),
        _ => {
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                // Single letters and digits are their own virtual key, upper
                // case — `VK_A` is 0x41, which is also `'A'`.
                (Some(c), None) if c.is_ascii_alphanumeric() => {
                    (c.to_ascii_uppercase() as u8, false)
                }
                _ => return None,
            }
        }
    };
    Some(key)
}

/// The character a key name types, if it types one.
fn character(name: &str) -> Option<char> {
    match name {
        "space" => Some(' '),
        "Return" | "Enter" | "KP_Enter" => Some('\r'),
        "minus" => Some('-'),
        "plus" => Some('+'),
        "equal" => Some('='),
        "comma" => Some(','),
        "period" => Some('.'),
        _ => {
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => Some(c),
                _ => None,
            }
        }
    }
}
