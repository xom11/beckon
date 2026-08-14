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
    /// A label and the control it names.
    pub const LABEL: i32 = 10;
    /// Height of one band line, and of every button on it.
    pub const CTL: i32 = 26;
    /// A button is never narrower than this, nor than its own caption.
    pub const BTN: i32 = 88;
    /// The right-aligned `Shortcut` column, the editor field under it, and
    /// the key list's ceiling.
    pub const SHORTCUT_COL: i32 = 200;
    /// A modifier chip is never narrower than this, nor than its own caption
    /// plus `glyph` -- direction B's `.wtog { min-width:46px }`.
    pub const CHIP_MIN: i32 = 46;
    /// List rows visible without scrolling. Fixed at every DPI, not scaled,
    /// not derived from the config.
    pub const ROWS: i32 = 8;
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
    /// token exists for. Left as a token from Task 8 rather than invented
    /// later because Task 8 is what sizes the list's card around
    /// `tok::ROWS`, and the two numbers belong beside each other.
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
/// why gate G-S5 prints those metrics by name rather than assuming them.
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

/// The four card rects, top to bottom: the external-change banner, the
/// Shortcuts card (head row plus the list), the editor card, the keyboard
/// card.
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
/// **What this does NOT do is re-stack the window per page.** The keyboard
/// card stays bottom-anchored above the command bar and still reserves its
/// height on every page, so the Shortcuts page keeps a card-shaped gap above
/// the command bar and the Keyboard page keeps a larger one below the strip.
///
/// **DECIDED 2026-08-14, Task 7, and deferred again.** This paragraph used to
/// end "System and About are where it stops being tolerable -- both are a
/// single line with no card at all yet -- so the page that adds them is the
/// one that should re-stack." That page is this one, and it did not. The
/// reasons, worst-case first:
///
/// - **The re-stack is a change to the SHORTCUTS page's vertical geometry, and
///   that geometry is another workstream's open subject.** Design §4 uncaps
///   the list and deletes `tok::ROWS`; design §3.1 deletes the editor card's
///   `Editing "…"` caption, which is the `s(24)` inside `grp_content_h` and so
///   an input to `card2_h`. `MIN_HEIGHT`'s own comment already names that
///   caption as pending and solves the table with it struck out. Re-deriving
///   the table now means re-deriving it again a landing later, and the second
///   pass would be checking the first pass's arithmetic rather than the
///   window's.
/// - **Nothing on the host this is written on can display the window.** Every
///   vertical figure in `MIN_HEIGHT` and beside `WINDOW_HEIGHT` is a hand
///   trace of this function; those figures were corrected twice on 2026-08-13
///   and 2026-08-14 and re-derived once more when the strip landed, and each
///   pass is where a stale number gets written down as fact. Two STATICs are
///   not worth a fourth.
///
/// **What deferring costs, re-derived here rather than asserted.** The
/// keyboard card's reservation is `gap_card + kb_card_h` = 8 + 78 = 86 px at
/// 96 DPI, which the list would otherwise get: `list_h` would go from
/// `h - 386 - notes_h` to `h - 300 - notes_h` with the banner down. At the
/// shipped client height of 600 with `notes_h` 36, that is a cap of 178 rather
/// than 264, against a `want` of `21 + 8*22` = 197 -- so today the cap binds
/// at seven whole rows and 3 px of an eighth, and after a re-stack `want`
/// would bind at eight. **One row at the shipped size, plus the 86 px gap
/// above the command bar**, which is the visible half and is on the page the
/// user lives on. At `MIN_HEIGHT` with the banner up it is larger -- 82 px
/// (two rows) against 168 (six) -- but nobody sits at the floor.
///
/// **What System and About needed instead, and got.** A page whose entire
/// content is one line has no stack to re-derive: `layout` puts each waiting
/// line at the content origin (card 0's top, which on those two pages is the
/// origin -- see that block) and the emptiness below it is the page being
/// empty rather than the line being misplaced. That is what "waiting" is
/// supposed to look like.
unsafe fn compute_card_rects(hwnd: HWND, ui: &LayoutHandles, dpi: u32) -> [RECT; 4] {
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return [RECT::default(); 4];
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
    // Which doors own the four cards. Cards 0-2 are the Shortcuts page; card
    // 3 is the Keyboard page. **System and About own none of them and are not
    // waiting for one**: each shows a single line with no card behind it, so
    // all four rects stay at zero height there and `layout` places that line
    // against the content origin directly. See the doc comment above on the
    // re-stack this landing decided against.
    let shortcuts = ui.page == Page::Shortcuts;
    let keyboard = ui.page == Page::Keyboard;

    // The two bottom bands are anchored, not stacked, so the window's
    // bottom edge is where they stay however tall the content above is.
    // The keyboard card's CONTENT keeps `kb_h`'s exact pre-Task-8 shape --
    // caption inset, one control line, a bottom inset the size of `gap` --
    // because that shape was always the caption's own `s(24)` line plus one
    // control line plus a bottom inset, and none of those three numbers
    // changed when the review fix reclassed `IDC_GRP_KEYBOARD` from
    // `BS_GROUPBOX` to a plain caption `STATIC` (see its creation comment in
    // `build_children`) -- only the CONTROL drawing that first `s(24)`
    // changed, not its height. Only the card's own `CARD_PAD` wrapping
    // around it is new, same as before that reclass.
    let bar_y = clamp(h - pad - ctl);
    let kb_content_h = s(24) + ctl + gap;
    let kb_card_h = card_pad * 2 + kb_content_h;
    let kb_y = clamp(bar_y - gap_card - kb_card_h);
    // `kb_y` is computed on EVERY page, and only the rect is page-bound: it
    // is the bottom stop the Shortcuts list measures its room against, and
    // making that stop move with the door is the re-stack this function's doc
    // comment weighs and defers. THIS line is the one to change when it is
    // taken -- the reservation is `gap_card + kb_card_h`, 86 px at 96 DPI, and
    // everything under `MIN_HEIGHT` and beside `WINDOW_HEIGHT` moves with it.
    let card3 = card(kb_y, if keyboard { kb_card_h } else { 0 });

    // The editor card's content keeps `grp_h`'s exact pre-Task-8 shape too
    // -- caption inset, two lines, the notes, a bottom inset -- for the same
    // reason as `kb_content_h` above: `IDC_GRP_EDITOR`'s caption `s(24)` line
    // is the same height whether a `BS_GROUPBOX` or a plain caption `STATIC`
    // draws it, and the review fix that reclassed it changed the control,
    // not this arithmetic. Computed HERE, before the banner and card 1,
    // because card 1 has to yield to it below and the two must not each
    // hold an opinion about how tall the editor card is.
    let notes_h = notes_height(hwnd, ui, dpi);
    let grp_content_h = s(24) + ctl + gap + ctl + gap + notes_h + gap;
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

    // -- Card 1: the Shortcuts card, head row plus the list. The list is
    // the one thing that flexes -- it wants `header + tok::ROWS rows` and
    // gives that up rather than let anything overlap when the window is
    // short: a shrunk list scrolls, an overlapped control is unreachable.
    // Everything below it (card 2, card 3) is fixed, which is what makes it
    // the thing that must yield.
    //
    // No `+ border` term any more (Task 10). That used to be
    // `2 * SM_CYBORDER`, the two pixels `WS_BORDER` drew OUTSIDE the list's
    // own client area -- height the control's window rect needed beyond its
    // content that a border-less control no longer spends. The list's
    // border is the card's now (`paint::card`), so there is nothing left
    // for that term to pay for.
    let row_h = list_row_height(ui.list, dpi);
    let want = list_header_height(ui.list, dpi) + row_h * tok::ROWS;
    // Where the LIST ITSELF starts: past card 1's own top inset, the head
    // row and the control gap below it. The direct analogue of the
    // pre-Task-8 `y` this same computation read, which was already past
    // band 2's head row for the same reason -- a bare band's content sat
    // right at `y` with no inset of its own, which is what let the old
    // formula skip this step.
    let list_top = y + card_pad + ctl + gap;
    // `editor_min` is card 2's WHOLE footprint, `CARD_PAD` included -- not
    // just `grp_content_h` -- because that whole footprint is the room card
    // 2 actually needs to sit in below card 1. `room` reserves one
    // `gap_card`: the guaranteed clearance between card 2's bottom and card
    // 3's top, the same role a `band` gap played here before cards existed.
    // The second subtraction below, `- card_pad`, is new: it reserves card
    // 1's OWN bottom inset, a fixed cost the old bare band 3 never had.
    // Miss it and the guard below cannot save it: at `MIN_HEIGHT`, where
    // this floor is exact-fit by construction, the list would be handed
    // `card_pad` (11 px at 96 DPI) more room than the card can actually
    // afford, and card 2 draws exactly that far over card 3 -- worst with
    // the banner up, where there is no slack left to absorb it, because the
    // 56 px card 0 would otherwise have taken (its own 48 plus the
    // `gap_card` below it) swallows a shortfall that small with room to
    // spare. Simulated, not seen: nothing on the machine this was written
    // on can display the window.
    let editor_min = card2_h;
    let room = clamp(kb_y - gap_card - list_top);
    let list_h = clamp(want.min(clamp(room - gap_card - card_pad - editor_min)));
    let card1_h = card_pad * 2 + ctl + gap + list_h;
    // The height is computed either way and spent only on its own page, so
    // the arithmetic above has exactly one shape rather than one per door.
    let card1 = card(y, if shortcuts { card1_h } else { 0 });
    y += card1_h + gap_card;
    // `y.min(kb_y)`: bounds card 2's TOP, and only its top. `card2_h` is
    // fixed -- not something `clamp` shrinks the way it shrinks `list_h`
    // above -- so this line cannot pull card 2's BOTTOM back up off card 3;
    // what keeps the bottom clear at and above `MIN_HEIGHT` is `editor_min`
    // reserving the whole of `card2_h` before the list takes any height at
    // all. Reachable in the state where `room` itself clamped negative --
    // an intermediate resize below `MIN_HEIGHT` that `WM_DPICHANGED`'s
    // suggested rect can hand this function without asking
    // `WM_GETMINMAXINFO` first (dragging can't reach it; a 0x0 client rect
    // clamps everything to 0 and is fine).
    y = y.min(kb_y);
    let card2 = card(y, if shortcuts { card2_h } else { 0 });

    [card0, card1, card2, card3]
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
pub(super) unsafe fn card_rects(hwnd: HWND) -> [RECT; 4] {
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return [RECT::default(); 4];
    };
    let dpi = GetDpiForWindow(hwnd).max(96);
    compute_card_rects(hwnd, &ui, dpi)
}

