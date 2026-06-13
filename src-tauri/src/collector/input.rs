//! Input *counters*. Raw Input (RIDEV_INPUTSINK) lets us count keypresses and
//! clicks across all apps without focus. The handler increments a number and
//! NEVER inspects which key fired — that single fact is the privacy guarantee.
//!
//! Mouse travel is sampled from GetCursorPos on a timer (pixels), never stored
//! as a position.
//!
//! NOTE: the Raw Input union field paths (`raw.data.keyboard`, the mouse button
//! flags) are the most version-sensitive lines against the `windows` crate. The
//! logic is correct; if a path doesn't resolve, check the crate's RAWINPUT defs.

use std::sync::atomic::{AtomicU64, Ordering};

static KEYS: AtomicU64 = AtomicU64::new(0);
static CLICKS: AtomicU64 = AtomicU64::new(0);
static DIST: AtomicU64 = AtomicU64::new(0); // accumulated integer pixels

/// (keypresses, clicks, pixel-distance) since process start.
pub fn snapshot() -> (u64, u64, u64) {
    (
        KEYS.load(Ordering::Relaxed),
        CLICKS.load(Ordering::Relaxed),
        DIST.load(Ordering::Relaxed),
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn start_listener() {}

#[cfg(any(windows, target_os = "macos"))]
static LAST_X: AtomicU64 = AtomicU64::new(u64::MAX);
#[cfg(any(windows, target_os = "macos"))]
static LAST_Y: AtomicU64 = AtomicU64::new(u64::MAX);

#[cfg(any(windows, target_os = "macos"))]
fn accumulate_cursor(x: i32, y: i32) {
    let lx = LAST_X.swap(x as i64 as u64, Ordering::Relaxed);
    let ly = LAST_Y.swap(y as i64 as u64, Ordering::Relaxed);
    if lx != u64::MAX {
        let dx = (x as i64) - (lx as i64);
        let dy = (y as i64) - (ly as i64);
        let d = ((dx * dx + dy * dy) as f64).sqrt() as u64;
        DIST.fetch_add(d, Ordering::Relaxed);
    }
}

#[cfg(windows)]
pub fn start_listener() {
    std::thread::spawn(run);
}

#[cfg(windows)]
extern "system" fn wndproc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wp: windows::Win32::Foundation::WPARAM,
    lp: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::{LRESULT, POINT};
    use windows::Win32::UI::Input::{
        GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GetCursorPos, WM_INPUT, WM_TIMER,
    };

    // Raw input device types: 0 = mouse, 1 = keyboard, 2 = HID.
    const RIM_MOUSE: u32 = 0;
    const RIM_KEYBOARD: u32 = 1;
    // Button-down flags.
    const BTN_DOWN: u16 = 0x0001 | 0x0004 | 0x0010; // L | R | M
    // Keyboard flag: bit0 set => key-up (break).
    const RI_KEY_BREAK: u16 = 0x01;

    unsafe {
        match msg {
            WM_INPUT => {
                let hdr = std::mem::size_of::<RAWINPUTHEADER>() as u32;
                let mut size: u32 = 0;
                GetRawInputData(HRAWINPUT(lp.0 as *mut _), RID_INPUT, None, &mut size, hdr);
                if size > 0 {
                    let mut buf = vec![0u8; size as usize];
                    let got = GetRawInputData(
                        HRAWINPUT(lp.0 as *mut _),
                        RID_INPUT,
                        Some(buf.as_mut_ptr() as *mut _),
                        &mut size,
                        hdr,
                    );
                    if got == size {
                        let raw = &*(buf.as_ptr() as *const RAWINPUT);
                        match raw.header.dwType {
                            RIM_KEYBOARD => {
                                let kb = raw.data.keyboard;
                                if kb.Flags & RI_KEY_BREAK == 0 {
                                    KEYS.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            RIM_MOUSE => {
                                let flags = raw.data.mouse.Anonymous.Anonymous.usButtonFlags;
                                if flags & BTN_DOWN != 0 {
                                    CLICKS.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_TIMER => {
                let mut p = POINT::default();
                if GetCursorPos(&mut p).is_ok() {
                    accumulate_cursor(p.x, p.y);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[cfg(windows)]
fn run() {
    unsafe {
        use windows::Win32::UI::Input::{RAWINPUTDEVICE, RIDEV_INPUTSINK, RegisterRawInputDevices};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, RegisterClassW,
            SetTimer, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
        };
        use windows::core::{PCWSTR, w};

        let class_name = w!("TraceInputSink");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            None,
            None,
        )
        .unwrap_or_default();

        let devices = [
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06, // keyboard
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02, // mouse
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
        ];
        let _ = RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32);

        SetTimer(hwnd, 1, 100, None); // ~10Hz cursor sampling

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

// macOS input counters via a CGEventTap. Like the Windows Raw Input handler,
// the callback only ever *increments* a counter — it never inspects which key
// fired. Mouse travel is accumulated from the event location (pixels), never
// stored as a position. The tap requires Accessibility/Input-Monitoring
// permission; without it the tap fails to create and counts stay at zero, which
// the rest of the collector tolerates.
//
// NOTE: the `CGEventTap` construction and run-loop wiring are the most
// version-sensitive lines against the `core-graphics` crate; if a symbol does
// not resolve, check that crate's `event` module.
#[cfg(target_os = "macos")]
pub fn start_listener() {
    std::thread::spawn(run);
}

#[cfg(target_os = "macos")]
fn run() {
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    };

    let current = CFRunLoop::get_current();

    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![
            CGEventType::KeyDown,
            CGEventType::LeftMouseDown,
            CGEventType::RightMouseDown,
            CGEventType::OtherMouseDown,
            CGEventType::MouseMoved,
            CGEventType::LeftMouseDragged,
            CGEventType::RightMouseDragged,
        ],
        |_proxy, event_type, event| {
            match event_type {
                CGEventType::KeyDown => {
                    KEYS.fetch_add(1, Ordering::Relaxed);
                }
                CGEventType::LeftMouseDown
                | CGEventType::RightMouseDown
                | CGEventType::OtherMouseDown => {
                    CLICKS.fetch_add(1, Ordering::Relaxed);
                }
                CGEventType::MouseMoved
                | CGEventType::LeftMouseDragged
                | CGEventType::RightMouseDragged => {
                    let p = event.location();
                    accumulate_cursor(p.x as i32, p.y as i32);
                }
                _ => {}
            }
            // ListenOnly: the return value is ignored, pass the event through.
            None
        },
    );

    let tap = match tap {
        Ok(tap) => tap,
        Err(_) => {
            eprintln!(
                "[trace] could not create event tap; grant Accessibility / Input Monitoring \
                 permission to count keys and clicks. Continuing without input counts."
            );
            return;
        }
    };

    let loop_source = match tap.mach_port.create_runloop_source(0) {
        Ok(source) => source,
        Err(_) => return,
    };

    unsafe {
        current.add_source(&loop_source, kCFRunLoopCommonModes);
    }
    tap.enable();
    CFRunLoop::run_current();
}
