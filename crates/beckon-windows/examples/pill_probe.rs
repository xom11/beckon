//! Gate G2: does `CDIS_HOT` reach a `BS_AUTORADIOBUTTON | BS_PUSHLIKE`?
//!
//! The whole tab strip rests on this. The design chose a pushlike radio over
//! `BS_OWNERDRAW` because owner-draw never receives `ODS_HOTLIGHT`
//! (`settings_window/mod.rs`, "the one bit a REAL `WM_DRAWITEM` never
//! carries") and because owner-draw kills `BM_GETCHECK`, which is why
//! `WM_CHIP_STATE` had to be invented. If a pushlike radio ALSO never sees
//! `CDIS_HOT`, the pills have no hover state and the choice was wrong -- the
//! named fallback is `BS_PUSHBUTTON + BS_NOTIFY` plus a `BN_SETFOCUS` arm and
//! `TrackMouseEvent`.
//!
//! **Every gate needs a control, and this one has three**, because a probe
//! that observes nothing and a control that is genuinely never hot look
//! identical:
//!
//!   1. An ordinary `BS_PUSHBUTTON` beside the radio. It is KNOWN to receive
//!      `CDIS_HOT` (nine of them do so in the real window). If the push button
//!      reports no hot state either, the probe is blind and its verdict about
//!      the radio means nothing.
//!   2. A `BS_AUTOCHECKBOX`, which is what `IDC_CAPS` is and which reaches the
//!      same custom-draw path in the shipping window.
//!   3. The mouse is moved by `SendInput` over each control in turn and then
//!      away, so "hot" is observed arriving AND leaving. A stuck bit that
//!      never clears is not a hover state.
//!
//! It also answers two questions the survey could not settle by reading, and
//! which cost nothing to fold in while a window exists:
//!
//!   - Does user32 migrate `WS_TABSTOP` onto the checked radio and off its
//!     siblings? (Spec G-S2: decides whether "the strip is ONE tab stop" is
//!     free or hand-maintained.)
//!   - Does `is_checked`'s `BM_GETCHECK` answer a `BS_AUTORADIOBUTTON`
//!     correctly? (Spec G-S3.)
//!
//! Run it on a machine with a desktop. Over SSH this lands in session 0, where
//! there is no window station, no cursor to move, and every answer is a
//! confident false negative -- go through a scheduled task in session 1 with
//! BOTH `-AllowStartIfOnBatteries` and `-Priority 4`, or run it from a VM
//! console.

#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{UpdateWindow, COLOR_BTNFACE, HBRUSH};
// Glob, matching `settings_window/mod.rs`: the BS_/CDIS_/NM* families sprawl
// across Controls and WindowsAndMessaging and the split moves between windows
// crate releases, so naming them individually is churn with no safety.
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// What each control reported, as a bitmask of the `CDIS_*` bits we care
/// about, accumulated across every `NM_CUSTOMDRAW` it sent.
static SEEN_RADIO: AtomicU32 = AtomicU32::new(0);
static SEEN_PUSH: AtomicU32 = AtomicU32::new(0);
static SEEN_CHECK: AtomicU32 = AtomicU32::new(0);
/// How many notifications arrived at all. Zero here means the probe never saw
/// custom draw, which is a different failure from "saw it, never hot".
static COUNT_RADIO: AtomicU32 = AtomicU32::new(0);
static COUNT_PUSH: AtomicU32 = AtomicU32::new(0);

const ID_RADIO_A: i32 = 101;
const ID_RADIO_B: i32 = 102;
const ID_PUSH: i32 = 103;
const ID_CHECK: i32 = 104;

fn main() {
    unsafe { run() }
}

