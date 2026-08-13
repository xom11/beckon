# Direction B — what is shipped, what is left

Date: 2026-08-13. Written as a handoff: a fresh session should be able to
start work after reading this one file.

## What direction B is

Three redesigns of the Windows settings window were drawn and compared; the
user chose **B, "keycaps"**. Design board (annotated with shipped status):
<https://claude.ai/code/artifact/3aaeb923-f07b-4686-bc1d-0c1d7ae22876>

B has **two layers**, and only the first is on screen:

1. **Composition** — the editor stops being seven controls on one line and
   becomes a titled group of two, App on its own line. *Shipped.*
2. **The keycap language** — the chord is drawn as physical keys rather than
   written as text. *Half shipped.*

The user's own words after v0.9.0, which shipped layer 1 alone: *"ngoài super
→ win ra thì không khác gì cũ, trong bản thiết kế, bạn có vẽ là các nút là các
vuông"*. Layer 2 is what makes B recognisably B, and the modifier chips are
the half of it still missing.

## Shipped

| Version | What |
|---|---|
| **v0.9.0** | Landing 3a: the editor is two lines in a titled group; App gets a full-width line; `tok::KEY_COL` and `tok::BTN_SM` retired; notes capped at two lines; `MIN_HEIGHT` 460 → 550; `super` leaves the UI (`ctrl+super+alt+t` → the string `Ctrl + Win + Alt + T`); two hardware probes added, unrun |
| **v0.9.1** | The **Shortcut column** is drawn as keycaps — `NM_CUSTOMDRAW` on subitem 1 |
| merged, **unreleased** | §F.4: a modifier held before `Record` was clicked now reaches the chord (`7d2b7dd`) |

Specs and plans: `docs/superpowers/specs/2026-08-12-settings-keycaps-design.md`,
`docs/superpowers/plans/2026-08-12-settings-keycaps-landing-3a.md` (its closing
section records two plan defects, three human rulings and two parked findings).

## What is left

### 1. The four modifier chips, and the three `Hold` chips

The visible gap. They are still `BS_AUTOCHECKBOX` — a small square plus a word
— where the design draws them as toggle keycaps, filled with the accent when
armed. The painter already exists: `draw_keycaps` in `settings_window.rs`,
which the Shortcut column uses. Reuse it; do not write a second one.

Everything that makes this bigger than it looks:

- **`BS_OWNERDRAW` replaces `BS_AUTOCHECKBOX`.** They are alternative values
  of one style field, not flags that combine. So **Windows stops tracking
  checked state**: `BM_SETCHECK` / `BM_GETCHECK` stop meaning anything on
  those seven controls. Five sites currently use them
  (`settings_window.rs:1151`, `3061`, `3184`, `3549`, `4495` at the time of
  writing — re-grep, the file moves). The window must track state itself and
  toggle on `BN_CLICKED`.
- The model is already the source of truth (`ControlState` holds the
  `ComboView`, `commit_fields` compares `ComboView`s not strings), so this is
  a change to the *push*, not to the architecture: `InvalidateRect` where
  `BM_SETCHECK` used to be.
- **Re-read `settings_window.rs:4495` before touching it.** Its comment
  reasons about `BM_SETCHECK` not raising a notification. An owner-draw toggle
  raises whatever you make it raise, so that reasoning does not carry over.
- **`WM_DRAWITEM` does not exist in this crate yet.** Grep confirms: no
  `WM_DRAWITEM`, no `BS_OWNERDRAW` anywhere. This is the first owner-draw
  surface in the window.
- Owner-draw draws no focus indication of its own → `DrawFocusRect` on
  `ODS_FOCUS`, or the keyboard route is silently lost.
- `ODS_DISABLED` → `COLOR_GRAYTEXT`, no fill. This actually *fixes* a §1
  complaint: unlike the `CBS_DROPDOWNLIST` beside them, an owner-draw chip
  looks disabled when it is.
- **Colours come from `GetSysColor`, never a literal.** `COLOR_HIGHLIGHT` /
  `COLOR_HIGHLIGHTTEXT` for an armed chip — it is the user's own accent, it
  matches the row highlight, and it is already correct in high contrast. The
  design mockup's `#2563eb` is **not** a colour specification.
- `draw_keycaps` already takes an `hc: bool` and draws a hard rectangle with
  no bottom edge in high contrast. Keep that path.
- The three `Hold` chips are in scope *with* the four, not after: they name
  the same three modifiers eight lines apart, and shipping two chip styles in
  one window is worse than either alone.
- The `Hold` chips keep their mnemonics (`t`, `w`, `l`), so `draw_keycaps`
  must render the `&` rather than print it: pass the caption with `&` intact
  and **without** `DT_NOPREFIX`, then add `DT_HIDEPREFIX` when
  `SPI_GETKEYBOARDCUES` says cues are off. The four editor chips carry no
  mnemonic by design — `mod cap`'s table is the only guard and `Hold` already
  claimed those letters.

### 2. Cut 0.9.2

§F.4 is merged but unreleased. Until it ships, a14's scoop install is 0.9.1
and the fix is only in a hand-built binary that does not survive the
watchdog's next restart.

Release is `git tag -a vX.Y.Z && git push origin vX.Y.Z`. **`release.yml`
chains the packager bump itself** through `workflow_call` — measured at
v0.9.0 and v0.9.1, both bumped the scoop manifest with no manual dispatch.
`CLAUDE.md` still says the bump does not fire on its own; **that claim is
false and should be corrected**.

A version bump must touch **three** files or CI fails: `Cargo.toml`,
`Cargo.lock` (via `cargo update --workspace --offline`) and
**`site/index.html`** — `tools/check-site.sh` asserts the page's version
matches `Cargo.toml`.

