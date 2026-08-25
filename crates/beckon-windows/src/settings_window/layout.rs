//! Where every control goes. Behaviour is unchanged by the split that created
//! this file; see the module header in `mod.rs` for the rules `layout` obeys,
//! and note the one that matters most: `layout` calls `SetWindowPos` on the
//! populated App combo, which is the measured data-loss path. Nothing may add
//! a new call site for it on a keystroke path.
//!
//! That call is now made only when the combo is not already where this pass
//! would put it (`combo_needs_placing`), and when it IS made it goes through
//! `place_app_combo`, which saves the edit's text and selection across it --
//! so "nothing may add a new call site" has two readings it did not have
//! before. An unguarded `place_h(ui.app, ..)` would not merely be one more
//! call site: it would reinstate the unconditional call this file stopped
//! making, AND it would drop the restore that makes the remaining, genuinely
//! needed placements safe.
//!
//! Task 8 turns the flat bands into cards. `compute_card_rects` is the ONE
//! place that decides where the four cards sit and how tall each is; `layout`
//! places every control `tok::CARD_PAD` inside whichever card rect it belongs
//! to, and `card_rects` (below) hands the same four rects to `WM_PAINT` so it
//! can paint the card backgrounds. Two copies of that arithmetic would drift,
//! and the drift would look like a rendering bug rather than a duplication
//! one -- which is why there is exactly one.

use super::*;
use beckon_core::page_plan::{keyboard_plan, AboutPlan, RowMetrics, SystemPlan};

/// Layout tokens, at 96 DPI. Every one of them goes through `scale`.
///
/// Two need their reasoning, because they look like they contradict the
/// a14 measurements (`docs/superpowers/measurements/2026-08-11-landing-1-a14.md`)
/// and do not:
///
/// - **`CTL` is 26, not the measured 22.** `BCM_GETIDEALSIZE` returns the
///   smallest box the theme can draw a caption in — a floor, not a layout
///   recommendation — so clearing it is the whole test, and 26 clears it by
///   4 logical px. That is also the height a14 actually ran: its buttons
///   were laid out at `s(26)` and measured back at 39 physical against the
///   theme's 33-physical ideal (§"The BUTTON row is worth an eyebrow"),
///   the same four logical px of slack.
///
///   **CORRECTED 2026-08-14: this read "`CTL` is 32" and closed on "the
///   measurement's job was to prove 32 does not clip, and it does not".**
///   The compaction pass took `CTL` from 32 to 26 (`1f46335`; the record is
///   the token list beside `WINDOW_HEIGHT` in `mod.rs`), and a run that
///   clears the floor at 32 says nothing about 26. What carries the shipped
///   value is the floor argument above, not that run. Note what neither
///   covers: an ideal size is font-derived, the a14 run predates Task 8's
///   font stack, and no button on this branch has been measured on
///   hardware.
/// - **The list row is asked for, not tabulated.** 29 px measured at 144
///   DPI is 19.33 at 96, and a non-integer is the tell that comctl32
///   derives the row height from the font at the live DPI. A 96-DPI token
///   pushed through `scale` would be wrong at every non-integer scale and
///   would break again the moment the font changes, so `list_row_height`
///   (`mod.rs`) asks the control with `LVM_GETITEMRECT`.
///
///   **CORRECTED 2026-08-14: this read "There is no list-row token".**
///   There is one — `tok::ROW_H`, below in this module — and since Task 10
///   it is not spare: `rebuild_state_image_list` feeds `s(tok::ROW_H)` to
///   `ImageList_Create`, and a ListView takes its row height from its image
///   list, so the token forces the live row to be at least that tall.
///   `list_row_height` falls back to it only when the list is empty and
///   there is no row to measure. The reasoning is what survives the
///   correction: the token sets a lower bound, comctl32 is still free to
///   pad above it, and the control is the only thing that knows.
///
/// **`BAND` (14) is gone, replaced by `GAP_CARD` (8).** Every band-to-band
/// gap Task 8 leaves as a gap (banner-to-card, card-to-card) now separates
/// two cards that already carry their own `CARD_PAD` margin, so the gap
/// between them wants to be a little tighter than the old gap between bare
/// bands. `GAP` (6), the gap between two controls *inside* one band, is a
/// different relationship, and Task 8 did not touch it.
///
/// **CORRECTED 2026-08-14:** that paragraph read `GAP_CARD` (12) and `GAP`
/// (8) — both pre-compaction values — and called `GAP` "unchanged". Task 8
/// leaving `GAP` alone is still true and is all that sentence was ever
/// entitled to claim; the standing "unchanged" was not, because `1f46335`
/// later took `GAP` to 6 and `GAP_CARD` to 8. The `BAND` (14) half is
/// history and stays: that token really did exist at 14 and really is gone.
pub(super) mod tok {
    /// Surface padding — the margin between the client rect and the first
    /// card, and the last card and the client rect. Also the command bar's
    /// own margin, since it is not a card.
    pub const PAD: i32 = 10;
    /// Between the banner and card 1, and between two cards. Was `BAND`
    /// (14) before Task 8; see this module's own doc comment for why a
    /// smaller number is right once every band carries `CARD_PAD` of its
    /// own.
    pub const GAP_CARD: i32 = 8;
    /// Between two controls inside one band.
    pub const GAP: i32 = 6;
    /// Between two SETTING ROWS on the System and About pages -- the rhythm
    /// `beckon_core::page_plan` stacks with.
    ///
    /// **New 2026-08-15, and it is one of the two halves of the void those
    /// two doors carried.** Those rows sat one `GAP` apart, so the pitch was
    /// `CTL + GAP` = 32 against the mock-up's 46 -- and the mock-up is the
    /// authority on what is on screen. Measured in Chrome at the drawn 680 px:
    /// `.srow` is 46 px and the System card is 364, where the shipped card was
    /// 254. That 110 px is half the 224 px of ground under it; the other half
    /// is `WINDOW_HEIGHT`, which fell in the same pass.
    ///
    /// **This is not a regrid of `CTL` / `ROW_H` / `CARD_PAD`**, which design
    /// §10 puts out of scope and which would move the list's tick cell through
    /// `ImageList_Create`. It is the space BETWEEN rows on two pages, owned by
    /// the two plan functions and read nowhere else.
    pub const ROW_GAP: i32 = 20;
    /// Above and below a divider hairline on those same two pages.
    ///
    /// Half `ROW_GAP`, so a group boundary is `10 + 1 + 10` = 21 against a
    /// 20 px row gap: parted by the LINE, and by one pixel more of air than
    /// the rows already have. A full `ROW_GAP` each side would part them
    /// twice and put a hole at every group.
    pub const DIV_GAP: i32 = ROW_GAP / 2;
    /// A label and the control it names.
    pub const LABEL: i32 = 10;
    /// Height of one band line, and of every button on it.
    pub const CTL: i32 = 26;
    /// A button is never narrower than this, nor than its own caption.
    pub const BTN: i32 = 88;
    /// The right-aligned `Shortcut` column, the editor field under it, and
    /// the key list's ceiling.
    ///
    /// **A ceiling in four places, and at 680 px three of them bind.**
    /// Re-derived at 96 DPI when `WINDOW_WIDTH` moved 760 -> 680, because "a
    /// ceiling" and "the width that is actually used" are different claims and
    /// only the first is a property of this token:
    ///
    /// - `filter_w` -- other term `cw1 / 3` = **212**. Binds, and does so
    ///   down to a card interior of 600, i.e. a window of 642.
    /// - `col_shortcut` -- other term `inner / 2` = **310**. Binds.
    /// - `key_w` (the editor's key list) -- **binds again as of 2026-08-15**,
    ///   and the recovery needs no font either. This entry read "stopped
    ///   binding at 680", correctly: the run was `220 - lw_lbl`, so any label
    ///   column wider than 20 px put it under the ceiling and `"Shortcut"` is
    ///   wider than that at every DPI (`lw_lbl` is that caption plus `s(4)`,
    ///   so the threshold on the caption itself is 16 px -- which is what the
    ///   version of this entry before the four-doors rewrite said, derived
    ///   from the other end). Design §3.1 deleted the label column
    ///   (and the editor card's `gap` inset with it), which makes the run
    ///   `450 - the four chips` -- 242 at the chips' `CHIP_MIN` floor, so the
    ///   ceiling binds with 42 px to spare. `layout`'s `key_w` line carries
    ///   the full derivation, including the correction: `212 - lw_lbl` and
    ///   "12 px" were transcribed here and are wrong by 8 px.
    /// - `tap_w` (the Caps line's `Tap` list) -- does not bind at 680: its run
    ///   is `653 - kx`, and `kx` is 279 px of fixed terms (the three `Hold`
    ///   chips at their `CHIP_MIN` floor, the toggle's own `s(50)` budget, and
    ///   six gaps) plus three measured captions, of which
    ///   `"Use Caps Lock as a shortcut key"` is by far the largest and which
    ///   no trace puts under 200 px in Body at 96 DPI. **Whether it EVER bound
    ///   is unmeasured and
    ///   the two available traces disagree**: `layout`'s own hand measurement
    ///   of that line (about 547 px without the `Tap` box, and flagged there
    ///   as pre-compaction and over-stated) leaves 159 px at 760, already
    ///   under the ceiling, while a per-character trace of the same string
    ///   puts it at 213 and just over. Gate G1 is the run that settles it.
    ///   Nothing depends on the answer -- both figures are below the ceiling
    ///   at the width that shipped.
    pub const SHORTCUT_COL: i32 = 200;
    /// A modifier chip is never narrower than this, nor than its own caption
    /// plus `glyph` -- direction B's `.wtog { min-width:46px }`.
    pub const CHIP_MIN: i32 = 46;
    // **`ROWS` (8) is gone, 2026-08-15** -- design 4, "the list is short and
    // scrolls". It was the list's CAP: `want` was `list_header_height(..) +
    // row_h * ROWS` and `list_h` was `want.min(room)`, so the list never grew
    // past eight rows however tall the window was dragged.
    //
    // It had to go in the same pass as the three deletions above it, not a
    // landing later. Removing the column header, the editor caption and the
    // field labels hands the list 110 px at 96 DPI, and with the cap still in
    // place all 110 would have arrived as EMPTY SPACE below the editor card --
    // the same void this pass exists to close, moved down the window rather
    // than removed. `compute_card_rects` now takes the room it has and snaps
    // it down to whole rows.
    //
    // `ROW_H` below is NOT its replacement and survives on its own reader
    // (`rebuild_state_image_list`); the snap reads `list_row_height`, which
    // asks the control.
    /// Widest a tooltip may draw before it wraps. Comfortably narrower than
    /// `MIN_WIDTH`, so the balloon never overhangs the window that owns it.
    pub const TOOLTIP_MAX: i32 = 420;
    /// Inner padding inside every card, on all four sides.
    pub const CARD_PAD: i32 = 11;
    /// `RoundRect`'s corner radius, halved (the API wants the full ellipse
    /// width/height, i.e. `2 * CARD_RADIUS`).
    pub const CARD_RADIUS: i32 = 10;
    /// The list's row height, fed to `ImageList_Create` for the state image
    /// list a row's height is actually derived from -- see
    /// `rebuild_state_image_list` in `mod.rs` (Task 10), the lever this
    /// token exists for. It is also `list_row_height`'s fallback while the
    /// list is empty, and therefore a LOWER BOUND on the live row rather than
    /// the live row (`Ui::shown_empty` is the guard that exists for the
    /// difference).
    ///
    /// It arrived beside `tok::ROWS`, which sized the list's card at eight of
    /// these; `ROWS` went on 2026-08-15 and this one did not, because the two
    /// answered different questions -- how tall a row is, and how many of them
    /// to show.
    pub const ROW_H: i32 = 22;

    /// A tab pill's drawn height.
    pub const TAB_VISUAL: i32 = 26;
    /// The trough's inner padding above and below the pill row.
    pub const TAB_PAD_Y: i32 = 2;
    /// A pill's margin inside the trough. The perceived gap between two
    /// pills is `2 * FOCUS_SLACK` = 6, which is `tok::GAP` -- the pills
    /// touch, and the space between them is their own margin. `TAB_GAP`
    /// would therefore be 0, which is why it is not a token.
    pub const FOCUS_SLACK: i32 = 3;
    /// The tab strip's trough.
    ///
    /// **Not an independent number**, and written as the sum rather than as
    /// the 36 the spec tabulates so that it cannot quietly stop being the
    /// sum: `TAB_VISUAL 26 + 2*TAB_PAD_Y 2 + 2*FOCUS_SLACK 3 = 36`. Move any
    /// of the three and this follows on its own rather than by anyone
    /// remembering to.
    ///
    /// It also earns the three their keep. `tok` is `pub` inside a PRIVATE
    /// module, so a token nothing reads is a hard `dead_code` error under the
    /// gate's `-D warnings` -- measured, by deleting the `allow` below and
    /// watching the build fail. Spelling this as the sum is what makes
    /// `TAB_VISUAL`, `TAB_PAD_Y` and `FOCUS_SLACK` live in the same commit
    /// that introduces them, before anything paints a pill.
    pub const TABSTRIP_H: i32 = TAB_VISUAL + 2 * TAB_PAD_Y + 2 * FOCUS_SLACK;
    /// A pill's inner padding, left and right.
    ///
    /// It carried an `#[allow(dead_code)]` for exactly one commit -- the band
    /// landed empty, and this was the one token of the five with no reader.
    /// The pills spend it now (`layout`'s band 0), so the `allow` is gone
    /// rather than left blanketing a token that has a reader again.
    pub const TAB_PAD_X: i32 = 14;
}