unsafe fn run() {
    let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
    let class = w!("BeckonPillProbe");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: class,
        hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as isize as *mut _),
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
        ..Default::default()
    };
    RegisterClassW(&wc);

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!("beckon G2 probe"),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        100,
        100,
        560,
        180,
        None,
        None,
        Some(hinst.into()),
        None,
    )
    .unwrap();

    // The two radios are a group: an auto-radio only clears siblings that
    // follow it up to the next WS_GROUP, so a single radio would not exercise
    // the behaviour the strip depends on.
    mk(
        hwnd,
        hinst,
        w!("BUTTON"),
        w!("Shortcuts"),
        WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32),
        WS_GROUP | WS_TABSTOP,
        20,
        20,
        120,
        30,
        ID_RADIO_A,
    );
    mk(
        hwnd,
        hinst,
        w!("BUTTON"),
        w!("Keyboard"),
        WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32),
        WINDOW_STYLE(0),
        150,
        20,
        120,
        30,
        ID_RADIO_B,
    );
    // Control 1: known to receive CDIS_HOT.
    mk(
        hwnd,
        hinst,
        w!("BUTTON"),
        w!("Plain push"),
        WINDOW_STYLE(BS_PUSHBUTTON as u32),
        WS_TABSTOP,
        290,
        20,
        120,
        30,
        ID_PUSH,
    );
    // Control 2: what IDC_CAPS is.
    mk(
        hwnd,
        hinst,
        w!("BUTTON"),
        w!("Check"),
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        WS_TABSTOP,
        20,
        70,
        120,
        30,
        ID_CHECK,
    );

    let _ = CheckRadioButton(hwnd, ID_RADIO_A, ID_RADIO_B, ID_RADIO_A);
    let _ = UpdateWindow(hwnd);

    // Let the window settle before moving the cursor: a hover delivered while
    // the control is still being created proves nothing.
    pump(400);

    for (name, id) in [
        ("radio", ID_RADIO_A),
        ("push", ID_PUSH),
        ("check", ID_CHECK),
    ] {
        hover(hwnd, id);
        pump(500);
        println!("hovered {name}");
        // Move away, so we can also see the bit CLEAR.
        move_cursor(2000, 2000);
        pump(300);
    }

    let r = SEEN_RADIO.load(Ordering::Relaxed);
    let p = SEEN_PUSH.load(Ordering::Relaxed);
    let c = SEEN_CHECK.load(Ordering::Relaxed);

    // Which comctl32 is actually loaded. This is the control for the
    // MANIFEST, and it is the reading that was missing when the first two
    // runs came back with every count at zero: a v5 BUTTON sends no
    // NM_CUSTOMDRAW at all, so "the probe is blind" and "the manifest did not
    // take" are the same picture until this line distinguishes them.
    let ver = comctl_version();
    println!("\ncomctl32: {ver}");

    println!("\n--- G2 result ---");
    println!(
        "notifications: radio={} push={}",
        COUNT_RADIO.load(Ordering::Relaxed),
        COUNT_PUSH.load(Ordering::Relaxed)
    );
    println!("radio bits: {}", bits(r));
    println!("push  bits: {}   <- CONTROL", bits(p));
    println!("check bits: {}", bits(c));

    let push_hot = p & CDIS_HOT.0 != 0;
    let radio_hot = r & CDIS_HOT.0 != 0;
    if !push_hot {
        println!("\nINCONCLUSIVE: the control never went hot either, so this probe is blind.");
        println!("Do not read the radio result. Check that the cursor really moved");
        println!("(session 1? a real desktop?) before trusting anything here.");
    } else if radio_hot {
        println!("\nPASS: CDIS_HOT reaches a BS_PUSHLIKE auto-radio. The design's control");
        println!("choice stands and the pills get a hover state from custom draw.");
    } else {
        println!("\nFAIL: the control went hot and the radio did not. The tab strip needs");
        println!("the named fallback: BS_PUSHBUTTON + BS_NOTIFY, a BN_SETFOCUS arm and");
        println!("TrackMouseEvent. Task 3's control choice must be revisited.");
    }

    // --- G-S2 / G-S3, free while a window exists ---------------------------
    println!("\n--- G-S2: does user32 migrate WS_TABSTOP onto the checked radio? ---");
    for (name, id) in [("radio A (checked)", ID_RADIO_A), ("radio B", ID_RADIO_B)] {
        let h = GetDlgItem(Some(hwnd), id).unwrap();
        let st = GetWindowLongW(h, GWL_STYLE) as u32;
        println!(
            "  {name}: WS_TABSTOP={} WS_GROUP={}",
            st & WS_TABSTOP.0 != 0,
            st & WS_GROUP.0 != 0
        );
    }
    println!("  (if only the checked one has WS_TABSTOP, the strip is ONE tab stop for free)");

    println!("\n--- G-S3: does BM_GETCHECK answer a BS_AUTORADIOBUTTON? ---");
    for (name, id) in [("radio A (checked)", ID_RADIO_A), ("radio B", ID_RADIO_B)] {
        let h = GetDlgItem(Some(hwnd), id).unwrap();
        let st = SendMessageW(h, BM_GETCHECK, None, None).0;
        println!("  {name}: BM_GETCHECK={st}  (1 = BST_CHECKED)");
    }

    let _ = DestroyWindow(hwnd);
}