### 3. Hardware gates, none of them run

`docs/superpowers/plans/2026-08-12-settings-keycaps-landing-3a.md` has the
table with setups written out. The ones that still matter:

- **G1 — 96 DPI.** Nobody has seen this window at 96 DPI. a14 runs at **144**.
  An earlier claim that G1 was answered came from *virtualized* coordinates
  handed to a DPI-unaware probe; see the trap below.
- **G3 — `customdraw_probe.exe`.** Decides whether the App column's flag can
  have its own font (§B.6). Built, never run. Does **not** block the chips.
- **G4 — the §F.4 fix, by hand.** Hold `Ctrl`, click `Record` with the mouse,
  press `Alt+T`; it must record `ctrl+alt+t`. Needs a keyboard and a mouse at
  once, so no probe can do it.
- **eye3** — an app Name containing `&` (`Notes & To Do`): the group caption
  must not eat the character or underline the next one.
- **eye4** — pick `comma` from the key list; the cell must say `,` not
  `COMMA`. `settings_probe`'s `key_cap` is a transcribed copy of
  `key_label` and an ordinary run only exercises its uppercase fallthrough,
  so a drift in the 25 special arms would be mirrored rather than caught.
- **High contrast** — the branch exists and is reachable; nobody has toggled
  the theme and looked.

## The environment, and four traps that cost real time

**The Rust toolchain on the Mac is currently unusable for this crate.** Build
scripts are SIGKILLed at random in a fresh worktree — with the sandbox off,
with `-j 1`, and not from memory pressure (63 % free, load 2.0). Each victim
runs fine when exec'd by hand, then cargo kills the next one. **Use a14 as the
compiler**: push, then `cd C:\Users\kln\dev\beckon; git fetch; git reset --hard
origin/<branch>; cargo build --release --bin beckon-serve` — about 15 s.
`cargo fmt` and `cargo test -p beckon-core` do work locally, eventually.

The cross-check that *does* work when the machine cooperates:
`cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets`
— `cargo check` never links, so MSVC is not needed. `--all-targets` is what
compiles `examples/` at all.

Four traps, each of which produced a confident wrong answer today:

1. **A long-lived `beckon.exe` is not a hung one-shot.** Check `CommandLine`,
   not `Path`: `beckon.exe serve …` is a daemon, started by a 5-minute
   watchdog. Killing one because it had been alive for hours cost a14 ten
   minutes of hotkeys.
2. **After a scoop update, `--version` and the junction both lie.** They spawn
   or describe a *new* process. The watchdog races the update and a serve
   started four seconds before the relink runs the old image for hours.
   Compare the running process's `StartTime` to the install directory's
   `CreationTime`.
3. **SSH lands in session 0**, whose desktop cannot see session 1's windows at
   all — `EnumWindows` returns nothing and it reads as "no window is open".
   Every GUI probe goes through a scheduled task with `-LogonType Interactive`
   and `New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -Priority 4`
   (both flags), writing to a file you read back.
4. **Set DPI awareness before measuring anything.**
   `SetProcessDpiAwarenessContext((IntPtr)(-4))`. Without it Windows hands the
   process virtualized 96-DPI-equivalent numbers while `CopyFromScreen` uses
   physical pixels — the capture lands on the desktop behind the window, and
   every rect logged is a scaled-down lie that looks exactly like a 96-DPI
   reading.

Quoting through `ssh` → PowerShell → `powershell.exe` is eaten. Put scripts in
files under `C:\Users\kln\hwpass\` and run them with `-File`.

**Screenshotting the settings window without a person there:** find the window
of class `beckon-serve-tray`, then
`PostMessageW(tray, 0x8001, (IntPtr)1, (IntPtr)0x0203)` — `WM_APP+1` and
`WM_LBUTTONDBLCLK`, exactly what `Shell_NotifyIcon` posts on a double-click.
Poll for class `BeckonSettingsWindow`, `SetForegroundWindow`, wait ~2 s, then
`Graphics.CopyFromScreen`. Use `powershell.exe` (5.1) — `System.Drawing` is in
the box there.

## Two ListView custom-draw traps, both measured

Relevant to the chips because the same painter and the same instincts apply:

- **A paint handler must not borrow the `UI` `RefCell`.** A paint reaches this
  window while `UI` is already borrowed; every subitem notification exited at
  `try_borrow` and the Shortcut column silently drew as text. `borrow()` there
  would have aborted the process. Take the control handle from
  `hdr.hwndFrom`, the row's content from the control (`LVM_GETITEMTEXTW`), and
  fonts from a `thread_local Cell` refreshed wherever `build_fonts` runs.
- **`nmcd.uItemState` reports `CDIS_SELECTED` for every row** at the
  `CDDS_SUBITEM` stage. With nothing selected the whole column painted
  `COLOR_HIGHLIGHT`. Ask `LVM_GETITEMSTATE`.

Debugging shape that worked: a **bounded** `eprintln!` (first ~24 calls) at
every exit point, with `beckon-serve --log <path>`, then read the log. A paint
path runs per row per frame; an unbounded trace fills the file faster than it
can be read.

## The lesson this stretch paid for three times

Three times a verification was reported that had measured a *proxy* rather
than the thing: `Path` instead of `CommandLine`, `--version` instead of the
running image, virtualized coordinates instead of real DPI. Each looked clean
and each was wrong in the same direction — the check confirmed something
adjacent to the claim.

The habit that caught the fourth one: before trusting a green test, break the
thing it tests and watch it go red. Reverting `mods()` to ignore the live set
turned four of five §F.4 tests red, which is the only reason they are known to
be load-bearing rather than decorative.
