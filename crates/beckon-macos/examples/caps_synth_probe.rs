//! Cử chỉ Caps đi hết đường được không, khi Caps là sự kiện TỔNG HỢP?
//!
//! `caps_live` bơm Caps bằng `CGEventPost` của `kVK_CapsLock` — một `keyDown`
//! bình thường — trong khi tap của beckon chỉ nghe `kCGEventFlagsChanged`.
//! Nên probe này dựng đúng loại sự kiện: cùng keycode, nhưng đổi TYPE sang
//! `flagsChanged` và mang cờ `alphaShift`, giống hệt một cú nhấn thật.
//!
//! **Máy dò đi kèm là điều làm probe này đáng tin.** "Tap không thấy Caps
//! tổng hợp" và "tap thấy nhưng chord bơm từ callback không bắn hotkey" cho
//! ra cùng một màn hình không có gì xảy ra. Nên bước 1 gõ Caps ĐƠN và đọc
//! khoá: với `caps_tap = "capslock"` trong config, khoá dịch chuyển **chỉ
//! khi** tap đã thấy sự kiện. Không dịch chuyển thì bước 2 không nói lên gì
//! và probe phải nói vậy.
//!
//! Chạy khi `beckon serve` đang chạy với một config có `caps = true`:
//!
//! ```text
//! sudo -n launchctl asuser "$(id -u)" sudo -n -u "$USER" ./caps_synth_probe <keycode>
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
    use std::ffi::c_void;
    use std::io::Write;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateKeyboardEvent(src: *const c_void, key: u16, down: bool) -> *mut c_void;
        fn CGEventSetType(ev: *mut c_void, ty: u32);
        fn CGEventSetFlags(ev: *mut c_void, flags: u64);
        fn CGEventPost(tap: u32, ev: *mut c_void);
        fn CGEventSourceFlagsState(state: i32) -> u64;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(p: *const c_void);
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    const SESSION_TAP: u32 = 1;
    const EVENT_FLAGS_CHANGED: u32 = 12;
    const K_CAPSLOCK: u16 = 0x39;
    const FLAG_ALPHA_SHIFT: u64 = 0x0001_0000;
    const COMBINED_SESSION: i32 = 0;

    fn say(s: &str) {
        println!("{s}");
        let _ = std::io::stdout().flush();
    }

    fn nap(ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    /// Khoá Caps là bit `alphaShift` của trạng thái cờ -- mức, không phải
    /// nhất thời. `CGEventSourceKeyState(_, kVK_CapsLock)` hỏi phím có đang
    /// bị GIỮ không, và Caps nhả ngay, nên nó luôn trả `false` bất kể khoá
    /// ở đâu -- đúng cái đã làm hỏng một phép đo trước.
    fn lock_on() -> bool {
        unsafe { CGEventSourceFlagsState(COMBINED_SESSION) & FLAG_ALPHA_SHIFT != 0 }
    }

    /// Một cú nhấn Caps THẬT tới tap dưới dạng `flagsChanged`, không phải
    /// `keyDown`. Cùng keycode, khác type -- đó là toàn bộ khác biệt.
    fn post_caps(down: bool) {
        unsafe {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), K_CAPSLOCK, down);
            if ev.is_null() {
                return;
            }
            CGEventSetType(ev, EVENT_FLAGS_CHANGED);
            CGEventSetFlags(ev, if down { FLAG_ALPHA_SHIFT } else { 0 });
            CGEventPost(SESSION_TAP, ev);
            CFRelease(ev as *const c_void);
        }
    }

    fn post_key(code: u16, down: bool, flags: u64) {
        unsafe {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if ev.is_null() {
                return;
            }
            CGEventSetFlags(ev, flags);
            CGEventPost(SESSION_TAP, ev);
            CFRelease(ev as *const c_void);
        }
    }

    pub fn run() {
        if !unsafe { AXIsProcessTrusted() } {
            say("TU CHOI: khong duoc tin cay, CGEventPost la no-op im lang.");
            std::process::exit(3);
        }
        let code: u16 = std::env::args()
            .nth(1)
            .and_then(|a| a.parse().ok())
            .unwrap_or(12); // kVK_ANSI_Q
                            // Nguoi that giu Caps vai tram ms truoc khi bam phim; probe mac dinh
                            // 40 ms. Neu ket qua khac nhau theo con so nay thi do la loi that.
        let hold: u64 = std::env::args()
            .nth(2)
            .and_then(|a| a.parse().ok())
            .unwrap_or(40);
        say(&format!("giu Caps {hold} ms truoc khi bam phim"));

        say("== MAY DO: tap co thay Caps tong hop khong? ==");
        let before = lock_on();
        say(&format!("  khoa truoc      : {before}"));
        post_caps(true);
        nap(60);
        post_caps(false);
        nap(400);
        let after = lock_on();
        say(&format!("  khoa sau Caps don: {after}"));
        if before == after {
            say("");
            say("DUNG LAI: khoa khong dich chuyen, nen tap KHONG thay Caps tong hop.");
            say("  Buoc 2 se khong noi len gi -- khong chay no.");
            say("  (Doi chieu: config phai co caps=true va caps_tap=\"capslock\".)");
            std::process::exit(4);
        }
        say("  => tap CO thay. Buoc 2 co nghia.");
        say("");

        say("== THU NGHIEM: Caps + phim, chord bom tu TRONG callback ==");
        post_caps(true);
        nap(hold);
        post_key(code, true, FLAG_ALPHA_SHIFT);
        nap(60);
        post_key(code, false, FLAG_ALPHA_SHIFT);
        nap(hold);
        post_caps(false);
        nap(300);
        // Tra khoa ve cho cu: mot cu nhan don nua.
        post_caps(true);
        nap(40);
        post_caps(false);
        say("  da gui. Nguoi goi doc ung dung tien canh de cham diem.");
    }
}