/// `DllGetVersion` on the loaded comctl32. v5 reports 5.82; a process under a
/// v6 activation context reports 6.x.
fn comctl_version() -> String {
    #[repr(C)]
    struct DllVersionInfo {
        cb_size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
    }
    unsafe {
        let lib = windows::Win32::System::LibraryLoader::LoadLibraryW(w!("comctl32.dll"));
        let Ok(lib) = lib else {
            return "LoadLibrary failed".into();
        };
        let proc = windows::Win32::System::LibraryLoader::GetProcAddress(
            lib,
            windows::core::s!("DllGetVersion"),
        );
        let Some(proc) = proc else {
            return "no DllGetVersion".into();
        };
        let f: extern "system" fn(*mut DllVersionInfo) -> i32 = std::mem::transmute(proc);
        let mut v = DllVersionInfo {
            cb_size: std::mem::size_of::<DllVersionInfo>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform_id: 0,
        };
        if f(&mut v) != 0 {
            return "DllGetVersion failed".into();
        }
        format!(
            "{}.{}.{}  ({})",
            v.major,
            v.minor,
            v.build,
            if v.major >= 6 {
                "v6 -- manifest took"
            } else {
                "v5 -- NO MANIFEST, buttons send no NM_CUSTOMDRAW"
            }
        )
    }
}

fn bits(v: u32) -> String {
    if v == 0 {
        return "<none seen>".into();
    }
    let mut s = Vec::new();
    if v & CDIS_HOT.0 != 0 {
        s.push("HOT")
    }
    if v & CDIS_SELECTED.0 != 0 {
        s.push("SELECTED")
    }
    if v & CDIS_FOCUS.0 != 0 {
        s.push("FOCUS")
    }
    if v & CDIS_CHECKED.0 != 0 {
        s.push("CHECKED")
    }
    if s.is_empty() {
        format!("{v:#x} (none of the four)")
    } else {
        s.join(" | ")
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn mk(
    parent: HWND,
    hinst: windows::Win32::Foundation::HMODULE,
    class: PCWSTR,
    text: PCWSTR,
    btn: WINDOW_STYLE,
    extra: WINDOW_STYLE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: i32,
) {
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        text,
        WS_CHILD | WS_VISIBLE | btn | extra,
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as isize as *mut _)),
        Some(hinst.into()),
        None,
    );
}

/// Move the cursor to the middle of a control with `SendInput`.
///
/// `SetCursorPos` is deliberately not used: it does not always generate the
/// `WM_MOUSEMOVE` that makes comctl32 mark a control hot, and a probe that
/// silently fails to hover would report FAIL for a control that is fine.
unsafe fn hover(parent: HWND, id: i32) {
    let h = GetDlgItem(Some(parent), id).unwrap();
    let mut rc = windows::Win32::Foundation::RECT::default();
    let _ = GetWindowRect(h, &mut rc);
    move_cursor((rc.left + rc.right) / 2, (rc.top + rc.bottom) / 2);
}

unsafe fn move_cursor(x: i32, y: i32) {
    let sw = GetSystemMetrics(SM_CXSCREEN);
    let sh = GetSystemMetrics(SM_CYSCREEN);
    let inp = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: x * 65535 / sw.max(1),
                dy: y * 65535 / sh.max(1),
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    SendInput(&[inp], std::mem::size_of::<INPUT>() as i32);
    let _ = POINT { x, y };
}

unsafe fn pump(ms: u32) {
    let end = windows::Win32::System::SystemInformation::GetTickCount() + ms;
    let mut msg = MSG::default();
    while windows::Win32::System::SystemInformation::GetTickCount() < end {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NOTIFY {
        let nmhdr = &*(lp.0 as *const NMHDR);
        if nmhdr.code == NM_CUSTOMDRAW {
            let nm = &*(lp.0 as *const NMCUSTOMDRAW);
            let id = nmhdr.idFrom as i32;
            let state = nm.uItemState.0;
            match id {
                ID_RADIO_A | ID_RADIO_B => {
                    SEEN_RADIO.fetch_or(state, Ordering::Relaxed);
                    COUNT_RADIO.fetch_add(1, Ordering::Relaxed);
                }
                ID_PUSH => {
                    SEEN_PUSH.fetch_or(state, Ordering::Relaxed);
                    COUNT_PUSH.fetch_add(1, Ordering::Relaxed);
                }
                ID_CHECK => {
                    SEEN_CHECK.fetch_or(state, Ordering::Relaxed);
                }
                _ => {}
            }
            // Ask for both stages so a control that only reports state at
            // PREERASE is not missed.
            if nm.dwDrawStage == CDDS_PREPAINT || nm.dwDrawStage == CDDS_PREERASE {
                return LRESULT(0x20); // CDRF_NOTIFYITEMDRAW
            }
            return LRESULT(0);
        }
    }
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