/// Everything `layout` needs out of `Ui`, copied in ONE borrow that is
/// dropped before a single `SendMessageW` or `SetWindowPos` runs.
///
/// This is not tidiness. A second `RefCell` borrow taken across an
/// `extern "system"` boundary — and every one of those calls can re-enter
/// this window's wndproc — ABORTS the process rather than unwinding, so it
/// shows up as neither a panic nor a test failure nor anything a
/// cross-compile can catch. Copying the handles out first makes it
/// unrepresentable.
#[derive(Clone, Copy)]
pub(super) struct LayoutHandles {
    list: HWND,
    combo: HWND,
    app: HWND,
    notes: HWND,
    filter: HWND,
    banner: HWND,
    reload: HWND,
    keep: HWND,
    pub(super) fonts: Fonts,
    external_change: bool,
    /// Which door is open, and therefore which controls this pass may place
    /// at all.
    ///
    /// **Read from the `PAGE` thread-local, NOT from a field of `Ui`**, and
    /// that is the point of `PAGE` being a `Cell`. `compute_card_rects` is
    /// documented never to touch `UI`, so every caller can hold its own copy
    /// and none of them can be the second `RefCell` borrow that aborts the
    /// process across an `extern "system"` boundary. A `Ui::page` would work
    /// and would quietly lose that property -- `card_rects`, called from
    /// `WM_PAINT`, would then need `UI` alive for a fact that has nothing to
    /// do with the window's contents.
    page: Page,
}

impl LayoutHandles {
    fn of(ui: &Ui) -> Self {
        Self {
            list: ui.list,
            combo: ui.combo,
            app: ui.app,
            notes: ui.notes,
            filter: ui.filter,
            banner: ui.banner,
            reload: ui.reload,
            keep: ui.keep,
            fonts: ui.fonts,
            external_change: ui.external_change,
            // Safe to read while the caller's `UI` borrow is alive: a
            // different thread-local, and a `Cell` takes no borrow at all.
            page: PAGE.with(|p| p.get()),
        }
    }
}

/// The System and About cards' row rhythm, at ONE DPI, already scaled.
///
/// **Both plans moved to `beckon_core::page_plan` on 2026-08-15**, and these
/// three functions are the seam. They were pure integer arithmetic sitting in
/// a `cfg(windows)` module with **no tests in it at all** -- so the whole
/// vertical geometry of two doors was unrunnable on the machine it is written
/// on and invisible to two of the three CI jobs, which is why 224 px of ground
/// under the System card reached a photograph before it reached a failure.
/// Design §12 q3 makes exactly this argument for `Page`.
///
/// **The TOKENS stay here.** Core is handed lengths and never names one, so
/// there is no second copy of `CTL` to drift from this one, and
/// `the_row_rhythm_is_the_one_core_stacks_with` is what ties the two ends
/// together.
fn row_metrics(dpi: u32) -> RowMetrics {
    let s = |v: i32| v * dpi as i32 / 96;
    RowMetrics {
        ctl: s(tok::CTL),
        row_gap: s(tok::ROW_GAP),
        div_gap: s(tok::DIV_GAP),
        mark: s(paint::MARK_D),
        // The name line is `s(24)`, the same budget the Keyboard card's
        // caption takes for its own single line of Subtitle-adjacent text.
        // `ctl` would be a control's height, and this is not a control.
        name: s(24),
    }
}

/// Walk the nine slots in drawing order, skipping the two conditional ones.
///
/// The order IS the mock-up's, top to bottom: the three service rows, a
/// divider, the two look rows, a divider, the two file rows. Design §3.3
/// calls it five rows; nine slots is the same page counted by control line
/// rather than by group, and the two dividers are what turn nine lines into
/// three groups without a heading on any of them -- design §7 rule 5, read
/// backwards: a group whose rows share a store does not need a word saying so.
fn system_plan(dpi: u32, rows: SystemRows) -> SystemPlan {
    beckon_core::page_plan::system_plan(row_metrics(dpi), rows)
}

/// `disclosure_h` is measured by `disclosure_height` and passed in, so the
/// arithmetic stays pure and the one measurement has one call site per pass.
fn about_plan(dpi: u32, disclosure_h: i32) -> AboutPlan {
    beckon_core::page_plan::about_plan(row_metrics(dpi), disclosure_h)
}

/// How tall the wrapped disclosure needs to be at `width`.
///
/// **`DT_CALCRECT` with `paint::DISCLOSURE_FLAGS`, the identical flag set the
/// painter draws with.** A fixed line budget was the alternative --
/// `notes_height`'s shape, and it is what `IDC_NOTES` does -- and it loses
/// here: that string is `beckon_core::settings::HOOK_DISCLOSURE`, roughly 150
/// characters, so it takes two lines in the shipped card and three in a
/// narrower one or a larger face. A budget short by one line clips the half of
/// the sentence that is the feature ("beckon keeps no record of what you
/// type"), and a budget long by one leaves a hole under the last line on every
/// machine.
///
/// **The width it is given is the width the painter gets**, dot column
/// subtracted, which is what makes the measurement answer the same question
/// the paint asks. Both terms come from `paint`'s own constants.
///
/// Falls back to three lines of the font's own height when the DC cannot be
/// had -- generous in the safe direction, exactly like `text_size`'s estimate.
unsafe fn disclosure_height(hwnd: HWND, ui: &LayoutHandles, dpi: u32, width: i32) -> i32 {
    let s = |v: i32| v * dpi as i32 / 96;
    let font = ui.fonts.get(Role::Caption);
    let line = text_size(hwnd, font, dpi, "Ag").1;
    let est = line * 3;
    let dc = GetDC(Some(hwnd));
    if dc.is_invalid() {
        return est;
    }
    let prev = SelectObject(dc, HGDIOBJ(font.0));
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: width.max(1),
        bottom: 0,
    };
    let mut t = wide(beckon_core::settings::HOOK_DISCLOSURE);
    let n = t.len() - 1;
    let h = DrawTextW(
        dc,
        &mut t[..n],
        &mut rc,
        paint::DISCLOSURE_FLAGS | DT_CALCRECT,
    );
    if !prev.is_invalid() {
        SelectObject(dc, prev);
    }
    ReleaseDC(Some(hwnd), dc);
    // `DT_CALCRECT` returns the height it computed and also writes it into
    // the rect; the return is used because it is the documented one. A zero
    // or negative answer means the call did not do what it says, so the
    // estimate stands rather than a control of no height.
    if h > 0 { h.max(line) } else { est }.max(s(1))
}

/// The width `disclosure_height` measures against and `paint::disclosure`
/// draws into: the card's interior less the dot column.
///
/// One function so the measure and the paint cannot disagree about the dot's
/// width -- which they would the first time either side was edited, and the
/// symptom would be a paragraph one line taller or shorter than its box.
fn disclosure_text_w(card_inner_w: i32, dpi: u32) -> i32 {
    let s = |v: i32| v * dpi as i32 / 96;
    (card_inner_w - s(paint::NOTE_DOT_D) - s(paint::NOTE_TEXT_GAP)).max(1)
}

/// The two divider hairlines inside the System card, in client coordinates.
///
/// `WM_PAINT`'s only window onto `system_plan`. Returns a zero-width rect for
/// each divider when the System door is not open, which `paint::divider`
/// declines to draw -- the same "a degenerate rect is nothing worth asking
/// GDI to draw" rule the card loop already applies.
pub(super) unsafe fn system_dividers(hwnd: HWND) -> [RECT; 2] {
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return [RECT::default(); 2];
    };
    if ui.page != Page::System {
        return [RECT::default(); 2];
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = |v: i32| v * dpi as i32 / 96;
    let card = compute_card_rects(hwnd, &ui, dpi)[4];
    if card.bottom <= card.top {
        return [RECT::default(); 2];
    }
    let pad = s(tok::CARD_PAD);
    let plan = system_plan(dpi, sys_rows());
    let line = |dy: i32| RECT {
        left: card.left + pad,
        top: card.top + pad + dy,
        right: card.right - pad,
        bottom: card.top + pad + dy + s(1).max(1),
    };
    [line(plan.div1), line(plan.div2)]
}

/// The two divider hairlines inside the Keyboard card, in client coordinates.
///
/// `system_dividers`' third twin, separate for that function's own stated
/// reason: each page computes its offsets from its own plan, so one entry
/// point taking a `page` would be an `if` wrapped around three unrelated
/// arithmetics. Card 3, not card 4.
pub(super) unsafe fn keyboard_dividers(hwnd: HWND) -> [RECT; 2] {
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return [RECT::default(); 2];
    };
    if ui.page != Page::Keyboard {
        return [RECT::default(); 2];
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = |v: i32| v * dpi as i32 / 96;
    let card = compute_card_rects(hwnd, &ui, dpi)[3];
    if card.bottom <= card.top {
        return [RECT::default(); 2];
    }
    let pad = s(tok::CARD_PAD);
    let plan = keyboard_plan(row_metrics(dpi));
    let line = |dy: i32| RECT {
        left: card.left + pad,
        top: card.top + pad + dy,
        right: card.right - pad,
        bottom: card.top + pad + dy + s(1).max(1),
    };
    [line(plan.div1), line(plan.div2)]
}

/// The two divider hairlines inside the About card, in client coordinates.
///
/// `system_dividers`' twin, and a SEPARATE function rather than a `page`
/// argument on that one: the two pages compute their offsets from different
/// plans, so a shared entry point would be an `if` on the page wrapped around
/// two unrelated arithmetics. `WM_PAINT` chains both, and each answers with
/// zero-width rects behind any door but its own -- which `paint::divider`
/// declines to draw, the same degenerate-rect rule the card loop applies.
pub(super) unsafe fn about_dividers(hwnd: HWND) -> [RECT; 2] {
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return [RECT::default(); 2];
    };
    if ui.page != Page::About {
        return [RECT::default(); 2];
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = |v: i32| v * dpi as i32 / 96;
    let card = compute_card_rects(hwnd, &ui, dpi)[5];
    if card.bottom <= card.top {
        return [RECT::default(); 2];
    }
    let pad = s(tok::CARD_PAD);
    let inner_w = (card.right - card.left - pad * 2).max(0);
    let plan = about_plan(
        dpi,
        disclosure_height(hwnd, &ui, dpi, disclosure_text_w(inner_w, dpi)),
    );
    let line = |dy: i32| RECT {
        left: card.left + pad,
        top: card.top + pad + dy,
        right: card.right - pad,
        bottom: card.top + pad + dy + s(1).max(1),
    };
    [line(plan.div1), line(plan.div2)]
}

/// The tab strip's trough, in client coordinates.
///
/// Separate from `compute_card_rects` because it is not a card and the
/// `WM_PAINT` card loop must not draw it -- but it is the SOURCE of the
/// strip's height, and `compute_card_rects` calls it rather than repeating
/// `s(tok::TABSTRIP_H)`. Two copies of that arithmetic would drift, and the
/// drift would look like a rendering bug rather than a duplication one --
/// the same rule `compute_card_rects` states for itself below.
///
/// The left and right edges are computed the way `compute_card_rects`
/// computes `cx` and `cw`, from the same `tok::PAD`, so the trough and every
/// card share two numbers rather than two arithmetics that agree today.
///
/// **That inset is load-bearing beyond looks.** `chrome::nchittest` resolves
/// all eight resize directions itself -- there is no non-client border left
/// for `DefWindowProc` to find one in -- and a child window gets its own
/// `WM_NCHITTEST`, so the parent's is only consulted for points no child
/// covers. A pill reaching the client edge would therefore kill the left and
/// right resize edge across this whole band. The strip Windows treats as
/// grabbable is `SM_CYSIZEFRAME + SM_CXPADDEDBORDER` wide (that pair, read
/// back from `chrome::nchittest`'s own `border`; note it spends the *Y*
/// size-frame metric on both axes, because there is no `SM_CYPADDEDBORDER`
/// to pair an X one with) -- roughly 8 px at 96 DPI against a `PAD` of 10,
/// and roughly 12 at 144 against a `PAD` of 15. A margin of 2-3 px, which is
/// why gate G-S5 prints those metrics by name rather than assuming them --
/// `examples/settings_probe.rs` prints all three, since Task 8.
///
/// **That margin does not move with the window's WIDTH**, which Task 8 had to
/// check rather than assume when `WINDOW_WIDTH` went 760 -> 680. Both edges
/// here are one `pad` in from the client rect, at every width, so the margin
/// stays 2 px at 96 DPI and 3 px at 144 whatever the window is dragged to.
/// Only the LEFT edge is anywhere near being spent: the pill run starts at
/// `strip.left` and, at four one-word captions, ends nowhere near the trough's
/// right edge at any width this window has -- and the trough's own FILL is
/// paint, not a window, so it takes no hit-test and cannot cover an edge that
/// no pill covers.
///
/// The one subtraction is clamped, for the reason `compute_card_rects`
/// gives below: `WM_SIZE` fires with a 0x0 client rect on minimize.
pub(super) fn strip_rect(rc: RECT, dpi: u32) -> RECT {
    let s = |v: i32| v * dpi as i32 / 96;
    let pad = s(tok::PAD);
    // No `pad` above it: the surface padding that used to sit between the
    // title bar and the first card is what the strip is spent on. See
    // `compute_card_rects`' `y`.
    let top = s(chrome::TITLEBAR_H);
    RECT {
        left: pad,
        top,
        right: pad + (rc.right - rc.left - pad * 2).max(0),
        bottom: top + s(tok::TABSTRIP_H),
    }
}

