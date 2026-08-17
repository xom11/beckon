//! Chord tổng hợp có bắn được `RegisterEventHotKey` không — và nếu không,
//! có phải vì bơm sai TẦNG không?
//!
//! Đo trên macmini 2026-08-17: gõ TAY `ctrl+cmd+opt+q` thì hotkey BẮN, nhưng
//! đúng chord đó do `caps_tap::inject_chord` bơm ra thì KHÔNG — nó rơi xuống
//! terminal (kitty ghi `^[[113;15u`, đúng phím và đúng ba modifier). Trên
//! airm3 `caps_live` lại đo được `on : HOTKEY FIRED`.
//!
//! Hai biến bị tách ra ở đây, mỗi lần một biến, giữ nguyên mọi thứ khác:
//!
//! 1. **Khoảng nghỉ giữa các sự kiện.** `inject_chord` bơm cả chuỗi không có
//!    delay nào; máy này có thể nhạy thời gian còn airm3 thì không.
//! 2. **Tầng bơm.** `caps_tap.rs` post vào `kCGSessionEventTap` (1). Còn
//!    `kCGHIDEventTap` (0) bơm thấp hơn, trước cả các tap phiên khác.
//!
//! **Đối chứng chạy TRƯỚC, và nó là lý do probe này đáng tin.** Một bộ bơm
//! chết và một phát hiện thật cho ra cùng một bảng toàn "im lặng".
//! `CGEventSourceKeyState` hỏi window server xem phím có đang được giữ
//! không — đúng tầng trạng thái mà `RegisterEventHotKey` ngồi bên trên — nên
//! nếu đối chứng không đọc thấy phím ta vừa nhấn, mọi số sau đó là vô nghĩa
//! và probe phải nói vậy thay vì báo cáo một kết luận.
//!
//! ```text
//! sudo -n launchctl asuser "$(id -u)" sudo -n -u "$USER" ./chord_timing_probe
//! ```

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("macOS only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateKeyboardEvent(src: *const c_void, key: u16, down: bool) -> *mut c_void;
        fn CGEventSetFlags(ev: *mut c_void, flags: u64);
        fn CGEventPost(tap: u32, ev: *mut c_void);
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(p: *const c_void);
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    /// `kCGHIDEventTap` = 0, `kCGSessionEventTap` = 1,
    /// `kCGAnnotatedSessionEventTap` = 2. `caps_tap.rs` dùng 1.
    const TAPS: [(u32, &str); 3] = [(0, "HID"), (1, "Session"), (2, "AnnotSess")];
    /// `kCGEventSourceStateCombinedSessionState`
    const COMBINED_SESSION: i32 = 0;

    const K_CONTROL: u16 = 0x3B;
    const K_OPTION: u16 = 0x3A;
    const K_COMMAND: u16 = 0x37;
    const FLAG_CONTROL: u64 = 0x0004_0000;
    const FLAG_ALTERNATE: u64 = 0x0008_0000;
    const FLAG_COMMAND: u64 = 0x0010_0000;
    const K_SHIFT: u16 = 0x38;
    const FLAG_SHIFT: u64 = 0x0002_0000;

    static FIRED: AtomicBool = AtomicBool::new(false);
    static STEP: AtomicUsize = AtomicUsize::new(0);

    fn say(s: &str) {
        println!("{s}");
        let _ = std::io::stdout().flush();
    }

    fn post_to(tap: u32, code: u16, down: bool, flags: u64) {
        unsafe {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if ev.is_null() {
                return;
            }
            CGEventSetFlags(ev, flags);
            CGEventPost(tap, ev);
            CFRelease(ev as *const c_void);
        }
    }

    /// Đúng hình dạng `caps_tap::inject_chord` dùng: mỗi modifier là một sự
    /// kiện phím THẬT mang cờ tích luỹ (đo 2026-08-16: chỉ đặt cờ mà không
    /// nhấn phím modifier thì không bắn hotkey), rồi phím chính, rồi nhả
    /// ngược lại. Chỉ `tap` và `gap_ms` thay đổi.
    fn inject_chord(tap: u32, main_key: u16, gap_ms: u64, mods: &[(u16, u64)]) {
        let nap = || {
            if gap_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(gap_ms));
            }
        };
        let mut acc = 0u64;
        for (k, f) in mods {
            acc |= f;
            post_to(tap, *k, true, acc);
            nap();
        }
        post_to(tap, main_key, true, acc);
        nap();
        post_to(tap, main_key, false, acc);
        nap();
        for (k, f) in mods.iter().rev() {
            acc &= !f;
            post_to(tap, *k, false, acc);
            nap();
        }
    }

    /// Window server có nhìn thấy phím ta vừa bơm không? Trả về `false` nghĩa
    /// là bộ bơm chết, và mọi kết quả sau đó không nói lên điều gì.
    fn injector_reaches_window_server(tap: u32) -> bool {
        // Một modifier đơn, không ký tự nào được gõ vào cửa sổ đang focus.
        post_to(tap, K_CONTROL, true, FLAG_CONTROL);
        std::thread::sleep(std::time::Duration::from_millis(60));
        let held = unsafe { CGEventSourceKeyState(COMBINED_SESSION, K_CONTROL) };
        post_to(tap, K_CONTROL, false, 0);
        std::thread::sleep(std::time::Duration::from_millis(60));
        let released = unsafe { CGEventSourceKeyState(COMBINED_SESSION, K_CONTROL) };
        held && !released
    }

    pub fn run() {
        let ns = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!("bootstrap namespace : {ns}"));
        if ns != "Aqua" {
            say("TU CHOI: khong phai phien Aqua. RegisterEventHotKey co the bao");
            say("thanh cong roi khong bao gio giao phim o day, nen ket qua se sai.");
            std::process::exit(3);
        }
        let trusted = unsafe { AXIsProcessTrusted() };
        say(&format!("AXIsProcessTrusted  : {trusted}"));
        if !trusted {
            say("TU CHOI: CGEventPost la no-op im lang khi khong duoc tin cay,");
            say("nen 'khong ban' se khong phan biet duoc voi 'khong bom duoc'.");
            std::process::exit(3);
        }

        say("");
        say("== DOI CHUNG: bo bom co toi window server khong? ==");
        let mut live: Vec<(u32, &str)> = Vec::new();
        for (tap, name) in TAPS {
            let ok = injector_reaches_window_server(tap);
            say(&format!(
                "  tap {name:<10} -> CGEventSourceKeyState {}",
                if ok { "THAY phim" } else { "khong thay" }
            ));
            if ok {
                live.push((tap, name));
            }
        }
        if live.is_empty() {
            say("");
            say("DUNG LAI: khong tang nao bom duoc. Probe hong, khong phai phat hien.");
            std::process::exit(5);
        }
        say("");

        let mut mgr = match beckon_macos::hotkey::HotkeyManager::install(Box::new(|_id| {
            FIRED.store(true, Ordering::SeqCst);
        })) {
            Ok(m) => m,
            Err(e) => {
                say(&format!("install that bai: {e}"));
                std::process::exit(1);
            }
        };

        // f19: khong phim vat ly nao mang no va khong ung dung nao giu no --
        // cung phim `hotkey_conflict_probe` dung, vi cung ly do.
        // Chord lay tu argv, vi hai probe khac nhau o DUNG hai bien nay va
        // chi doi mot bien moi lan thi moi biet bien nao giet ket qua.
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let key_name = argv.first().cloned().unwrap_or_else(|| "f19".into());
        let has = |m: &str| argv.iter().any(|a| a == m);
        let (ctrl, cmd, opt, shift) = (has("ctrl"), has("cmd"), has("opt"), has("shift"));
        let Some(key) = beckon_core::shortcuts::lookup_key(&key_name) else {
            say(&format!("bang phim khong co '{key_name}'"));
            std::process::exit(2);
        };
        let mut mods: Vec<(u16, u64)> = Vec::new();
        if ctrl {
            mods.push((K_CONTROL, FLAG_CONTROL));
        }
        if cmd {
            mods.push((K_COMMAND, FLAG_COMMAND));
        }
        if opt {
            mods.push((K_OPTION, FLAG_ALTERNATE));
        }
        if shift {
            mods.push((K_SHIFT, FLAG_SHIFT));
        }
        if let Err(e) = mgr.register(0, ctrl, cmd, opt, shift, key) {
            say(&format!("dang ky that bai: {e}"));
            std::process::exit(1);
        }
        let names: Vec<&str> = argv.iter().skip(1).map(|s| s.as_str()).collect();
        say(&format!("da dang ky : {}+{key_name}", names.join("+")));

        // DOI CHUNG THU HAI, va no chay TRUOC: hotkey nay co the ban chut nao
        // trong tien trinh nay khong? `f19` khong co tren ban phim nen khong
        // go tay kiem duoc, va neu dang ky im lang thi moi o "im lang" ben
        // duoi noi ve PROBE chu khong phai ve chord tong hop. `hid_key` bom
        // tu MOT TIEN TRINH KHAC -- dung to hop tung do duoc `HOTKEY FIRED`
        // tren airm3.
        let hid_key = std::env::var("HID_KEY").ok().filter(|p| !p.is_empty());
        let mut cases: Vec<(Option<u32>, String, u64)> = Vec::new();
        if let Some(p) = &hid_key {
            cases.push((None, format!("NGOAI:{p}"), 0));
        }
        for (tap, name) in &live {
            for gap in [0u64, 3, 20] {
                cases.push((Some(*tap), name.to_string(), gap));
            }
        }
        say(&format!("dang bom {} truong hop...", cases.len()));
        say("");

        let results = RefCell::new(Vec::<(String, u64, bool)>::new());
        beckon_macos::hotkey::add_tick(
            0.9,
            Box::new(move || {
                let i = STEP.fetch_add(1, Ordering::SeqCst);
                if i > 0 && i <= cases.len() {
                    let fired = FIRED.load(Ordering::SeqCst);
                    let (tap, name, gap) = &cases[i - 1];
                    results.borrow_mut().push((name.clone(), *gap, fired));
                    let shown = if tap.is_none() {
                        "tien trinh KHAC".to_string()
                    } else {
                        format!("tap {name:<10} gap {gap:>2} ms")
                    };
                    say(&format!(
                        "  {shown:<26} ->  hotkey {}",
                        if fired { "BAN" } else { "im lang" }
                    ));
                }
                if i < cases.len() {
                    FIRED.store(false, Ordering::SeqCst);
                    let (tap, name, gap) = &cases[i];
                    match tap {
                        Some(t) => inject_chord(*t, key.mac, *gap, &mods),
                        None => {
                            let path = name.trim_start_matches("NGOAI:");
                            let mut a = vec![key.mac.to_string()];
                            a.extend(argv.iter().skip(1).cloned());
                            let _ = std::process::Command::new(path).args(&a).output();
                        }
                    }
                    return;
                }

                let r = results.borrow();
                let external = hid_key.is_some().then(|| r[0].2);
                let inproc: Vec<_> = r.iter().skip(usize::from(hid_key.is_some())).collect();
                let any = inproc.iter().any(|(_, _, f)| *f);
                say("");
                if external == Some(false) {
                    say("DUNG LAI: ngay ca chord bom tu TIEN TRINH KHAC cung khong ban.");
                    say("  => Dang ky hotkey trong tien trinh probe nay khong giao phim.");
                    say("  Moi o 'im lang' ben tren noi ve probe, KHONG noi gi ve chord");
                    say("  tong hop. Sua probe truoc da.");
                    std::process::exit(6);
                }
                if !any {
                    say("KET LUAN: bom tu tien trinh KHAC thi BAN, bom tu CHINH tien trinh");
                    say("  dang ky thi khong, o ca hai tang va moi muc delay.");
                    say("  => Do la nguyen nhan: CGEventPost tu chinh tien trinh giu hotkey");
                    say("     khong duoc dem ra khop voi bang hotkey cua no tren may nay.");
                } else if inproc.iter().all(|(_, _, f)| *f) {
                    say("KET LUAN: ban o MOI truong hop. => bo bom khong phai nguyen nhan.");
                } else {
                    let win: Vec<String> = r
                        .iter()
                        .filter(|(_, _, f)| *f)
                        .map(|(n, g, _)| format!("{n}/gap {g}ms"))
                        .collect();
                    say(&format!(
                        "KET LUAN: chi nhung truong hop nay ban: {}",
                        win.join(", ")
                    ));
                    say("  => do la cach sua, va no giai thich khac biet airm3 vs macmini.");
                }
                std::process::exit(if any { 0 } else { 4 });
            }),
        );

        beckon_macos::hotkey::HotkeyManager::run_forever();
    }
}
