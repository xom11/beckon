# beckon — fix & improvement plan

Tracked work after the 2026-04-28 audit. Each item is a self-contained commit.
Order is ROI-descending.

## Round 1 — Bug fixes

- [x] **1.1** Fix `AttachThreadInput` leak on focus failure
  `crates/beckon-windows/src/window_ops.rs:163` — `BringWindowToTop(...)?`
  early-returns without detaching. Wrap attach/detach in a `Drop` guard so
  every exit path detaches.

- [x] **1.2** Atomic write for MRU state file
  `crates/beckon-linux/src/state.rs:51` — `fs::write` is not atomic; concurrent
  invocations can produce torn reads. Write to `beckon-mru.tmp` and `rename`.

- [x] **1.3** Bound recursion depth for `.lnk` scan
  `crates/beckon-windows/src/apps.rs:129` — `collect_lnk_files` recurses
  unbounded; junction loops would hang. Add `depth: u8`, bail at 8.

## Round 2 — Performance

- [ ] **2.1** Parallelize Start Menu scan with `EnumWindows` (Windows)
  `crates/beckon-windows/src/backend.rs:15` — `scan_start_menu()` and
  `enum_visible_windows()` are independent. `thread::spawn` the scan, join
  before `apps::resolve`. Cuts hot-path latency ~40–50%.

- [ ] **2.2** Parallelize installed-app scan with running query (macOS)
  `crates/beckon-macos/src/backend.rs` — same shape: `installed_apps()`
  vs `all_running_for_bundle()`.

## Round 3 — UX & error surfacing

- [ ] **3.1** Surface backend errors in `cmd_search`
  `crates/beckon-cli/src/main.rs:210` — `unwrap_or_default()` swallows IPC
  failures; print a stderr warning and continue with empty list.

- [ ] **3.2** Verbose-mode logging for silent focus failures
  - Windows: log `SetForegroundWindow` returning false
  - macOS: log `cycle_to_next_window` returning false (often AX denied)
  Gate behind `-v`; default stays quiet.

## Round 4 — Cleanups

- [ ] **4.1** Drop redundant `as isize` casts on `HWND.0`
  `crates/beckon-windows/src/backend.rs:75,78` — `HWND.0` already `isize`.

---

Ship as 8 small commits in numeric order. No squash.