/// The four card rects: the external-change banner, the Shortcuts card (head
/// row plus the list), the editor card, the keyboard card.
///
/// **Cards 1-2 and card 3 belong to different pages, so they are alternatives
/// rather than a stack of four.** Cards 0, 1 and 2 stack top to bottom on the
/// Shortcuts page; card 3 sits alone at the same origin card 1 would take, on
/// the Keyboard page. A card behind a closed door gets zero height, so the
/// array is always four long and never four tall.
///
/// **The ONE arithmetic `layout` and `card_rects` both run.** `layout`
/// places every control `tok::CARD_PAD` inside whichever of these four
/// rects it belongs to; `card_rects` (below) hands the same four rects to
/// `WM_PAINT` so it can paint the card backgrounds. Two copies of this
/// arithmetic would drift, and the drift would look like a rendering bug
/// rather than a duplication one.
///
/// **Takes `ui: &LayoutHandles`, never touches `UI` itself.** Every caller
/// already holds its own copy — `layout`'s single borrow, or `card_rects`'
/// own — so this function is free of the `RefCell` entirely and cannot be
/// the second borrow that aborts the process.
///
/// **The banner's rect has zero height when the banner is hidden**, at
/// whatever `y` the rest of the stack would have started at anyway — the
/// same "contributes no height" rule the flat band had. A caller that draws
/// or places into a zero-height rect must check for that itself; this
/// function does not skip it, because `layout` needs to know the banner's
/// *position* even when it has no height (to decide where card 1 starts),
/// while `card_rects`' caller only needs to know not to paint it.
///
/// **A card behind a closed door gets the same zero height**, through the
/// same rule rather than a second one. `WM_PAINT`'s card loop already skips
/// a degenerate rect, so a card whose page is not showing is not painted --
/// and it has to be, because an EMPTY card is not "nothing drawn", it is a
/// rounded, bordered rectangle with nothing in it, which reads as a page
/// that failed to load rather than as a page that is not on screen.
///
/// **The stack is PAGE-DEPENDENT since 2026-08-15: a page reserves only what
/// it draws.** Every card starts from the same content origin (the strip's
/// bottom, past the banner if it is up) and the page decides which ones follow
/// it; the command bar is the only thing anchored to the bottom edge, and
/// `content_bottom` -- one `gap_card` above it -- is where the cards must stop.
/// So the Shortcuts page is banner / card 1 / card 2 down to `content_bottom`,
/// and the Keyboard page is card 3 alone at the origin.
///
/// **REVERSED, Task 7 having weighed the same change and deferred it twice.**
/// The keyboard card used to be bottom-anchored and to reserve
/// `gap_card + kb_card_h` = 86 px at 96 DPI **on every page**, so the Shortcuts
/// page carried a card-shaped hole above the command bar and the Keyboard page
/// carried a larger one below the strip. Both are visible in
/// `docs/superpowers/measurements/2026-08-14-four-doors-shell-a14-dark.png`,
/// and the first is the largest single difference between that photograph and
/// the mock-up.
///
/// Task 7's two reasons for deferring were good and are both spent:
///
/// - *"The re-stack changes the Shortcuts page's vertical geometry, which is
///   another workstream's open subject -- design §4 uncaps the list, design
///   §3.1 deletes the editor caption, so doing it now means deriving the table
///   twice."* Both of those landed in the SAME pass as this one, which is
///   precisely the condition Task 7 was waiting for. The table is derived once,
///   after all four changes.
/// - *"Nothing on the host can display the window; every vertical figure is a
///   hand trace corrected twice already."* Still true, and still the reason
///   every number below is spelled out as arithmetic rather than asserted. It
///   argues for deriving carefully, not for deferring again -- the cost of a
///   pass is the same whenever it is taken.
///
/// **What the re-stack gives back, derived rather than asserted.** With the
/// 86 px reservation gone and the editor caption's `s(24)` gone with it, the
/// list's room goes from `h - 442 - notes_h` to `h - 332 - notes_h` with the
/// banner up (110 px), and from `h - 386 - notes_h` to `h - 276 - notes_h` with
/// it down. At the shipped client height of 600 with `notes_h` 36 that is 288
/// px where it was 178 -- and since `tok::ROWS` went with the same pass, all of
/// it reaches the list instead of stopping at eight rows. See `MIN_HEIGHT` for
/// the full table.
///
/// **CORRECTED 2026-08-15: About has a card too.** This read "**About needed
/// none of this and is unchanged.** A page whose entire content is one line
/// has no stack: `layout` puts the waiting line at the content origin ... and
/// the emptiness below it is the page being empty rather than the line being
/// misplaced." That was true of a page holding one waiting line and stopped
/// being true the day design §3.4's fifteen controls replaced it. The
/// paragraph's own reasoning is what survives, one door across: the Keyboard,
/// System and About pages each put ONE card at the content origin with space
/// below it, which is what a page with one thing on it looks like, while one
/// card at the bottom and space above it is a page that failed to lay out.
///
/// **System grew card 4 on 2026-08-15** (design §3.3) **and About grew card 5
/// the same day** (design §3.4); both follow card 3's shape exactly -- one
/// card at the content origin, its height fixed by its own contents rather
/// than by the window's. The array is six long and still never six tall:
/// Shortcuts stacks 0/1/2, Keyboard draws 3 alone, System draws 4 alone,
/// About draws 5 alone.
unsafe fn compute_card_rects(hwnd: HWND, ui: &LayoutHandles, dpi: u32) -> [RECT; 6] {
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return [RECT::default(); 6];
    }
    let s = |v: i32| v * dpi as i32 / 96;
    // See `layout`'s own comment on `clamp` for the widths-vs-positions
    // rule this closure exists to enforce. Every card here spans the same
    // `[cx, cx+cw]`, so nothing in this function subtracts from a right
    // edge and `.max(cx)` never comes up.
    let clamp = |v: i32| v.max(0);

    let pad = s(tok::PAD);
    let gap_card = s(tok::GAP_CARD);
    let gap = s(tok::GAP);
    let ctl = s(tok::CTL);
    let card_pad = s(tok::CARD_PAD);

    // Independent of WM_GETMINMAXINFO, for the same reason `layout` computes
    // its own `w`/`h` fresh: WM_SIZE fires with a 0x0 client rect on
    // minimize, and every subtraction below goes negative without `clamp`.
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let cx = pad;
    let cw = clamp(w - pad * 2);
    let card = |top: i32, height: i32| RECT {
        left: cx,
        top,
        right: cx + cw,
        bottom: top + height,
    };
    // Which doors own the six cards. Cards 0-2 are the Shortcuts page, card 3
    // is Keyboard, card 4 is System and card 5 is About -- **every door owns
    // at least one now**, where this comment used to record that the last two
    // owned none and were "not waiting for one".
    let shortcuts = ui.page == Page::Shortcuts;
    let keyboard = ui.page == Page::Keyboard;
    let system = ui.page == Page::System;
    let about = ui.page == Page::About;

    // The command bar is anchored, not stacked, so the window's bottom edge is
    // where it stays however tall the content above is. `content_bottom` is
    // the stop every stacked card shares: one `gap_card` above the bar, which
    // is the same gap two cards keep between themselves.
    //
    // **It replaced `kb_y - gap_card`, which was the stop when the keyboard
    // card was reserved on every page.** One expression, one meaning, on all
    // four doors -- which is what makes "a page reserves only what it draws"
    // true by construction rather than by four `if`s agreeing.
    let bar_y = clamp(h - pad - ctl);
    let content_bottom = clamp(bar_y - gap_card);

    // The keyboard card's CONTENT keeps `kb_h`'s exact pre-Task-8 shape --
    // caption inset, one control line, a bottom inset the size of `gap` --
    // because that shape was always the caption's own `s(24)` line plus one
    // control line plus a bottom inset, and none of those three numbers
    // changed when the review fix reclassed `IDC_GRP_KEYBOARD` from
    // `BS_GROUPBOX` to a plain caption `STATIC` (see its creation comment in
    // `build_children`) -- only the CONTROL drawing that first `s(24)`
    // changed, not its height. Only the card's own `CARD_PAD` wrapping
    // around it is new, same as before that reclass.
    // **Design §3.2's three groups, since 2026-08-16**, where this was
    // `s(24) + ctl + gap` -- a caption line, one control line and a bottom
    // inset. The caption was `IDC_GRP_KEYBOARD` reading `Keyboard` directly
    // beneath a pill captioned `Keyboard`; §3.1 deleted the same duplication on
    // the Shortcuts door and `measurements/fd-after-keyboard.png` is the
    // photograph of it surviving on this one. The card is 56 px of interior
    // taller as a result, which is design arriving rather than a regrid: the
    // rhythm is `keyboard_plan`'s, which is `system_plan`'s.
    let kb_card_h = card_pad * 2 + keyboard_plan(row_metrics(dpi)).content_h;

    // The editor card's content is `grp_h`'s pre-Task-8 shape MINUS its
    // caption: two lines, the notes, a bottom inset. Computed HERE, before the
    // banner and card 1, because card 1 has to yield to it below and the two
    // must not each hold an opinion about how tall the editor card is.
    //
    // **The leading `s(24)` went on 2026-08-15** with `IDC_GRP_EDITOR` (design
    // §3.1, "no `Editing "…"` caption on the editor card"). It was the caption
    // line's own height, unchanged across the Task 8 review's reclass from
    // `BS_GROUPBOX` to a plain `STATIC` because that changed which control drew
    // the line rather than how tall it was -- and now there is no line. 24 px
    // at 96 DPI, all of it to the list. `kb_content_h` above keeps its `s(24)`
    // because the Keyboard card keeps its caption.
    let notes_h = notes_height(hwnd, ui, dpi);
    let grp_content_h = ctl + gap + ctl + gap + notes_h + gap;
    let card2_h = card_pad * 2 + grp_content_h;

    // Offset by the client-drawn title bar (Task 7) and the tab strip's
    // trough below it. `GetClientRect` includes the title bar --
    // `nccalcsize` gave it back to the client -- so the first card has to
    // start below both bands rather than draw underneath them.
    //
    // **Read from `strip_rect`, not re-added from `tok::TABSTRIP_H`.** This
    // is the file's ONE "content starts below the bar" statement and it now
    // has to agree with a rect something else paints; deriving it from that
    // rect's own bottom edge is what makes disagreement unrepresentable
    // rather than merely unlikely.
    //
    // **The strip costs 34, not 36.** The surface `pad` that used to sit
    // above the first card is SPENT by the strip rather than added to it --
    // `strip_rect`'s top is `TITLEBAR_H` with no `pad` above it, and the
    // mockup's `.tabstrip{padding:0 10px 8px}` puts none there either. So
    // this line went from `pad 10 + TITLEBAR_H 34 = 44` to `TITLEBAR_H 34 +
    // TABSTRIP_H 36 + GAP_CARD 8 = 78`. Everything vertical below moves by
    // that 34 and nothing else in the module may add a second offset; the
    // term list beside `WINDOW_HEIGHT` and the derivation under `MIN_HEIGHT`
    // are both readings of this line and both were re-run against it.
    let mut y = strip_rect(rc, dpi).bottom + gap_card;

    // -- Card 0: the banner. Contributes NO height when hidden -- `y` is
    // not advanced and the returned rect has zero height at that `y`.
    //
    // `banner_shown`, not `ui.external_change`, even though the two agree
    // today: the condition is allowed to change in exactly one place, and Task
    // 6 changes it (back to the Shortcuts door, once its pill carries a warn
    // dot). Card 0 is the band that would then stop being spent on the other
    // three pages -- the one visible cost of the wide version -- so this is
    // the site with the most to gain from having no opinion of its own.
    let card0 = if banner_shown(ui.external_change, ui.page) {
        let h0 = card_pad * 2 + ctl;
        let r = card(y, h0);
        y += h0 + gap_card;
        r
    } else {
        card(y, 0)
    };

    // The content origin: where the FIRST card of whichever page is open
    // starts. Card 1 takes it on Shortcuts and card 3 takes it on Keyboard --
    // they are alternatives, never neighbours, so one name serves both and
    // neither can drift from the other.
    let content_top = y;

    // -- Card 1: the Shortcuts card, head row plus the list. The list is
    // the one thing that flexes -- it takes whatever the fixed cards below it
    // leave, and gives room up rather than let anything overlap when the
    // window is short: a shrunk list scrolls, an overlapped control is
    // unreachable. Card 2 is fixed, which is what makes card 1 the thing that
    // must yield.
    //
    // No `+ border` term any more (Task 10). That used to be
    // `2 * SM_CYBORDER`, the two pixels `WS_BORDER` drew OUTSIDE the list's
    // own client area -- height the control's window rect needed beyond its
    // content that a border-less control no longer spends. The list's
    // border is the card's now (`paint::card`), so there is nothing left
    // for that term to pay for.
    //
    // **No `want` term either, since 2026-08-15** (design §4, and see
    // `tok`'s own record of `ROWS` going). The list used to ask for
    // `list_header_height + ROWS * row_h` and take the SMALLER of that and the
    // room -- a cap, which at the shipped size would now leave 112 px of
    // nothing below the editor card. It takes the room instead.
    //
    // `.max(1)` on the row height is not defensive dressing: `list_row_height`
    // cannot return zero today (both arms are positive), but the snap below
    // DIVIDES by it, and a division by zero inside a wndproc is a panic across
    // an `extern "system"` boundary, which aborts the process rather than
    // unwinding -- the same failure mode a second `RefCell` borrow has, and
    // the same reason it is made unrepresentable rather than argued about.
    let row_h = list_row_height(ui.list, dpi).max(1);
    // Where the LIST ITSELF starts: past card 1's own top inset, the head
    // row and the control gap below it. The direct analogue of the
    // pre-Task-8 `y` this same computation read, which was already past
    // band 2's head row for the same reason -- a bare band's content sat
    // right at `y` with no inset of its own, which is what let the old
    // formula skip this step.
    let list_top = content_top + card_pad + ctl + gap;
    // `editor_min` is card 2's WHOLE footprint, `CARD_PAD` included -- not
    // just `grp_content_h` -- because that whole footprint is the room card
    // 2 actually needs to sit in below card 1. `room` runs from the list's
    // own top down to `content_bottom`, the stop every card shares. The two
    // subtractions after it are card 1's OWN bottom inset (`card_pad`) and the
    // `gap_card` between card 1 and card 2.
    //
    // Miss the `card_pad` and the guard below cannot save it: near the floor,
    // the list would be handed 11 px at 96 DPI more room than the card can
    // afford and card 2 would draw exactly that far past `content_bottom`,
    // i.e. into the command bar. Simulated, not seen: nothing on the machine
    // this was written on can display the window.
    let editor_min = card2_h;
    let room = clamp(content_bottom - list_top);
    let avail = clamp(room - gap_card - card_pad - editor_min);
    // **Snapped DOWN to whole rows**, which is the half of `tok::ROWS`' job
    // worth keeping. Two reasons, and the second is the load-bearing one:
    //
    // - A list whose last row is sliced horizontally reads as a rendering
    //   fault rather than as a scroll affordance; comctl32 clips, it does not
    //   scale.
    // - It keeps `row_h` an INPUT to this function, and therefore keeps
    //   `Ui::shown_empty` a live guard. Design §12 q2 puts it exactly that
    //   way: "keep the whole-row snap or delete the guard -- do not leave a
    //   guard that guards nothing." `list_row_height` falls back to
    //   `tok::ROW_H` while the list is empty, which is a LOWER BOUND on the
    //   real row, so the first row to arrive can change this answer and the
    //   layout has to be recomputed when it does. That is the whole of what
    //   `shown_empty` is for.
    //
    // The remainder -- at most `row_h - 1`, so 21 px at 96 DPI -- lands
    // between card 2's bottom and the command bar. It is a margin, not the
    // void this pass closed: that one was a fixed 86 px card reservation on a
    // page that drew no card.
    let list_h = avail - avail % row_h;
    let card1_h = card_pad * 2 + ctl + gap + list_h;
    // The height is computed either way and spent only on its own page, so
    // the arithmetic above has exactly one shape rather than one per door.
    let card1 = card(content_top, if shortcuts { card1_h } else { 0 });
    // `.min(content_bottom)`: bounds card 2's TOP, and only its top. `card2_h`
    // is fixed -- not something `clamp` shrinks the way it shrinks `list_h`
    // above -- so this line cannot pull card 2's BOTTOM back up; what keeps
    // the bottom clear at and above `MIN_HEIGHT` is `editor_min` reserving the
    // whole of `card2_h` before the list takes any height at all. Reachable in
    // the state where `room` itself clamped negative -- an intermediate resize
    // below `MIN_HEIGHT` that `WM_DPICHANGED`'s suggested rect can hand this
    // function without asking `WM_GETMINMAXINFO` first (dragging can't reach
    // it; a 0x0 client rect clamps everything to 0 and is fine).
    //
    // It was `.min(kb_y)`, which is `content_bottom - kb_card_h`: a tighter
    // bound, on a page that was reserving the keyboard card whether or not it
    // drew one. Both are only reachable below the floor.
    let card2_y = (content_top + card1_h + gap_card).min(content_bottom);
    let card2 = card(card2_y, if shortcuts { card2_h } else { 0 });

    // -- Card 3: the Keyboard page's only card, at the SAME content origin
    // card 1 takes on its own page. It was bottom-anchored above the command
    // bar until 2026-08-15 and reserved its height on every page; see this
    // function's doc comment for the re-stack and what it cost to defer.
    //
    // No clamp against `content_bottom`, and none is wanted. This card is a
    // FIXED 78 px at 96 DPI against a floor of 560, so it cannot reach the
    // command bar at any size `WM_GETMINMAXINFO` allows; below the floor -- the
    // `WM_DPICHANGED` path card 2's own `.min` covers -- clamping its top
    // downward would push it INTO the bar rather than away from it, which is
    // why the two cards are treated differently rather than uniformly.
    let card3 = card(content_top, if keyboard { kb_card_h } else { 0 });

    // -- Card 4: the System page's only card, at the same content origin,
    // and its height is its CONTENTS' rather than the window's -- so the page
    // is a card with space below it, which is what a page with one thing on
    // it looks like. Unlike card 1 nothing here flexes: every row is `ctl`
    // tall and the two conditional rows either take a row's height or none.
    //
    // No clamp against `content_bottom`, on card 3's reasoning: the tallest
    // this card gets is nine slots, which at 96 DPI is **304 px of interior**
    // and 326 with its own `CARD_PAD`, against a floor of 480 for the whole
    // window -- it needs 448 -- so it cannot reach the command bar at any size
    // `WM_GETMINMAXINFO` allows, and below the floor clamping its top downward
    // would push it INTO the bar rather than away from it.
    //
    // **The figure here said 262 and was wrong by 30 px before the rhythm
    // even changed.** Nine slots at the old `CTL + GAP` pitch is 232, not 262,
    // and no reading of the function produced 262 -- which is the whole reason
    // `system_plan` now lives in `beckon_core::page_plan` with
    // `the_system_card_interior_is_304_with_every_row` beside it. A comment
    // asserting a number no test could reach is how this door came to carry
    // 224 px of ground without anything failing.
    let sys_card_h = card_pad * 2 + system_plan(dpi, sys_rows()).content_h;
    let card4 = card(content_top, if system { sys_card_h } else { 0 });

    // -- Card 5: the About page, same shape again. One thing here is unlike
    // cards 3 and 4: its height depends on a TEXT MEASUREMENT (the wrapped
    // disclosure), so this function does what it already does for card 2 --
    // `notes_height` is the same kind of call -- and asks for it. The
    // measurement is pure and depends only on `hwnd`, the font and the width,
    // so `layout` recomputing it a moment later cannot get a different answer.
    //
    // **This is the card `MIN_HEIGHT` is derived from**, and the only one on
    // any door whose height moves with a text measurement rather than with a
    // token. At 96 DPI: the mark (36) plus a row gap (20) plus a name line
    // (24) plus two dividers (21 each) plus three rows (46 each) plus the
    // disclosure plus a row gap plus a link row (26), and `CARD_PAD` twice --
    // 340 at a two-line disclosure, 356 at three, 372 at four.
    //
    // No clamp against `content_bottom`, on cards 3 and 4's reasoning, but the
    // margin is REAL here rather than large: the floor of 480 leaves
    // `480 - 44 - 78` = 358 px, so two lines clear it by 18 and three by 2.
    // **Four lines do not fit**, and that is stated rather than guarded
    // because of what it collides with: nothing. The command bar draws no
    // buttons on this door (`command_bar_shown`), so the band the card would
    // reach into is empty ground. See `MIN_HEIGHT` for the full table.
    let about_inner_w = clamp(cw - card_pad * 2);
    let about_card_h = card_pad * 2
        + about_plan(
            dpi,
            disclosure_height(hwnd, ui, dpi, disclosure_text_w(about_inner_w, dpi)),
        )
        .content_h;
    let card5 = card(content_top, if about { about_card_h } else { 0 });

    [card0, card1, card2, card3, card4, card5]
}

