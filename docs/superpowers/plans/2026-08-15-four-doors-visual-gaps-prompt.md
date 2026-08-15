# Prompt: bốn chỗ cửa sổ còn lệch mockup

Copy nguyên phần trong khối dưới làm prompt cho một phiên mới.

---

```
ĐỌC TRƯỚC KHI LÀM GÌ KHÁC:

  1. docs/superpowers/specs/2026-08-14-four-doors-settings-window-design.md
     — bản thiết kế ĐÃ CHỐT. §1 (split by STORE), §3.1-3.4 (bốn trang),
       §4 (list ngắn và cuộn), §6 (auto-save + 11 guard), §7 (bảy quy tắc biên tập).
  2. docs/superpowers/specs/2026-08-14-four-doors-mockup.html
     — bản vẽ. KHI SPEC TỰ MÂU THUẪN, BẢN VẼ THẮNG về chuyện gì nằm trên màn hình.
       Bài học đã trả giá: pass trước làm theo BULLET của §3.1, mà bullet không
       nhắc tới card heading, trong khi DRAWING không có heading — cửa sổ ship
       với chữ "Shortcuts" hai lần cách nhau một dòng, không ai thấy cho tới khi
       có ảnh chụp.
  3. docs/superpowers/plans/2026-08-14-four-doors-tracking.md
     — bảng đối chiếu từng dòng thiết kế ↔ thực tế. CẬP NHẬT NÓ sau mỗi thay đổi.
  4. docs/superpowers/measurements/fd-dark-{shortcuts,keyboard,system,about}.png
     — ảnh chụp thật trên a14, 1020x900 @144 DPI, sau khi cả bốn trang land.
       Đây là bằng chứng của bốn lỗi dưới.

NHÁNH: four-doors-phase-0 (đã push, 43 commit). KHÔNG thiết kế lại — bốn hướng
khác đã bị loại và §11 ghi lý do từng cái.

═══════════════════════════════════════════════════════════════════
BỐN LỖI, theo thứ tự tôi đề nghị làm
═══════════════════════════════════════════════════════════════════

■ LỖI 1 — Save và Close nằm dưới MỌI trang, kể cả System và About.

Ảnh: cả bốn PNG đều có `Open config file` / `Close` / `Save` ở đáy.

Trên System và About chúng vô nghĩa: hai trang đó KHÔNG ghi apps.toml. Thiết kế
§1 nói thẳng lý do phải tách: "Split by STORE, not by topic. Shortcuts and
Keyboard write apps.toml; System and About write HKCU\Software\beckon, the Run
key, or nothing." Và §1 nói việc tách này tự nó sửa một lỗi: hôm nay một file
config không parse được làm TOÀN BỘ cửa sổ read-only, kể cả theme switch và
Start-with-Windows, hai thứ chẳng liên quan gì tới file đó.

§6 xoá hẳn Save và Close, nhưng §6 là auto-save với mười một guard và CHƯA làm.
Nên việc của lần này là nhỏ hơn: command bar chỉ mang Save/Close trên trang nào
thực sự ghi apps.toml.

CẨN THẬN, đã đo:
  - `DefaultButton::HOME = Save` và `default_button` short-circuit trên nó, với
    lý do "Save is on screen in every state" — điều đó thôi đúng ngay khi bạn ẩn
    nó. Bốn chỗ trong beckon-windows gọi thẳng IDC_APPLY.
  - Ẩn một control KHÔNG raise focus notification (đo được), nên vòng mặc định
    ở lại trên nút đã biến mất và Enter bấm nó. `repair_hidden_button` tồn tại
    vì lý do đó; nó nhận một `successor` — dùng nó, đừng nới rộng fallback.
  - Ctrl+S là accelerator: nếu Save không có trên trang hiện tại, quyết định
    xem Ctrl+S làm gì và ghi lại. Đừng để nó ghi file một cách vô hình.

■ LỖI 2 — khoảng trống lớn dưới card trên System và About.

Ảnh fd-dark-system.png và fd-dark-about.png: card kết thúc ở khoảng giữa, phần
còn lại tới command bar là nền trống.

Mockup vẽ `.page { min-height: 326px }` — trang có chiều cao, card không co lại
bằng nội dung. Quyết định xem card nên giãn ra, hay các hàng nên giãn, hay
cửa sổ nên thấp hơn khi ở hai trang đó — và nói rõ vì sao chọn cái đó.

CẨN THẬN: mọi con số dưới MIN_HEIGHT và cạnh WINDOW_HEIGHT đã được suy lại BA
lần trong hai ngày, lần nào cũng vì một lý do khác. CHẠY LẠI cả tổng, đừng cộng
thêm vào một tổng bạn chưa tự kiểm. `compute_card_rects` trong layout.rs là
nguồn duy nhất của hình học dọc; `layout` và `card_rects` (WM_PAINT) cùng đọc nó.

■ LỖI 3 — list ở Shortcuts cao và rỗng.

Ảnh fd-dark-shortcuts.png: ba binding trong một vùng cao gần 400 px.

Thiết kế §4 nói list "short and scrolls" và uncapped (list_h là hàm của client
rect, không phải của config). Cả hai đang đúng — nhưng kết quả không giống bản
vẽ, nơi list ngắn và editor nằm ngay dưới nó.

Đây là chỗ THIẾT KẾ VÀ BẢN VẼ CÓ THỂ ĐANG NÓI HAI CHUYỆN. Đọc §4 và mockup rồi
quyết: hoặc list có trần mềm (và §4 phải được sửa lại kèm lý do), hoặc bản vẽ
chỉ là một cấu hình 6 dòng và cửa sổ đúng như đang có (và tracking phải nói thế
để người sau không "sửa" lại). Đừng im lặng chọn một bên.

■ LỖI 4 — đường dẫn hiện `\\?\C:\Users\kln\.config\beckon\`.

Ảnh fd-dark-system.png, hàng apps.toml. Tiền tố extended-length lọt ra giao diện.

Nó tới từ đâu: serve log cũng in `\\?\C:\...`, nên nhiều khả năng đường dẫn đã
mang tiền tố từ trước khi tới cửa sổ (canonicalize() trên Windows thêm nó).
Tìm nguồn thật thay vì strip ở chỗ vẽ — nếu strip ở painter thì lần sau ai đó
in đường dẫn ở chỗ khác lại lộ tiếp. Chú ý manifest đã bật longPathAware, nên
bỏ tiền tố KHÔNG làm mất khả năng xử lý path dài.

═══════════════════════════════════════════════════════════════════
GATE — năm chân, CẢ HAI target Windows
═══════════════════════════════════════════════════════════════════

  export CARGO_TARGET_DIR=/Users/lenamkhanh/Documents/dev/beckon/target
  cargo fmt --all -- --check
  cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
  cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
  cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows --all-targets -- -D warnings
  cargo clippy --target x86_64-pc-windows-msvc  -p beckon-windows --all-targets -- -D warnings

Một dòng "Finished" nhanh có thể nghĩa là KHÔNG có gì được biên dịch lại. Muốn
tin một lần clippy thì phải falsify nó trước: chèn một lỗi kiểu, xác nhận nó
fail đúng target mình quan tâm, rồi bỏ ra.

MÔI TRƯỜNG — đã đo, đừng khám phá lại:
  - SIGKILL trên máy này là SANDBOX, không phải cargo. Build script và tiến
    trình con của cargo bị giết một cách TẤT ĐỊNH dưới sandbox; cùng lệnh đó
    với sandbox tắt chạy được ngay lần đầu. Đừng "thử lại 5 lần".
  - Chạy test binary THẲNG từ target/debug/deps.
  - Một lần chạy bị giết có thể để lại .rmeta RỖNG 0 byte mà cargo coi là fresh
    → clippy báo một loạt lỗi thiếu symbol đọc y như branch mất sạch việc. Kiểm
    tra file 0 byte, và `touch crates/beckon-core/src/*.rs`.
  - Target dir dùng chung giữa các worktree phục vụ artifact cross-target CŨ, và
    `cargo clean -p` không xoá được trừ khi truyền thêm `--target`.
  - beckon-windows test KHÔNG chạy được trên macOS. Biên dịch chúng là gate; các
    invariant về bảng id phải kiểm bằng tay bằng cách parse cả hai bảng (các
    pass trước đều làm thế).

PHẦN CỨNG: a14 (zenbook-a14, Windows 11 ARM64, repo ở C:\Users\kln\dev\beckon).
SSH vào rơi vào session 0 — không desktop, không con trỏ, mọi quan sát thị giác
ở đó là FALSE NEGATIVE tự tin. Đi qua scheduled task session 1 với CẢ HAI cờ
-AllowStartIfOnBatteries -Priority 4, principal là SID chứ không phải
DOMAIN\user. Script sẵn có:
  C:\Users\kln\hwpass\run-fourdoors.ps1  — chụp cả bốn cửa, dark và light
  FindWindow KHÔNG thấy message-only window; tray của beckon là một cái, nên
  dùng EnumWindows + so class name (fourdoors.ps1 đã làm đúng).
Kill beckon* trước cargo build, nếu không link fail trên exe đang khoá VÀ để lại
binary cũ, khiến phép đo sau đó âm thầm chạy trên mã cũ.

═══════════════════════════════════════════════════════════════════
BA ĐIỀU BẮT BUỘC
═══════════════════════════════════════════════════════════════════

1. MỌI khẳng định về hành vi hiện tại phải trích file:line thật, đã mở ra xem.
   Nhánh này đã sửa hơn hai mươi comment khẳng định điều sai, trong đó có ba
   comment do chính plan sinh ra. Nếu không tìm thấy thì viết "not found".

2. MỖI thay đổi phải có một test CÓ THỂ FAIL. Phiên trước tìm được ba tautology,
   một trong số đó (`banner ^ dot` với `dot := !banner`) được viện dẫn ở BỐN chỗ
   làm bằng chứng an toàn và đúng với mọi thân hàm, kể cả một hàm không cảnh báo
   gì. Cách kiểm: dựng một implementation SAI trong đầu và hỏi test có bắt được
   không. Tốt hơn nữa là làm thật — stub hàm cho trả giá trị sai, xem test đỏ,
   rồi khôi phục.

3. KHI XONG, chụp lại cả bốn cửa trên a14 và đối chiếu với mockup từng trang.
   Ảnh là thứ duy nhất bắt được lỗi loại "chữ Shortcuts xuất hiện hai lần" — nó
   không nằm trong bất kỳ test nào và không ai đọc code mà thấy.
```