/// Four cards, top to bottom: the external-change banner (no height when
/// hidden), the Shortcuts card (head row plus the list), the editor card,
/// the keyboard card — then the command bar, anchored to the bottom and NOT
/// a card (Task 8 keeps it a flat band, same as before).
///
/// Everything is placed from the client rect at the current DPI, so a
/// 150 % display is not an afterthought — `GetDpiForWindow` scales the
/// tokens rather than the tokens assuming 96.
///
/// **Vertical shape.** The command bar is anchored to the bottom and the
/// keyboard card sits directly above it; the top cards stack downward.
/// `compute_card_rects` resolves all of that once; this function reads the
/// four rects back and places every control `tok::CARD_PAD` inside
/// whichever card it belongs to.
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
/// own comment on why, and on `editor_min`/`room`/`y.min(kb_y)` — that
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
/// The version of this note that stood here until 2026-08-14 said
/// `MIN_WIDTH (753)` and concluded ≈150 px of slack. 753 has not been this
/// window's floor since the compaction pass; the real floor is 93 px
/// narrower, and that note also measured the line against `cw` (705 there)
/// rather than against this run, so it was over-generous by two `CARD_PAD`s
/// and a `gap` on top of the width it borrowed from a window that does not
/// exist. **Gate G1 measures the line with `GetTextExtentPoint32W` at 96 and
/// 144 DPI, with the same measurement at 760 px as its control.** Do not move
/// `MIN_WIDTH` — in either direction — before it has run.
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
    // buttons, Add / Remove / Reload / Keep mine, the two field labels, the
    // App/Shortcut/Tap row and the "Ag" that sizes the EDIT. The `Shortcuts`
    // heading is the one Subtitle in the window and its width is never
    // measured; it takes whatever Add and Remove leave it. The exceptions
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
    let [card0, card1, card2, card3] = compute_card_rects(hwnd, &ui, dpi);
    // Which door is open. The same two names `compute_card_rects` binds, and
    // the two must agree: a card given height there and skipped here is an
    // empty card, and a card skipped there and placed into here puts every
    // control at the origin.
    let shortcuts = ui.page == Page::Shortcuts;
    let keyboard = ui.page == Page::Keyboard;

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
    // Nothing clamps the run to the trough's right edge. The four captions
    // would have to measure 504 px between them to overflow it at
    // `MIN_WIDTH` -- 660 less `2*PAD` is 640, less the
    // `4 * (2*TAB_PAD_X + 2*FOCUS_SLACK)` = 136 px of padding they carry --
    // and four one-word captions at Body size are not within reach of that.
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
        // -- Card 1: the Shortcuts card. `Shortcuts` leading, then the filter,
        // then Remove and Add right-aligned; the list directly below.
        let cx1 = card1.left + card_pad;
        let cy1 = card1.top + card_pad;
        let cw1 = clamp(card1.right - card1.left - card_pad * 2);

        let bw_add = btn(cap::ADD);
        let bw_remove = btn(cap::REMOVE);
        // Capped at a third of the CARD's interior width, the same ceiling the
        // key list puts on itself in card 2. The HEADING takes what is left,
        // which makes it -- not the filter -- the first thing to run out.
        // Every subtraction is clamped, so an intermediate rect `WM_DPICHANGED`
        // can suggest below `MIN_WIDTH` produces a hidden heading rather than a
        // negative width.
        let filter_w = s(tok::SHORTCUT_COL).min(clamp(cw1 / 3));
        let filter_x = cx1 + clamp(cw1 - bw_add - gap - bw_remove - gap - filter_w);
        place(IDC_ADD, cx1 + clamp(cw1 - bw_add), cy1, bw_add, ctl);
        place(
            IDC_REMOVE,
            cx1 + clamp(cw1 - bw_add - gap - bw_remove),
            cy1,
            bw_remove,
            ctl,
        );
        place_h(ui.filter, filter_x, cy1 + edit_dy, filter_w, edit_h);
        // **The heading is measured now**, where it never used to be: it shares
        // its line with the count, so it has to end somewhere definite rather
        // than taking everything up to the filter. Measured in SUBTITLE -- the
        // only string in `layout` that is not Body or Keycap, and `tw` would
        // under-measure it by a third and put the count on top of it.
        let head_w = text_size(hwnd, ui.fonts.get(Role::Subtitle), dpi, "Shortcuts").0 + s(4);
        let head_w = head_w.min(clamp(filter_x - gap - cx1));
        place(IDC_LBL_SECTION, cx1, cy1, head_w, ctl);
        place(
            IDC_LBL_COUNT,
            cx1 + head_w + lblgap,
            cy1,
            clamp(filter_x - gap - (cx1 + head_w + lblgap)),
            ctl,
        );
        // A control gap, not a card gap: the head labels the list directly
        // below it, so the two read as one group even though both now sit
        // inside one card.
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

        // -- Card 2: the editor card. A caption line, then TWO lines of fields,
        // then the notes on a third line below that -- unchanged internally
        // from before Task 8; only its origin now comes from card 2 instead of
        // a running `y` cursor.
        //
        //   +- Editing "Windows Terminal" ----------------------------------+
        //   |  App       [ ..................................... v ]        |
        //   |  Shortcut  [ ]Ctrl [ ]Win [ ]Alt [ ]Shift [ key v ]  [R] [R]  |
        //   |  ok  Registered. Press Ctrl + Win + Alt + T to focus it.      |
        //   +---------------------------------------------------------------+
        //
        // Bound once, and named rather than left as `y`, because the caption's
        // top edge is a coordinate other controls are placed against.
        //
        // `IDC_GRP_EDITOR` is placed at `grp_x, grp_y, grp_w, s(24)` -- ITS OWN
        // caption line, not the card's whole interior -- since the review fix
        // that reclassed it from `BS_GROUPBOX` to a plain caption `STATIC` (see
        // the creation comment in `build_children`). Before that fix this
        // control was the group box's own frame and got the full `grp_h`,
        // `card2.bottom - card2.top - card_pad * 2`; a `STATIC` paints no frame
        // at all, so giving it that same full height bought nothing and cost a
        // click-through dead zone over the fields below it. `card2_h` (in
        // `compute_card_rects`) still budgets `s(24)` for this line -- the
        // reclass moved which control draws it, not how tall it is.
        let grp_y = card2.top + card_pad;
        let grp_x = card2.left + card_pad;
        let grp_w = clamp(card2.right - card2.left - card_pad * 2);
        let ins_x = grp_x + gap;
        let ins_w = clamp(grp_w - gap * 2);
        place(IDC_GRP_EDITOR, grp_x, grp_y, grp_w, s(24));

        // Both lines share one label column, so `App` and `Shortcut` left-align
        // with each other instead of each starting wherever its own line does.
        let lw_lbl = tw("Shortcut").max(tw("App")) + s(4);
        let fld_x = ins_x + lw_lbl + lblgap;
        let fld_w = clamp(ins_x + ins_w - fld_x);

        // Line 1: App, full width.
        let mut ly = grp_y + s(24);
        place(IDC_LBL_APP, ins_x, ly, lw_lbl, ctl);
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
            x: fld_x,
            y: ly + edit_dy,
            cx: fld_w,
        };
        if combo_needs_placing(want_app, app_seen) {
            place_app_combo(ui.app, want_app.x, want_app.y, want_app.cx, field_h * 9);
        }
        ly += ctl + gap;

        // Line 2: the shortcut. Chips left, then the key list, then the two
        // commands right-aligned -- the same "commands close the line" rule the
        // Shortcuts card's Add/Remove follow.
        place(IDC_LBL_SHORTCUT, ins_x, ly, lw_lbl, ctl);
        // Sized from `RECORD`, never from `STOP`: the armed caption is the
        // narrower of the two, so a caption flip cannot clip and `layout` never
        // has to run on the capture path -- which matters, because `layout`
        // means `SetWindowPos` on the populated App combo, the measured
        // data-loss call (`Ui::shown_external`).
        let bw_record = btn(cap::RECORD);
        let bw_reset = btn(cap::RESET);
        let res_x = ins_x + clamp(ins_w - bw_reset);
        let rec_x = ins_x + clamp(ins_w - bw_reset - gap - bw_record);
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
        let mut mx = fld_x;
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
        let key_w = s(tok::SHORTCUT_COL).min(clamp(rec_x - gap - mx));
        // `cy` is the DROPPED-DOWN height here too, capped by the same
        // CB_SETMINVISIBLE(8) the App combo carries.
        place_h(ui.combo, mx, ly + edit_dy, key_w, field_h * 9);
        // Buttons honour `cy` and look right at the band height, so they take
        // `ctl` directly and sit on the band line rather than on the fields'
        // midline -- the same rule the command bar's three follow.
        place(IDC_RECORD, rec_x, ly, bw_record, ctl);
        place(IDC_RESET, res_x, ly, bw_reset, ctl);
        ly += ctl + gap;

        // Line 3: the notes, inside the card and beside what they describe.
        // Fixed height -- see `notes_height`. `notes_h` is recomputed here
        // (the same pure call `compute_card_rects` already made once, to size
        // card 2) rather than threaded through, because the value cannot
        // disagree between the two calls -- same `hwnd`, `ui`, `dpi` -- and
        // `card_rects`' interface is `[RECT; 4]`, not a richer struct.
        let notes_h = notes_height(hwnd, &ui, dpi);
        place_h(ui.notes, ins_x, ly, ins_w, notes_h);
    }

    // -- Card 3: the Keyboard page. A caption line, then ONE content line,
    // left to right: the check box, then `Hold` and its three chips, then
    // `Tap` and its combo. The only card behind that door today, and it keeps
    // its bottom anchor there rather than rising to the top -- see
    // `compute_card_rects` on the re-stack this landing does not do.
    //
    // Skipped off-page for the same reason cards 1 and 2 are, though the
    // stakes are lower: `IDC_TAP` is a `CBS_DROPDOWNLIST`, which has no edit
    // child for a resize to re-synchronise. Uniformity is the argument here,
    // not a second measured hazard.
    if keyboard {
        let kb_x = card3.left + card_pad;
        let kb_y = card3.top + card_pad;
        let kb_w = clamp(card3.right - card3.left - card_pad * 2);
        // `IDC_GRP_KEYBOARD` gets its own `s(24)` caption line, not the card's
        // whole interior -- same reclass, same reasoning as `IDC_GRP_EDITOR`
        // above. `kb_card_h` (in `compute_card_rects`) still budgets `s(24)`
        // for this line; only the control drawing it changed.
        place(IDC_GRP_KEYBOARD, kb_x, kb_y, kb_w, s(24));
        let inner_x = kb_x + gap;
        let ry = kb_y + s(24);
        // Every width on this line comes from the caption it has to hold.
        //
        // `IDC_CAPS` gets its OWN budget, not `glyph` (`glyph` stays exactly as
        // declared above -- the four modifier chips still need the old `s(24)`).
        // `paint::toggle` draws a 40 px track inset `off` (2 px) from the
        // control's own left edge -- so the focus ring it draws around the
        // track can grow outward without its left edge and arcs falling outside
        // `NM_CUSTOMDRAW`'s clip rect, see the track-rect comment in `paint.rs`
        // -- then `tok::GAP` (6 px) before the caption. `off`, the 40 px track
        // and `gap` are each their own `scale()` call in `paint.rs` (`off` and
        // `track_w` in the track-rect block, `gap` in the caption block), so a
        // single `s(50)` call here is provably never short of their sum: floor
        // is subadditive (`floor(a)+floor(b)+floor(c) <= floor(a+b+c)` for any
        // a/b/c >= 0) and monotone, and 2 + 40 + tok::GAP is 48, so
        // `s(2)+s(40)+gap <= s(48) <= s(50)` at every DPI.
        //
        // It is never short, and it is no longer tight. Re-derived at every
        // standard Windows scale step (100/125/150/175/200/225/250/300%, i.e.
        // dpi 96/120/144/168/192/216/240/288), `s(50)` exceeds the sum by
        // 2/3/3/4/4/5/5/6 px respectively. Nothing clips, because the slack
        // falls on the caption's side: `w_caps` is `tw(cap::CAPS)` plus this
        // budget, so `paint::toggle`'s `DrawTextW` box comes out 2-6 px WIDER
        // than the caption it holds and `DT_END_ELLIPSIS` never fires. What it
        // costs is looseness -- `IDC_CAPS`' rect ends 2-6 px past its own
        // caption, so `IDC_LBL_HOLD` sits that much further right than the
        // nominal `gap * 2` that separates the line's three sections.
        //
        // **CORRECTED 2026-08-14: this said "all eight land on equality, so
        // this does not over-allocate in practice either".** That was exact
        // while `tok::GAP` was 8 -- 2 + 40 + 8 == 50 is what made subadditivity
        // bind at equality, re-derived at all eight steps to check -- and the
        // compaction pass (`1f46335`) took `GAP` to 6 without moving the 50,
        // leaving 2 logical px of budget with nothing to spend it on. The
        // safety half of the old claim needed no rescue: a smaller `GAP` can
        // only widen the margin. Do not "fix" the looseness by dropping this to
        // `s(48)` without reading `paint.rs`'s copy of the same budget first --
        // the two are one number written twice, and the number went stale here
        // because only one copy was re-derived.
        //
        // The two STATICs below get a hair of slack instead of this budget, for
        // the reason the editor strip's labels do: SS_CENTERIMAGE clips rather
        // than wraps.
        let toggle_glyph = s(50);
        let w_caps = tw(cap::CAPS) + toggle_glyph;
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
        let mut kx = inner_x;
        place(IDC_CAPS, kx, ry, w_caps, ctl);
        kx += w_caps + gap * 2;
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
    }

    // -- System and About: one waiting line each, at the content origin.
    //
    // **`card0.top` IS the content origin on these two pages**, and reading it
    // back is what keeps this function from spelling
    // `strip_rect(rc, dpi).bottom + gap_card` a second time. The banner is
    // `BANNER_PAGE`-only (`banner_shown`), so behind these two doors
    // `compute_card_rects` never advances `y` past card 0 -- its rect is a
    // zero-height one AT the origin, which is exactly the number wanted here.
    // If the banner ever widens to every page again, this line follows it down
    // by itself instead of drawing under it.
    //
    // Inset by `card_pad` on the left and top, so the line begins where every
    // other string in the window begins rather than out at the card border.
    // Neither page has a card today; both will grow one, and a placeholder
    // that has to move sideways when it arrives is a placeholder standing in
    // the wrong place.
    //
    // No `else` and no clearing: the OTHER page's line is hidden by
    // `show_page_controls`, and leaving it wherever it was last placed is what
    // every other off-page control does.
    let waiting = match ui.page {
        Page::System => Some(IDC_SYS_PLACEHOLDER),
        Page::About => Some(IDC_ABOUT_PLACEHOLDER),
        Page::Shortcuts | Page::Keyboard => None,
    };
    if let Some(id) = waiting {
        place(
            id,
            cx + card_pad,
            card0.top + card_pad,
            clamp(cw - card_pad * 2),
            ctl,
        );
    }

    // -- The command bar. Save is the outermost button on the right, Close
    // inboard of it, `Open config file` hard left -- as far from Save as
    // the bar allows. Not a card; anchored at `bar_y`, same as before.
    let bw_open = btn(cap::OPEN_FILE);
    let bw_apply = btn(cap::SAVE);
    let bw_close = btn(cap::CLOSE);
    place(IDC_OPENFILE, cx, bar_y, bw_open, ctl);
    place(IDC_APPLY, cx + clamp(cw - bw_apply), bar_y, bw_apply, ctl);
    place(
        IDC_CLOSE,
        cx + clamp(cw - bw_apply - gap - bw_close),
        bar_y,
        bw_close,
        ctl,
    );
}