/// The four card rects, for `WM_PAINT` -- see `compute_card_rects` for the
/// arithmetic, which is the ONE this function and `layout` both run.
///
/// Takes its own one-time `UI` borrow, dropped on this line, independent of
/// `layout`'s: it is called from a different place (`WM_PAINT`) at a
/// different time, so sharing the arithmetic does not mean sharing a
/// borrow, and this function must obey the same "ONE borrow, dropped
/// immediately" rule on its own. The PAGE it lays out for does not ride in
/// that borrow at all -- `LayoutHandles::of` reads the `PAGE` `Cell`, so a
/// paint arriving while `UI` is held elsewhere still gets the right answer.
pub(super) unsafe fn card_rects(hwnd: HWND) -> [RECT; 6] {
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return [RECT::default(); 6];
    };
    let dpi = GetDpiForWindow(hwnd).max(96);
    compute_card_rects(hwnd, &ui, dpi)
}

/// Four cards — the external-change banner (no height when hidden), the
/// Shortcuts card (head row plus the list), the editor card, the keyboard
/// card — then the command bar, anchored to the bottom and NOT a card (Task 8
/// keeps it a flat band, same as before).
///
/// Everything is placed from the client rect at the current DPI, so a
/// 150 % display is not an afterthought — `GetDpiForWindow` scales the
/// tokens rather than the tokens assuming 96.
///
/// **Vertical shape.** The command bar is the only thing anchored to the
/// bottom; every card stacks downward from the content origin, and each page
/// stacks only the cards it draws. `compute_card_rects` resolves all of that
/// once; this function reads the four rects back and places every control
/// `tok::CARD_PAD` inside whichever card it belongs to.
///
/// **Only the CURRENT page's controls are placed, and that is a correctness
/// requirement rather than an optimisation.** The one call this function
/// makes that can destroy data is `place_h(ui.app, ..)` -- `SetWindowPos` on
/// a populated `CBS_DROPDOWN`, which answers a resize by re-synchronising its
/// edit field to the closest matching item and selecting the whole string, so
/// the next keystroke replaces what the user typed (`Ui::shown_external`, and
/// the module header at the top of `mod.rs`). Skipping the band it lives in
/// is what keeps a tab switch away from it; hiding the control is NOT enough,
/// because what a `SetWindowPos` does to a hidden populated combo is
/// unmeasured (spec §10 open question 2). The strip and the command bar are
/// chrome and are placed on every page.
///
/// **And that skip covers only the outward half of the trip.** Every switch
/// back INTO Shortcuts runs this function with `shortcuts` true, on a combo
/// that in the ordinary case has not moved a pixel -- and the placement is a
/// real resize even so, because the `cy` handed to a combo is its DROPPED
/// height while its window rect holds its closed one, so the request can
/// never equal the current state. That half of the return trip is closed by
/// `combo_needs_placing`: the control is asked where it is, and the call is not
/// made when it is already there.
///
/// **The other half of the return trip is the case where the combo really did
/// need moving**, and no skip can close that one. Because this function leaves
/// the combo alone from three of the four doors, every input it reads that
/// moves while one of those doors is open makes the trip back a genuine
/// placement -- a resize or a `WM_DPICHANGED` taken on another page, the list
/// gaining its first row. (**CORRECTED 2026-08-14, Task 6:** this listed the
/// banner appearing as a fourth, "page-wide since 2026-08-14". It was one for
/// the hours `banner_shown` ignored the page; narrowed back to `BANNER_PAGE`,
/// card 0 cannot gain height while another door is open. The other three are
/// untouched and are why this paragraph stands.)
/// `place_app_combo` is what those go through: it saves the edit's text and
/// selection across the `SetWindowPos` and restores them if the control
/// re-synchronised. It also covers the routes that never touched a door --
/// `WM_SIZE` while the App field holds half-typed text was always going to run
/// this function.
///
/// **The LIST is the one thing that flexes.** See `compute_card_rects`'s
/// own comment on why, and on `editor_min` / `room` / `.min(content_bottom)` — that
/// arithmetic lives there now, not here; this function only reads
/// `card1`'s already-resolved height back out.
///
/// **The keyboard line is the width-critical one.** `MIN_WIDTH` is 660, and
/// a card interior there is `w - 2*tok::PAD - 2*tok::CARD_PAD` = 618 px at
/// 96 DPI — but this line is inset one `gap` at each end (`inner_x` on the
/// left, the `- gap` inside `tap_w` on the right), so the run it gets is 606.
/// The line — `"Use Caps Lock as a shortcut key"`, the three `Hold` chips and
/// the `Tap` combo — was hand-measured at ≈547 px, which leaves `IDC_TAP`
/// about **59 px** against its `tok::SHORTCUT_COL` ceiling of 200.
/// `"Use Caps Lock as a shortcut key"` is the widest measured string in the
/// window, which is why this line and not another decides the floor.
///
/// **At the SHIPPED width the same trace gives 79 px, not 59** (Task 8, where
/// `WINDOW_WIDTH` went 760 -> 680). A card interior is 638 there, the run is
/// 626, and the same ≈547 px line leaves `IDC_TAP` 79 px. Both figures are the
/// same hand measurement read at two widths, so both inherit its error in the
/// same direction and the 20 px between them is just the 20 px between 660 and
/// 680. The floor is what `MIN_WIDTH` is answerable for and it did not move;
/// the number a user actually sees is this one, and it was 159 px while the
/// window opened at 760.
///
/// The version of this note that stood here until 2026-08-14 said
/// `MIN_WIDTH (753)` and concluded ≈150 px of slack. 753 has not been this
/// window's floor since the compaction pass; the real floor is 93 px
/// narrower, and that note also measured the line against `cw` (705 there)
/// rather than against this run, so it was over-generous by two `CARD_PAD`s
/// and a `gap` on top of the width it borrowed from a window that does not
/// exist. **Gate G1 measures the line with `GetTextExtentPoint32W` at 96 and
/// 144 DPI, with the same measurement at a WIDER window as its control.** That
/// control used to be free — 760 was the shipped width, so the gate got it by
/// doing nothing — and since Task 8 it costs a hand-drag, because the window
/// now opens at 680. Say which width the control run was taken at; a G1
/// result with no width beside it answers nothing. Do not move `MIN_WIDTH` —
/// in either direction — before it has run.
pub(super) unsafe fn layout(hwnd: HWND) {
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return;
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = |v: i32| v * dpi as i32 / 96;
    // Independent of WM_GETMINMAXINFO: the floor is about the frame, and a
    // clamp is about the arithmetic. Either alone leaves a negative cy
    // reachable -- SetWindowPos with one produces a control the user can
    // never see or focus again. Widths need it as much as heights: WM_SIZE
    // fires with a 0x0 client rect on minimize (ptMinTrackSize only
    // constrains dragging, not that), so `w` is 0 here on every minimize,
    // on every machine, and every subtraction below goes negative without
    // it.
    //
    // **Widths take `clamp`; a POSITION computed leftward from a right edge
    // takes `.max(cx)` instead.** They are not interchangeable: clamping a
    // width to 0 hides the control, which is recoverable, while clamping a
    // position to 0 puts it outside the surface padding -- flush against the
    // window edge, overlapping whatever is to its left -- which is not.
    //
    // No band needs `.max(cx)` today, and that is structural rather than
    // lucky: every rightward position in this function is spelled
    // `origin + clamp(...)`, which cannot fall left of its origin. The rule
    // is written down here, beside the tool it is about, because the band
    // that reintroduces the hazard will be a NEW one subtracting from a right
    // edge, and it will have no local precedent to copy.
    let clamp = |v: i32| v.max(0);

    // ONE borrow of UI, taken here and dropped on this line. Nothing below
    // may hold one: every SetWindowPos and SendMessageW that follows can
    // re-enter this window's wndproc, and a second borrow across an
    // `extern "system"` boundary aborts the process instead of unwinding.
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return;
    };

    let pad = s(tok::PAD);
    let gap = s(tok::GAP);
    let lblgap = s(tok::LABEL);
    let ctl = s(tok::CTL);
    let card_pad = s(tok::CARD_PAD);

    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let cx = pad;
    let cw = clamp(w - pad * 2);

    // Body, with two named exceptions: every OTHER string measured in this
    // function labels or captions a Body control -- the three command-bar
    // buttons, Add / Remove / Reload / Keep mine, Record / Revert, the four
    // modifier chips, the Caps line's own three words and the "Ag" that sizes
    // the EDIT. (The editor's `App` and `Shortcut` labels were on this list
    // until design §3.1 deleted them on 2026-08-15, and the `Shortcuts`
    // heading -- the window's one Subtitle, and the one string `layout`
    // measured in a third font -- went the same day, which is why `text_size`
    // is now reached only through `tw`, `tw_kc` and the `"Ag"` below.) The
    // exceptions
    // are the three `Hold` chips (`IDC_HOLD_CTRL`/`WIN`/`ALT`), measured
    // through `tw_kc` below: they draw in `Role::Keycap` (Task 8), and a
    // chip sized for 14 px Body text while 11 px Keycap text is drawn into
    // it is a chip with the wrong padding on every side.
    //
    // Measured through `shown`, so a caption's `&` -- a mnemonic marker,
    // which is not drawn -- does not buy the control a character of width it
    // will never use.
    let tw = |t: &str| text_size(hwnd, ui.fonts.get(Role::Body), dpi, &shown(t)).0;
    let tw_kc = |t: &str| text_size(hwnd, ui.fonts.get(Role::Keycap), dpi, &shown(t)).0;
    let btn = |t: &str| s(tok::BTN).max(tw(t) + s(24));

    let place = |id: i32, x: i32, y: i32, cxx: i32, cy: i32| {
        if let Ok(c) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowPos(c, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    };
    let place_h = |h_: HWND, x: i32, y: i32, cxx: i32, cy: i32| {
        let _ = SetWindowPos(h_, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
    };

    // The command bar is anchored, not stacked, and is NOT a card -- Task 8
    // leaves it a flat band, same as before.
    let bar_y = clamp(h - pad - ctl);

    // Field geometry, computed before the Shortcuts card because the filter
    // box needs it there and the editor card needs it too. `combo_h` is
    // therefore read BEFORE the combo is placed this pass, i.e. it is the
    // height the combo had on the PREVIOUS pass. That is sound: the value is
    // the theme's choice for a font and a DPI, so it moves only on
    // WM_DPICHANGED or a font change, both of which run `layout` again
    // immediately. The one pass that can read a not-yet-snapped height is
    // the first, and the floor below falls back to the font-derived height
    // there.
    let text_h = text_size(hwnd, ui.fonts.get(Role::Body), dpi, "Ag").1;
    let field_h = (text_h + s(10)).min(ctl);
    // ONE `GetWindowRect` on the App combo, read for two different questions:
    // how tall the theme made it (`combo_h`, just below) and whether it is
    // already exactly where this pass would put it (`app_seen`, further
    // down). Asking twice would be two answers to one question, and the
    // second answer is the one that decides whether the measured data-loss
    // call runs.
    let app_rect = {
        let mut r = RECT::default();
        if GetWindowRect(ui.app, &mut r).is_ok() {
            Some(r)
        } else {
            None
        }
    };
    let combo_h = app_rect.and_then(|arc| {
        let ah = arc.bottom - arc.top;
        if ah > 0 && ah < ctl && ah >= text_h + s(2) {
            Some(ah)
        } else {
            None
        }
    });
    // The same rect in CLIENT coordinates, which is what every rect this
    // function computes is in. `GetWindowRect` answers in screen
    // coordinates, so the conversion is not optional and its failure means
    // "unknown", which `combo_needs_placing` reads as "place it".
    //
    // Only the top-left is converted: `ScreenToClient` is a translation, so
    // the width is the same number in both spaces and a second call would be
    // a second chance to fail for no extra fact.
    let app_seen = app_rect.and_then(|r| {
        let mut tl = POINT {
            x: r.left,
            y: r.top,
        };
        if ScreenToClient(hwnd, &mut tl).as_bool() {
            Some(ComboSpot {
                x: tl.x,
                y: tl.y,
                cx: r.right - r.left,
            })
        } else {
            None
        }
    });
    // Both EDITs take the combo's height, so the three fields in this window
    // are one box repeated. A single-line EDIT top-aligns its text -- Win32
    // gives it no vertical centring at all -- so it is centred in its band
    // line rather than stretched to it.
    let (edit_h, edit_dy) = match combo_h {
        Some(ah) => (ah, clamp(ctl - ah) / 2),
        None => (field_h, clamp(ctl - field_h) / 2),
    };
    // What a caption costs beyond its own width. Two readings, one number,
    // which is why it is computed here rather than in a card: `IDC_CAPS` is
    // a real `BS_AUTOCHECKBOX` and this is its square plus the gap before
    // its text, while for the seven keycap chips it is the padding around
    // the letters -- `.wtog { padding:0 10px }` plus a hair, since
    // `draw_keycaps` fills whatever width the chip control is given.
    let glyph = s(24);
    // One chip's width: its caption plus that slack, never below
    // `tok::CHIP_MIN`. Both chip rows go through one of these, so the four
    // modifier chips and the three `Hold` chips cannot drift apart on
    // WIDTH -- `draw_chip` fills whatever width it is given, so this
    // closure alone decides how big a key is. `chip_kc` differs only in
    // which font it measures with (Keycap, not Body), for the `Hold` chips
    // -- see `tw_kc` above.
    let chip = |c: &str| (tw(c) + glyph).max(s(tok::CHIP_MIN));
    let chip_kc = |c: &str| (tw_kc(c) + glyph).max(s(tok::CHIP_MIN));

    // The four card rects -- see `compute_card_rects` for the shared
    // arithmetic `card_rects` (called from `WM_PAINT`) also runs. Every
    // control below is placed `card_pad` inside whichever of these it
    // belongs to.
    let [card0, card1, card2, card3, card4, card5] = compute_card_rects(hwnd, &ui, dpi);
    // Which door is open. The same four names `compute_card_rects` binds, and
    // the four must agree: a card given height there and skipped here is an
    // empty card, and a card skipped there and placed into here puts every
    // control at the origin.
    let shortcuts = ui.page == Page::Shortcuts;
    let keyboard = ui.page == Page::Keyboard;
    let system = ui.page == Page::System;
    let about = ui.page == Page::About;

    // -- Band 0: the tab strip, above card 0 and outside all four of them.
    // The trough is not a card, which is why `compute_card_rects` does not
    // return it and `strip_rect` is its own function.
    //
    // **A pill's CONTROL is meant to be bigger than the pill anyone sees.**
    // It carries `tok::FOCUS_SLACK` of margin on all four sides, and the
    // painter is to inset by the same amount -- which is what makes
    // `TABSTRIP_H`'s sum come out (`TAB_VISUAL + 2*TAB_PAD_Y +
    // 2*FOCUS_SLACK`) and why two neighbouring pills are placed with no gap
    // between them: their controls touch, and each one's own margin draws
    // half of the 6 px the strip appears to have. That 6 is `tok::GAP`, so
    // the strip's rhythm is the window's without a second token saying so.
    //
    // **KEPT 2026-08-14, Task 6.** This paragraph read "that is a promise the
    // painter has not kept yet"; `paint::tab_pill` now insets by exactly
    // `tok::FOCUS_SLACK` on all four sides, fills the margin with the trough
    // colour, and draws its focus ring in it -- which is what the token is
    // named for.
    //
    // The row's height is read back OUT of the trough rather than scaled from
    // `TAB_VISUAL` directly, and the two are NOT the same number: at 144 DPI
    // `s(36) - 2*s(2)` is 48 while `s(26) + 2*s(3)` is 47, because `s(3)`
    // truncates 4.5 to 4 and the band pays that twice. Deriving from the
    // trough puts the rounding error inside the pill, where the painter's
    // inset absorbs it; deriving from `TAB_VISUAL` would leave a 1 px seam
    // under the row at 150 %, which reads as a paint bug rather than as
    // integer division.
    //
    // Nothing clamps the run to the trough's right edge. At `MIN_WIDTH` the
    // trough is `660 - 2*PAD` = 640 px wide, and the four pills spend
    // `4 * (2*TAB_PAD_X + 2*FOCUS_SLACK)` = 136 px of padding plus one
    // `badge_slot_w` before a caption is drawn at all -- so the four captions
    // would have to measure about 470 px between them to overflow, and four
    // one-word captions at Body size are not within reach of that. At the
    // shipped 680 the trough is 660 and the budget is 20 px larger again.
    //
    // **CORRECTED 2026-08-14, Task 8: that budget read 504 and left out the
    // badge.** It was written when a pill was exactly its caption plus its
    // padding; Task 6 then added the Shortcuts pill's fixed four-digit slot to
    // this very loop, which is `tok::GAP` plus the width of `"0000"` in the
    // Keycap face -- roughly 34 px at 96 DPI, and the reason the figure here
    // is "about". It is spelled as a term rather than a number because
    // `badge_slot_w` is a live font measurement and this comment is not.
    //
    // The padding half of that is exact; the caption half is unmeasured, here
    // as everywhere in this function (no string in this window has been
    // through `GetTextExtentPoint32W` on hardware -- gate G1). Read it as
    // arithmetic with a large margin, not as a checked fit.
    let strip = strip_rect(rc, dpi);
    let tab_h = clamp(s(tok::TABSTRIP_H) - s(tok::TAB_PAD_Y) * 2);
    let tab_y = strip.top + s(tok::TAB_PAD_Y);
    let mut tab_x = strip.left;
    for (id, opens, caption) in TABS {
        // `tw`, so the pill is sized in the font it is drawn in, and through
        // `shown` like every other measured caption -- these four carry no
        // `&` today and the measurement does not depend on that staying true.
        //
        // The Shortcuts pill is wider by a FIXED badge slot, and fixed is the
        // whole point: the count changes on every add and remove, and the only
        // way to apply a new control width is this function, which is
        // `SetWindowPos` on the populated App combo -- the measured data-loss
        // call (`Ui::shown_external`). So the slot is reserved for
        // `BADGE_SLOT`'s four digits once and never re-measured against the
        // data. `paint::tab_pill` takes the same `badge_slot_w` off the right
        // of its content box before centring the caption in what is left; two
        // spellings of that width would drift into a caption drawn off-centre.
        //
        // It is reserved whether or not the file has any bindings. A slot that
        // appeared with the first binding would move the other three pills
        // sideways on a data push, which is the same forbidden call by another
        // route.
        let badge = if opens == BANNER_PAGE {
            badge_slot_w(hwnd, dpi)
        } else {
            0
        };
        let tab_w = tw(caption) + badge + (s(tok::TAB_PAD_X) + s(tok::FOCUS_SLACK)) * 2;
        place(id, tab_x, tab_y, tab_w, tab_h);
        tab_x += tab_w;
    }

    // -- Card 0: the banner. Contributes NO height when hidden, and since
    // 2026-08-14 it is hidden on a door only when there is nothing to announce
    // -- see `banner_shown`, which is also where that narrows again.
    if banner_shown(ui.external_change, ui.page) {
        let bx = card0.left + card_pad;
        let by = card0.top + card_pad;
        let bw_avail = clamp(card0.right - card0.left - card_pad * 2);
        let bw_reload = btn(cap::RELOAD);
        let bw_keep = btn(cap::KEEP_MINE);
        let buttons = bw_reload + gap + bw_keep;
        place_h(ui.banner, bx, by, clamp(bw_avail - buttons - gap), ctl);
        place_h(
            ui.reload,
            bx + clamp(bw_avail - buttons),
            by,
            bw_reload,
            ctl,
        );
        place_h(ui.keep, bx + clamp(bw_avail - bw_keep), by, bw_keep, ctl);
    }

    // -- Cards 1 and 2: the Shortcuts page.
    //
    // **The `if` is a correctness requirement, not an optimisation, and it
    // is the whole reason this task is the dangerous one.** `place_h(ui.app,
    // ...)` below is `SetWindowPos` on a POPULATED `CBS_DROPDOWN`, which
    // answers a resize by re-synchronising its edit field to the closest
    // matching item and selecting the whole string -- so the next keystroke
    // replaces what the user typed. Measured on a14 (comctl32 6.16, 121
    // items); `Ui::shown_external` exists for it and the module header names
    // it as the one call site nothing may add another of.
    //
    // Hiding a control does NOT make `SetWindowPos` on it harmless -- that
    // is spec 10 open question 2, unmeasured -- so the skip is what keeps the
    // combo out of reach, not the `ShowWindow` that precedes it.
    //
    // The sharp case is `Ctrl+1`..`Ctrl+4`: `TranslateAcceleratorW` runs
    // BEFORE `IsDialogMessageW` (`filter_dialog_message`) and moves no
    // focus at all, so without this the combo would be resized while it is
    // focused, populated, and holding half-typed text.
    if shortcuts {
        // -- Card 1: the Shortcuts card. The filter leading, then Remove and
        // Add right-aligned; the list directly below.
        let cx1 = card1.left + card_pad;
        let cy1 = card1.top + card_pad;
        let cw1 = clamp(card1.right - card1.left - card_pad * 2);

        let bw_add = btn(cap::ADD);
        let bw_remove = btn(cap::REMOVE);
        // Capped at a third of the CARD's interior width, the same ceiling the
        // key list puts on itself in card 2. Nothing competes with it for the
        // rest of the row any more, so the cap is what keeps a filter box from
        // running the width of the card at every size.
        //
        // **Both STATICs that used to open this row are gone**, and the second
        // one moved an edge where the first did not. The `· 18 bindings` count
        // went first and `layout` did nothing with the space, correctly: the
        // heading beside it was sized from its OWN caption and only clamped by
        // where the filter started, so the count's departure freed a gap
        // between two controls and moved neither. The `Shortcuts` heading then
        // went on 2026-08-15 (design §3.1, and `ids.rs` for why), and that one
        // was the row's leading control -- leaving it out without moving
        // anything would open the card with a hole where the design's drawing
        // and the mock-up both put the filter. So the filter takes the card's
        // own left edge, `Add` and `Remove` do not move at all, and the space
        // between them is where the row grows.
        //
        // The filter's `x` is a plain `cx1` now rather than a clamped
        // subtraction, because it no longer depends on the row's
        // right-hand run -- which is why the local that held it is gone. What is
        // NOT guarded, and was not before either: at a card interior under
        // about 282 px the filter's third overlaps `Remove`. `MIN_WIDTH`
        // leaves 618, and every intermediate rect `WM_DPICHANGED` can suggest
        // is clamped elsewhere, so this is the same exposure the previous
        // arithmetic carried, not a new one.
        let filter_w = s(tok::SHORTCUT_COL).min(clamp(cw1 / 3));
        place(IDC_ADD, cx1 + clamp(cw1 - bw_add), cy1, bw_add, ctl);
        place(
            IDC_REMOVE,
            cx1 + clamp(cw1 - bw_add - gap - bw_remove),
            cy1,
            bw_remove,
            ctl,
        );
        place_h(ui.filter, cx1, cy1 + edit_dy, filter_w, edit_h);
        // A control gap, not a card gap: the head row belongs to the list
        // directly below it, so the two read as one group even though both sit
        // inside one card. The row is still `ctl` tall -- the buttons in it
        // decide that, not the heading that used to lead it -- so nothing
        // below this line moved when the heading went.
        let list_y = cy1 + ctl + gap;
        // The flexing height, already resolved by `compute_card_rects` into
        // `card1`'s own height -- read back by subtraction rather than
        // recomputed, so there is only one place that runs the want/room/clamp
        // arithmetic. `card1.bottom - card_pad` is the card's own bottom inset;
        // whatever is left above `list_y` is what the list gets.
        let list_h = clamp(card1.bottom - card_pad - list_y);
        place_h(ui.list, cx1, list_y, cw1, list_h);

        // Columns, sized from the list's OWN client width now that it has one,
        // minus a vertical scroll bar's width whether or not one is showing.
        // That subtraction is what makes overflow structurally impossible: a
        // scroll bar appearing later steals client width the columns have
        // already been told not to use.
        //
        // **This `GetClientRect` is the ONE of `layout`'s six inputs the
        // `apply_state` guard does not track.** (It was the fifth of five
        // until the tab strip made the page the sixth -- which IS guarded,
        // by `Ui::shown_page`. The "fifth" ordinal is gone rather than
        // renumbered, because nothing else counted on it.) When a scroll bar
        // is up the list reports `C - SB`, so the columns get `C - 2*SB`;
        // drop back under the page size and the client returns to `C` while
        // the columns keep the narrower figure until the next resize, DPI
        // change, banner flip or page switch -- roughly a 34 px gutter at 96
        // DPI, 52 at 150 %.
        //
        // Tolerated, on purpose. The subtraction only ever errs in the safe
        // direction: too narrow is a margin, never a clipped column or a
        // horizontal scroll bar, which is the failure this line was written to
        // kill. Guarding it would mean recording this width and re-running
        // `layout` whenever it moved -- i.e. `SetWindowPos` on the populated App
        // combo on a data push, the exact call that silently replaced what the
        // user typed with a catalogue entry (see `Ui::shown_external`). A stale
        // margin is not worth reopening that.
        //
        // If it is ever fixed: the cheap route is a `shown_list_w: Option<i32>`
        // alongside the other three guards, NOT a wider `layout`.
        let mut lrc = RECT::default();
        let inner = if GetClientRect(ui.list, &mut lrc).is_ok() {
            clamp(lrc.right - lrc.left - GetSystemMetricsForDpi(SM_CXVSCROLL, dpi))
        } else {
            0
        };
        // `Shortcut` never takes more than half, so `App` -- which leads, and
        // carries the tick and the flag -- can never be squeezed out.
        let col_shortcut = s(tok::SHORTCUT_COL).min(inner / 2);
        let col_app = clamp(inner - col_shortcut);
        set_column_width(ui.list, 0, col_app);
        set_column_width(ui.list, 1, col_shortcut);

        // -- Card 2: the editor card. TWO lines of fields, then the notes on a
        // third line below them.
        //
        //   +----------------------------------------------------------------+
        //   |  [ Windows Terminal ............................... v ]        |
        //   |  [ ]Ctrl [ ]Win [ ]Alt [ ]Shift [ key v ]  [Record] [Revert]   |
        //   |  ok  Registered. Press Ctrl + Win + Alt + T to focus it.       |
        //   +----------------------------------------------------------------+
        //
        // **Three things left this card on 2026-08-15** (design §3.1): the
        // `Editing "…"` caption (`IDC_GRP_EDITOR`), and the `App` / `Shortcut`
        // labels that used to open each field line. With them went the `s(24)`
        // caption line out of `card2_h` and the label column out of the width.
        //
        // **And so did the `gap` inset the group box needed.** `ins_x` was
        // `grp_x + gap` and `ins_w` was `grp_w - 2*gap`: clearance from a
        // `BS_GROUPBOX`'s drawn frame, kept through the Task 8 review that
        // reclassed the caption to a plain `STATIC` because the caption still
        // wanted to sit outside its own contents. There is no caption and no
        // frame now, so the inset had nothing left to clear -- and it was
        // MISALIGNING the two cards, since card 1's contents start at
        // `card1.left + card_pad` with no such inset. One name (`ed_x` / `ed_w`)
        // where there were two.
        let ed_x = card2.left + card_pad;
        let ed_y = card2.top + card_pad;
        let ed_w = clamp(card2.right - card2.left - card_pad * 2);

        // Line 1: the App field, the full width of the card.
        let mut ly = ed_y;
        // **Two guards, and they answer two different questions.**
        //
        // `combo_needs_placing` is "should this call be made at all". The
        // `if shortcuts` above is one-directional: it keeps the combo out of
        // reach while another door is open, and then every switch BACK through
        // this door placed it again -- on a combo that had not moved a pixel,
        // which is the same resize the a14 measurement pinned (nothing in the
        // layout had moved and typing was still lost). That function carries
        // the whole argument, including why the `cy` below is not what to
        // change and why it settles the no-op return trip without waiting on
        // spec 10 open question 1.
        //
        // `place_app_combo` is "what does the call do when it IS needed", and
        // it is the half that was missing. A return trip after the banner
        // appeared on another door, a resize taken there, a `WM_DPICHANGED`
        // there, or the list gaining its first row all move this combo for
        // real -- so the placement runs, and the placement is what re-snaps the
        // edit. It saves the edit's text and selection across the call.
        //
        // `field_h * 9` is the DROPPED height, and is left exactly as it was.
        // It is also why the height is absent from `ComboSpot`: it is the one
        // component `GetWindowRect` can never report back.
        //
        // Neither guard is applied to `ui.combo` or `IDC_TAP` below. Both are
        // `CBS_DROPDOWNLIST`, which has no edit child for a resize to
        // re-synchronise, so there is no data to lose there and no second
        // measured hazard to guard -- and an unnecessary guard on the two
        // harmless controls would make the ones on the dangerous control look
        // like tidiness.
        let want_app = ComboSpot {
            x: ed_x,
            y: ly + edit_dy,
            cx: ed_w,
        };
        if combo_needs_placing(want_app, app_seen) {
            place_app_combo(ui.app, want_app.x, want_app.y, want_app.cx, field_h * 9);
        }
        ly += ctl + gap;

        // Line 2: the shortcut. Chips left, then the key list, then the two
        // commands right-aligned -- the same "commands close the line" rule the
        // Shortcuts card's Add/Remove follow. The line opens at the card's own
        // left edge now that `Shortcut` is not standing in front of it.
        //
        // Sized from `RECORD`, never from `STOP`: the armed caption is the
        // narrower of the two, so a caption flip cannot clip and `layout` never
        // has to run on the capture path -- which matters, because `layout`
        // means `SetWindowPos` on the populated App combo, the measured
        // data-loss call (`Ui::shown_external`).
        let bw_record = btn(cap::RECORD);
        let bw_reset = btn(cap::REVERT);
        let res_x = ed_x + clamp(ed_w - bw_reset);
        let rec_x = ed_x + clamp(ed_w - bw_reset - gap - bw_record);
        // Each chip is its caption plus `glyph`, floored at `tok::CHIP_MIN` --
        // the same rule the keyboard card's `Hold` chips follow, same two
        // constants. `chip` is declared above so both rows read from it (or
        // from `chip_kc`, its Keycap-measuring twin).
        let w_mod_ctrl = chip(cap::MOD_CTRL);
        let w_mod_win = chip(cap::MOD_WIN);
        let w_mod_alt = chip(cap::MOD_ALT);
        let w_mod_shift = chip(cap::MOD_SHIFT);
        // Chips and key list share the fields' midline (`edit_dy`) and their
        // height, so App, the key list and the filter are ONE box repeated
        // rather than three boxes that happen to be concentric.
        let mut mx = ed_x;
        place(IDC_MOD_CTRL, mx, ly + edit_dy, w_mod_ctrl, edit_h);
        mx += w_mod_ctrl + gap;
        place(IDC_MOD_WIN, mx, ly + edit_dy, w_mod_win, edit_h);
        mx += w_mod_win + gap;
        place(IDC_MOD_ALT, mx, ly + edit_dy, w_mod_alt, edit_h);
        mx += w_mod_alt + gap;
        place(IDC_MOD_SHIFT, mx, ly + edit_dy, w_mod_shift, edit_h);
        mx += w_mod_shift + gap;
        // The key list takes what is between the chips and the commands, under
        // the shortcut column's ceiling.
        //
        // **The ceiling binds again at 680 px, which it had stopped doing**
        // (`tok::SHORTCUT_COL`'s own note records it as one of two that no
        // longer bound). Deleting the label column returned it. The `before`
        // half of that needs no font at all: the run was
        // `rec_x - gap - mx` = `(ins_w - bw_reset - gap - bw_record) - gap -
        // (lw_lbl + lblgap + chips)`, and with `ins_w` 626, both buttons at
        // their `tok::BTN` floor of 88, `lblgap` = `tok::LABEL` = 10 and the
        // four chips at their `tok::CHIP_MIN` floor (`4*46 + 4*gap` = 208),
        // that is `626 - 176 - 12 - 10 - 208` = `220 - lw_lbl` -- under the
        // 200 ceiling for ANY label column wider than 20 px. `lw_lbl` is
        // `tw("Shortcut").max(tw("App")) + s(4)`, so that is `"Shortcut"`
        // measuring more than 16 px, which it does at every DPI.
        //
        // **CORRECTED 2026-08-15: this said `212 - lw_lbl` and "wider than
        // 12 px".** Both were transcription, not derivation -- substituting
        // into the run written on the line above gives 220 and 20, and the
        // pre-rewrite entry in `tok::SHORTCUT_COL` had independently derived
        // the same threshold from the other end ("it would need `"Shortcut"`
        // to measure 16 px"), which is the cross-check that settles it. The
        // conclusion does not move: 20 px is still a label column `"Shortcut"`
        // exceeds everywhere, so the ceiling still did not bind.
        //
        // After: `ins_w` becomes `ed_w` 638 (the group inset went too), the
        // label column is gone, and the run is `638 - 182 - 6 - chips` = `450 -
        // chips`. So the ceiling binds while the four chips total 250 px or
        // less -- 208 at their floor, i.e. 42 px of headroom, and any ONE of
        // `Ctrl`/`Win`/`Alt`/`Shift` measuring more than 22 px in Body starts
        // spending it (a chip is `tw(c) + glyph` floored at `CHIP_MIN`, and
        // `glyph` is `s(24)`). A THRESHOLD, not a measurement: no string in
        // this window has been through `GetTextExtentPoint32W` on hardware
        // (gate G1).
        let key_w = s(tok::SHORTCUT_COL).min(clamp(rec_x - gap - mx));
        // `cy` is the DROPPED-DOWN height here too, capped by the same
        // CB_SETMINVISIBLE(8) the App combo carries.
        place_h(ui.combo, mx, ly + edit_dy, key_w, field_h * 9);
        // Buttons honour `cy` and look right at the band height, so they take
        // `ctl` directly and sit on the band line rather than on the fields'
        // midline -- the same rule the command bar's three follow.
        place(IDC_RECORD, rec_x, ly, bw_record, ctl);
        place(IDC_REVERT, res_x, ly, bw_reset, ctl);
        ly += ctl + gap;

        // Line 3: the notes, inside the card and beside what they describe.
        // Fixed height -- see `notes_height`. `notes_h` is recomputed here
        // (the same pure call `compute_card_rects` already made once, to size
        // card 2) rather than threaded through, because the value cannot
        // disagree between the two calls -- same `hwnd`, `ui`, `dpi` -- and
        // `card_rects`' interface is `[RECT; 4]`, not a richer struct.
        let notes_h = notes_height(hwnd, &ui, dpi);
        place_h(ui.notes, ed_x, ly, ed_w, notes_h);
    }

    // -- Card 3: the Keyboard page. A caption line, then ONE content line,
    // left to right: the check box, then `Hold` and its three chips, then
    // `Tap` and its combo. The only card behind that door today, and since
    // 2026-08-15 it sits at the page's content origin like every other first
    // card rather than clinging to the command bar -- see `compute_card_rects`
    // for the re-stack.
    //
    // Skipped off-page for the same reason cards 1 and 2 are, though the
    // stakes are lower: `IDC_TAP` is a `CBS_DROPDOWNLIST`, which has no edit
    // child for a resize to re-synchronise. Uniformity is the argument here,
    // not a second measured hazard.
    if keyboard {
        let kb_x = card3.left + card_pad;
        let kb_y = card3.top + card_pad;
        let kb_w = clamp(card3.right - card3.left - card_pad * 2);
        // **No caption line, since 2026-08-16.** `IDC_GRP_KEYBOARD` drew the
        // word `Keyboard` here, directly beneath a tab pill captioned
        // `Keyboard`. Design §3.1 deleted exactly that duplication on the
        // Shortcuts door (`IDC_LBL_SECTION`) and §7 rule 5 forbids it in
        // general; this door kept it until a photograph showed it.
        let plan = keyboard_plan(row_metrics(dpi));
        let inner_x = kb_x + gap;
        let ry = kb_y + plan.hold;
        // Every width on this line comes from the caption it has to hold.
        //
        // **`toggle_glyph` (`s(50)`) went with the one-line card, 2026-08-16.**
        // It sized `IDC_CAPS` as caption-plus-track when the switch shared a
        // line with `Hold` and `Tap`; §3.2 gives that switch a row of its own,
        // full card width, so there is no neighbour to leave room for and
        // nothing left to budget. Two paragraphs of subadditivity proof went
        // with it, and only their conclusion is worth carrying: the track's own
        // `2 + 40 + tok::GAP` budget lives in `paint.rs`, is the authority, and
        // is not duplicated here any more -- which was the standing hazard the
        // 2026-08-14 correction in that comment was about. One copy now.
        //
        // The two STATICs below get a hair of slack, for the reason the editor
        // strip's labels do: SS_CENTERIMAGE clips rather than wraps.
        let w_hold = tw(cap::HOLD) + s(4);
        // `chip_kc`, not `chip`: the `Hold` chips draw in `Role::Keycap` (Task
        // 8's role_of change), so they must be MEASURED in it too, or each chip
        // is sized for 14 px Body text and drawn with 11 px Keycap text --
        // wrong padding on every side, in the direction that shows.
        let w_ctrl = chip_kc(cap::HOLD_CTRL);
        let w_win = chip_kc(cap::HOLD_WIN);
        let w_alt = chip_kc(cap::HOLD_ALT);
        let w_tap = tw(cap::TAP) + s(4);
        // `gap * 2` between the three sections of the line, `lblgap` between a
        // word and what it names, `gap` between chips -- so the grouping is
        // legible from the spacing rather than only from the words.
        // -- Group 1: the switch that arms Caps. A switch row is ONE control
        // the full width of the card, exactly as the System page's four are:
        // `paint::toggle` draws the caption at the left and the track at the
        // right of whatever rect it is given, so a full-width rect is what
        // puts the track on the card's right edge and makes the two switch
        // rows on this page line up with each other and with System's.
        place(IDC_CAPS, kb_x, kb_y + plan.caps, kb_w, ctl);

        // -- Group 2: what holding Caps stands for, and what tapping it does.
        let mut kx = inner_x;
        place(IDC_LBL_HOLD, kx, ry, w_hold, ctl);
        kx += w_hold + lblgap;
        place(IDC_HOLD_CTRL, kx, ry, w_ctrl, ctl);
        kx += w_ctrl + gap;
        place(IDC_HOLD_WIN, kx, ry, w_win, ctl);
        kx += w_win + gap;
        place(IDC_HOLD_ALT, kx, ry, w_alt, ctl);
        kx += w_alt + gap * 2;
        place(IDC_LBL_TAP, kx, ry, w_tap, ctl);
        kx += w_tap + lblgap;
        // Whatever the line has left, capped at the same width the filter box
        // and the key list take, so every box in the window narrows together.
        let tap_w = s(tok::SHORTCUT_COL).min(clamp(kb_x + kb_w - gap - kx));
        place(IDC_TAP, kx, ry + edit_dy, tap_w, field_h * 5);

        // -- Group 3: the view preference (design §3.2). Full width, on group
        // 1's reasoning, so the two switches on this page share a right edge.
        //
        // **Its caption is plain text where the drawing sets keycaps inside
        // the sentence.** That needs a painter interleaving text runs and
        // caps, measuring each to place the next, and nothing in this window
        // does it -- `draw_keycaps` lays out a row of caps and
        // `paint::toggle` draws one line of text. Recorded as owed rather
        // than faked; see `beckon_core::page_plan::KeyboardPlan`.
        place(IDC_CAPS_SHORTHAND, kb_x, kb_y + plan.view, kb_w, ctl);
    }

    // -- Card 4: the System page (design §3.3). Nine slots in three groups,
    // parted by two painted dividers; `system_plan` owns every vertical
    // figure and `system_dividers` hands the same two hairlines to
    // `WM_PAINT`.
    //
    // Skipped off-page for cards 1-3's reason. Nothing here is a
    // `CBS_DROPDOWN`, so the measured data-loss call is not in reach on this
    // page -- uniformity is the argument, and one fewer `SetWindowPos` per
    // door change is the incidental gain.
    if system {
        let sx = card4.left + card_pad;
        let sy = card4.top + card_pad;
        let sw = clamp(card4.right - card4.left - card_pad * 2);
        let rows = sys_rows();
        let plan = system_plan(dpi, rows);

        // **A switch row is ONE control the full width of the card**, not a
        // label plus a switch. `paint::toggle` draws the caption at its rect's
        // left edge and the track at the right, so the row's own rect IS the
        // layout -- which is what puts every switch on this page flush with
        // the card's right edge, where the mock-up draws them, with no
        // right-alignment arithmetic and no second control to keep in step.
        place(IDC_PAUSE, sx, sy + plan.pause, sw, ctl);
        if rows.autostart {
            place(IDC_AUTOSTART, sx, sy + plan.autostart, sw, ctl);
        }
        let bw_reload = btn(cap::SYS_RELOAD);
        place(
            IDC_SYS_RELOAD,
            sx + clamp(sw - bw_reload),
            sy + plan.reload,
            bw_reload,
            ctl,
        );
        place(IDC_DARK, sx, sy + plan.dark, sw, ctl);

        // The transparency row: the label-and-value STATIC, then the slider
        // hard right.
        //
        // **The slider's width is fixed, and the STATIC takes what is left.**
        // The mock-up's track is 120 px against a 638 px card, and a slider
        // that grew with the window would make a 15-step range span 500 px --
        // 33 px per step, which is a control that is harder to land on the
        // faster you drag it. The STATIC flexes instead, because what it holds
        // varies: `96%` is four characters and
        // `Off in a remote session` is twenty-three.
        let track_w = s(120);
        let track_h = s(20);
        place(
            IDC_OPACITY,
            sx + clamp(sw - track_w),
            // Centred on the row rather than sitting on its top edge: a
            // trackbar's own rect is taller than the channel it draws, and
            // `ctl` here would put the channel above the label's baseline.
            sy + plan.opacity + clamp(ctl - track_h) / 2,
            track_w,
            track_h,
        );
        place(
            IDC_OPACITY_VALUE,
            sx,
            sy + plan.opacity,
            clamp(sw - track_w - gap),
            ctl,
        );

        // The two file rows: name, value, two glyph buttons.
        //
        // **A glyph button is square-ish rather than `tok::BTN` wide**: it
        // holds one character, and 88 px of button around a 10 px arrow is a
        // target the size of the row it sits in. `ctl` on both axes is the
        // smallest square this window's own grid can produce, which is what
        // keeps them from needing a token of their own.
        let gw = ctl;
        let glyphs = gw * 2 + gap;
        // The value slot is capped at the same width the filter box and the
        // key list take, so every measured column in this window narrows
        // together -- and floored at nothing, since `SS_PATHELLIPSIS` shortens
        // whatever it is given. Both value STATICs are `SS_RIGHT`, so the slot
        // being wider than its contents costs nothing: the text sits against
        // the glyph buttons and the slack falls on the NAME's side, where a
        // long PWA name can use it.
        let val_w = s(tok::SHORTCUT_COL).min(clamp((sw - glyphs - gap) / 2));
        let name_w = clamp(sw - glyphs - gap - val_w - gap);
        let file_row = |y: i32, name: i32, value: i32, open: i32, show: i32| {
            place(name, sx, y, name_w, ctl);
            place(value, sx + name_w + gap, y, val_w, ctl);
            place(open, sx + clamp(sw - glyphs), y, gw, ctl);
            place(show, sx + clamp(sw - gw), y, gw, ctl);
        };
        file_row(
            sy + plan.config,
            IDC_CONFIG_NAME,
            IDC_CONFIG_DIR,
            IDC_CONFIG_OPEN,
            IDC_CONFIG_SHOW,
        );
        if rows.log {
            file_row(
                sy + plan.log,
                IDC_LOG_NAME,
                IDC_LOG_SIZE,
                IDC_LOG_OPEN,
                IDC_LOG_SHOW,
            );
        }
    }

    // -- Card 5: the About page (design §3.4). The mark and the name, a
    // divider, three value rows, a divider, the disclosure, three links.
    // `about_plan` owns every vertical figure and `about_dividers` hands the
    // same two hairlines to `WM_PAINT`.
    //
    // Skipped off-page for cards 1-4's reason. As on System there is no
    // `CBS_DROPDOWN` here, so the measured data-loss call is out of reach on
    // this page and uniformity is the argument.
    //
    // **The waiting line this replaces had its own comment about `card0.top`
    // being the content origin on a page with no card.** That is now
    // `compute_card_rects`' business like every other page's, and the origin
    // reaches this block through `card5` instead.
    if about {
        let ax = card5.left + card_pad;
        let ay = card5.top + card_pad;
        let aw = clamp(card5.right - card5.left - card_pad * 2);
        // The SAME two calls `compute_card_rects` just made, in the same
        // order, on the same inputs -- `notes_height`'s arrangement one card
        // across, and sound for the same reason: both are pure functions of
        // `hwnd`, the font and the width, so the two calls cannot disagree,
        // and `card_rects`' interface is `[RECT; 6]` rather than a richer
        // struct that could carry the answer over.
        let disc_h = disclosure_height(hwnd, &ui, dpi, disclosure_text_w(aw, dpi));
        let plan = about_plan(dpi, disc_h);

        // The mark and the name both take the card's FULL width and centre
        // their own content -- `paint::mark` centres the tile in its rect,
        // `SS_CENTER` centres the name in its. Sizing either to its content
        // would mean measuring a 36 px tile and an 18 px string here only to
        // centre them by arithmetic, and the two would then be centred on two
        // slightly different axes.
        place(IDC_ABOUT_MARK, ax, ay + plan.mark, aw, s(paint::MARK_D));
        place(IDC_ABOUT_NAME, ax, ay + plan.name, aw, s(24));

        // The three value rows: label, value, copy button. The label column is
        // as wide as the widest of the three captions, so the values line up
        // in a column -- the mock-up's `.kv .k{width:74px}` as a measurement
        // rather than as a number, because a fixed 74 would be wrong at every
        // DPI and in any face wider than the one it was traced from.
        //
        // The copy button is `ctl` square, exactly like the System page's four
        // glyph buttons and for the same reason: it holds one character, and
        // `tok::BTN` of button around it would be a target the size of the row.
        let lbl_w = tw(cap::ABOUT_BUILD)
            .max(tw(cap::ABOUT_LOCATION))
            .max(tw(cap::ABOUT_LICENCE))
            + s(4);
        let cw_btn = ctl;
        // The value takes everything between the label and the button. **No
        // `tok::SHORTCUT_COL` ceiling here**, unlike every other measured
        // column in this window: the one thing this slot holds that can be
        // long is a path, `SS_PATHELLIPSIS` shortens it to whatever it is
        // given, and capping the width would mean shortening a path that had
        // room -- spending the card's own width on nothing.
        let val_w = clamp(aw - lbl_w - lblgap - cw_btn - gap);
        let value_row = |y: i32, label: i32, value: i32, copy: i32| {
            place(label, ax, y, lbl_w, ctl);
            place(value, ax + lbl_w + lblgap, y, val_w, ctl);
            place(copy, ax + clamp(aw - cw_btn), y, cw_btn, ctl);
        };
        value_row(
            ay + plan.build,
            IDC_ABOUT_BUILD_LABEL,
            IDC_ABOUT_BUILD_VALUE,
            IDC_ABOUT_BUILD_COPY,
        );

        // The update check (Task 9): two rows under `Build`, mirroring the
        // macOS twin's placement of `update_row` / `command_row` beneath its
        // own `Build`. See `IDC_ABOUT_UPDATE_STATUS`'s own doc in `ids.rs`
        // for why both are always on screen rather than shown/hidden with
        // the row.
        //
        // Row one: the status line, `Check now` and `Open releases page`.
        // The two buttons are right-aligned as a pair -- the links row below
        // is the same "measure each caption, sum the run" shape, minus the
        // centring, since this row answers a question above it (design
        // §3.4's asymmetry between an answering row and a leaving row) --
        // and the status line takes whatever is left, uncapped, on the value
        // column's own reasoning just above: a short-lived status word must
        // not be truncated for the same reason a path is not.
        let bw_check = btn(cap::CHECK_NOW);
        let bw_open_releases = btn(cap::ABOUT_OPEN_RELEASES);
        let buttons_w = bw_check + gap + bw_open_releases;
        let status_w = clamp(aw - buttons_w - gap);
        place(IDC_ABOUT_UPDATE_STATUS, ax, ay + plan.update, status_w, ctl);
        let mut ux = ax + clamp(aw - buttons_w);
        place(IDC_ABOUT_CHECK_NOW, ux, ay + plan.update, bw_check, ctl);
        ux += bw_check + gap;
        place(
            IDC_ABOUT_OPEN_RELEASES,
            ux,
            ay + plan.update,
            bw_open_releases,
            ctl,
        );

        // Row two: the upgrade command's own value and copy button --
        // `value_row`'s value-and-copy half exactly, with no label column:
        // there is nothing here for a label to name that `Check now` and the
        // status line above it have not already said.
        place(
            IDC_ABOUT_UPDATE_VALUE,
            ax,
            ay + plan.command,
            clamp(aw - cw_btn - gap),
            ctl,
        );
        place(
            IDC_ABOUT_UPDATE_COPY,
            ax + clamp(aw - cw_btn),
            ay + plan.command,
            cw_btn,
            ctl,
        );

        value_row(
            ay + plan.location,
            IDC_ABOUT_LOCATION_LABEL,
            IDC_ABOUT_LOCATION_VALUE,
            IDC_ABOUT_LOCATION_COPY,
        );
        value_row(
            ay + plan.licence,
            IDC_ABOUT_LICENCE_LABEL,
            IDC_ABOUT_LICENCE_VALUE,
            IDC_ABOUT_LICENCE_COPY,
        );

        // The disclosure takes the card's full interior; `paint::disclosure`
        // insets the dot column itself, from the same two constants
        // `disclosure_text_w` subtracted above.
        place(IDC_ABOUT_DISCLOSURE, ax, ay + plan.disclosure, aw, disc_h);

        // The three links, CENTRED as a run rather than right-aligned like
        // every other button row in this window. That is the drawing
        // (`.linkrow{justify-content:center}`) and it is also the only row
        // here that is not answering a question above it -- a command bar's
        // buttons close a page, and these three leave it.
        //
        // Each is sized from its own caption through `btn`, so `Report a bug`
        // is wider than `GitHub` and the run is their sum plus two gaps. The
        // start is clamped to the card's left edge: at a width where the three
        // do not fit, they run off the RIGHT (where the window can be
        // widened) rather than off the left under the card border.
        let bw_github = btn(cap::ABOUT_GITHUB);
        let bw_releases = btn(cap::ABOUT_RELEASES);
        let bw_bug = btn(cap::ABOUT_BUG);
        let run = bw_github + gap + bw_releases + gap + bw_bug;
        let mut lx = ax + clamp(aw - run) / 2;
        place(IDC_ABOUT_GITHUB, lx, ay + plan.links, bw_github, ctl);
        lx += bw_github + gap;
        place(IDC_ABOUT_RELEASES, lx, ay + plan.links, bw_releases, ctl);
        lx += bw_releases + gap;
        place(IDC_ABOUT_BUG, lx, ay + plan.links, bw_bug, ctl);
    }

    // -- The command bar. Save is the outermost button on the right, Close
    // inboard of it, `Open config file` hard left -- as far from Save as
    // the bar allows. Not a card; anchored at `bar_y`, same as before.
    // The service line (design §6.4) takes the bar's LEFT end, on every door.
    // It runs from the card column's left edge to wherever the leftmost button
    // starts, less a gap -- so on System and About, where no button is drawn,
    // it has the whole bar and the band stops being empty ground.
    //
    // Placed unconditionally like the three buttons: it is chrome. Its width
    // is what is left rather than a measurement of its own text, because the
    // text changes with the service and `layout` must not run on a data push.
    let bw_open = btn(cap::OPEN_FILE);
    let bw_apply = btn(cap::SAVE);
    let bw_close = btn(cap::CLOSE);
    place(IDC_OPENFILE, cx, bar_y, bw_open, ctl);
    // `command_bar_shown` decides whether the buttons are there to make room
    // for -- the same predicate that hides them, so the two cannot disagree
    // about which doors have a gap on the left.
    // **The left end is only free where the buttons are gone.** §6.4 puts the
    // service line hard left because in its world auto-save has deleted all
    // three buttons; here they still exist on the two doors that save, and
    // `Open config file` sits exactly where §6.4 expects the line. Placing it
    // at `cx` regardless drew it UNDER that button on Shortcuts and Keyboard,
    // which is invisible rather than wrong-looking -- found in a photograph,
    // because nothing in the layout overlaps in a way a test could see.
    //
    // So the line starts past `Open config file` where that button is drawn,
    // and at the card column's own left edge where it is not. Both ends come
    // from `command_bar_shown`, the same predicate that shows the buttons, so
    // the gap and the thing filling it cannot disagree.
    let (service_left, service_right) = if command_bar_shown(ui.page) {
        (
            cx + bw_open + gap * 2,
            cx + clamp(cw - bw_apply - gap - bw_close - gap * 2),
        )
    } else {
        (cx + gap, cx + cw)
    };
    place(
        IDC_SERVICE_LINE,
        service_left,
        bar_y,
        clamp(service_right - service_left),
        ctl,
    );
    place(IDC_APPLY, cx + clamp(cw - bw_apply), bar_y, bw_apply, ctl);
    place(
        IDC_CLOSE,
        cx + clamp(cw - bw_apply - gap - bw_close),
        bar_y,
        bw_close,
        ctl,
    );
}

/// **The first tests this file has ever had, 2026-08-15.**
///
/// Nothing here touches an `HWND`, and that is the whole reason they can
/// exist: they re-run the vertical arithmetic of `compute_card_rects` against
/// the same constants that function reads, for the three doors whose cards are
/// FIXED. `compute_card_rects` itself cannot be called without a window, so
/// what is checked is the arithmetic rather than the function -- and the two
/// are kept honest by every term below being the token or the core function
/// the real one uses, never a transcribed number.
///
/// **They run on the Windows CI job only.** `settings_window` is `cfg`-gated,
/// so `cargo test` on ubuntu and macOS never sees them; on those two jobs the
/// crate is `--exclude`d outright. The half that runs everywhere is
/// `beckon_core::page_plan`'s, which is why the plans were moved there.
#[cfg(test)]
mod tests {
    use super::*;

    /// `compute_card_rects`' own vertical terms, spelled once here.
    fn geometry(h: i32) -> (i32, i32) {
        let content_top = chrome::TITLEBAR_H + tok::TABSTRIP_H + tok::GAP_CARD;
        let content_bottom = h - tok::PAD - tok::CTL - tok::GAP_CARD;
        (content_top, content_bottom)
    }

    fn card_h(content_h: i32) -> i32 {
        tok::CARD_PAD * 2 + content_h
    }

    const M96: RowMetrics = RowMetrics {
        ctl: tok::CTL,
        row_gap: tok::ROW_GAP,
        div_gap: tok::DIV_GAP,
        mark: paint::MARK_D,
        name: 24,
    };

    const BOTH: SystemRows = SystemRows {
        autostart: true,
        log: true,
    };

    /// The seam. `row_metrics` scales the tokens for core, and at 96 DPI the
    /// scale is the identity -- so this is the assertion that a token moved
    /// here reaches the arithmetic tested there.
    #[test]
    fn the_row_rhythm_is_the_one_core_stacks_with() {
        assert_eq!(row_metrics(96), M96);
        assert_eq!(M96.pitch(), 46, "the setting-row pitch left the drawing");
        // 144 DPI is the a14 screenshots' scale, and the one the probe runs
        // at; a rounding change here would move every row on two doors.
        assert_eq!(row_metrics(144).ctl, 39);
        assert_eq!(row_metrics(144).row_gap, 30);
    }

    /// **The defect, as an assertion.** At the shipped size the System card
    /// ended 224 px above the command bar and About 210 -- a third of the
    /// window, on two doors out of four -- and no test anywhere could fail.
    ///
    /// The bound is 60 px: one row's pitch (46) plus a card gap, which is the
    /// most a page can leave before the emptiness stops reading as margin.
    /// The mock-up, measured in Chrome, leaves 10 px on System and 14 on
    /// About.
    ///
    /// **NARROWED to About alone, 2026-08-25 (Task 9).** System's own ground
    /// is no longer checked against the 60 px bound here: `WINDOW_HEIGHT`
    /// grew to fit About's two new rows and System's card did not grow with
    /// it, so System now leaves real ground behind by DESIGN, not by the
    /// defect this test was written to catch -- `page_plan`'s own
    /// `about_now_legitimately_outgrows_system_by_the_update_check` is where
    /// that gap is pinned and explained. Pretending otherwise here would mean
    /// either shrinking About back down or failing a healthy build. What this
    /// test keeps catching, unweakened, is About's OWN ground -- the page
    /// `WINDOW_HEIGHT` is actually derived from -- and it still fails the
    /// instant that page regains the empty third of a window the original
    /// defect looked like.
    #[test]
    fn the_fixed_doors_leave_no_room_for_a_second_card() {
        let (top, bottom) = geometry(WINDOW_HEIGHT);
        let sys = card_h(beckon_core::page_plan::system_plan(M96, BOTH).content_h);
        let about = card_h(beckon_core::page_plan::about_plan(M96, 32).content_h);
        assert_eq!(sys, 326);
        assert_eq!(about, 432);
        let about_ground = bottom - (top + about);
        assert!(
            (0..=60).contains(&about_ground),
            "the About door leaves {about_ground} px of ground at the \
             shipped size (card {about}, page {} px); WINDOW_HEIGHT is \
             derived from this page, so this bound is what keeps that \
             derivation honest",
            bottom - top
        );
        // System's own ground, pinned rather than bounded -- see the
        // 2026-08-25 note above for why 60 no longer applies to it. A change
        // to this number is a change to how much emptier System reads than
        // About, and is worth a second look, not a silent pass.
        let sys_ground = bottom - (top + sys);
        assert_eq!(
            sys_ground, 144,
            "System's ground moved; re-read whether the gap it leaves below \
             its card is still acceptable before updating this number"
        );
    }

    /// The floor has to fit the card it was derived from, or the window has a
    /// size at which a page draws into the command bar's band.
    ///
    /// Three-line disclosure, which is the row `MIN_HEIGHT` is set from --
    /// see its own table for why four lines is stated rather than guarded.
    #[test]
    fn the_fixed_doors_fit_above_the_command_bar_at_the_floor() {
        let (top, bottom) = geometry(MIN_HEIGHT);
        let sys = card_h(beckon_core::page_plan::system_plan(M96, BOTH).content_h);
        let kb = tok::CARD_PAD * 2 + 24 + tok::CTL + tok::GAP;
        for (name, h) in [
            ("System", sys),
            ("Keyboard", kb),
            (
                "About, two lines",
                card_h(beckon_core::page_plan::about_plan(M96, 32).content_h),
            ),
            (
                "About, three lines",
                card_h(beckon_core::page_plan::about_plan(M96, 48).content_h),
            ),
        ] {
            assert!(
                top + h <= bottom,
                "{name} needs {} px and the floor gives {}",
                h,
                bottom - top
            );
        }
    }

    /// The Shortcuts list is what absorbs whatever the fixed cards leave, so
    /// its row count is a CONSEQUENCE of the two constants above rather than
    /// an input to them -- but it still has to be a list.
    ///
    /// `MIN_HEIGHT`'s own three bullets are these numbers; this is what makes
    /// them fail rather than merely go stale.
    #[test]
    fn the_list_is_still_worth_looking_at_at_both_sizes() {
        // `compute_card_rects`' chain, banner DOWN, with the 96-DPI fallback
        // row and a one-line notes strip (`notes_h = 2L + 4`, L = 16).
        let rows = |h: i32| {
            let (top, bottom) = geometry(h);
            let list_top = top + tok::CARD_PAD + tok::CTL + tok::GAP;
            let card2_h = tok::CARD_PAD * 2 + tok::CTL * 2 + tok::GAP * 3 + 36;
            let avail = (bottom - list_top) - tok::GAP_CARD - tok::CARD_PAD - card2_h;
            avail / tok::ROW_H
        };
        // 8 / 7 before 2026-08-25 (Task 9); `WINDOW_HEIGHT` / `MIN_HEIGHT`
        // both grew for About's two new rows (see `MIN_HEIGHT`'s own
        // "CORRECTED 2026-08-25" note), and the Shortcuts list -- which
        // absorbs whatever the fixed cards leave -- grew right along with
        // them.
        assert_eq!(rows(WINDOW_HEIGHT), 12);
        assert_eq!(rows(MIN_HEIGHT), 11);
        assert!(
            rows(MIN_HEIGHT) >= 2,
            "a window whose list shows one row is not a smaller version of \
             this window, it is a broken one"
        );
    }
}
