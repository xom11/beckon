//! Where every control goes. Behaviour is unchanged by the split that created
//! this file; see the module header in `mod.rs` for the rules `layout` obeys,
//! and note the one that matters most: `layout` calls `SetWindowPos` on the
//! populated App combo, which is the measured data-loss path. Nothing may add
//! a new call site for it on a keystroke path.
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
/// - **`CTL` is 32, not the measured 22.** `BCM_GETIDEALSIZE` returns the
///   smallest box the theme can draw a caption in — a floor, not a layout
///   recommendation. The measurement's job was to prove 32 does not clip,
///   and it does not.
/// - **There is no list-row token.** 29 px measured at 144 DPI is 19.33 at
///   96, and a non-integer is the tell that comctl32 derives the row
///   height from the font at the live DPI. A 96-DPI token pushed through
///   `scale` would be wrong at every non-integer scale and would break
///   again the moment the font changes, so `list_row_height` asks the
///   control instead.
///
/// **`BAND` (14) is gone, replaced by `GAP_CARD` (12).** Every band-to-band
/// gap Task 8 leaves as a gap (banner-to-card, card-to-card) now separates
/// two cards that already carry their own `CARD_PAD` margin, so the gap
/// between them wants to be a little tighter than the old gap between bare
/// bands. `GAP` (8), the gap between two controls *inside* one band, is
/// unchanged — it is a different relationship and Task 8 does not touch it.
pub(super) mod tok {
    /// Surface padding — the margin between the client rect and the first
    /// card, and the last card and the client rect. Also the command bar's
    /// own margin, since it is not a card.
    pub const PAD: i32 = 16;
    /// Between the banner and card 1, and between two cards. Was `BAND`
    /// (14) before Task 8; see this module's own doc comment for why a
    /// smaller number is right once every band carries `CARD_PAD` of its
    /// own.
    pub const GAP_CARD: i32 = 12;
    /// Between two controls inside one band.
    pub const GAP: i32 = 8;
    /// A label and the control it names.
    pub const LABEL: i32 = 12;
    /// Height of one band line, and of every button on it.
    pub const CTL: i32 = 32;
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
    pub const CARD_PAD: i32 = 16;
    /// `RoundRect`'s corner radius, halved (the API wants the full ellipse
    /// width/height, i.e. `2 * CARD_RADIUS`).
    pub const CARD_RADIUS: i32 = 10;
    /// The list's row height, fed to `ImageList_Create` for the state image
    /// list a row's height is actually derived from.
    ///
    /// Unreachable until Task 10 wires it in — see that task's own Step 1.
    /// `#[allow(dead_code)]` here, named after the task that removes it, the
    /// same pattern `colorref` and `Role::Title` carried across Tasks 4/5
    /// and 5/7. Left as a token now rather than invented later because
    /// Task 8 is what sizes the list's card around `tok::ROWS`, and the two
    /// numbers belong beside each other.
    #[allow(dead_code)]
    pub const ROW_H: i32 = 26;
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
        }
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
    let card3 = card(kb_y, kb_card_h);

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

    // Offset by the client-drawn title bar (Task 7): `GetClientRect` now
    // includes that band -- `nccalcsize` gave it back to the client -- so
    // the first card has to start below it rather than draw underneath it.
    let mut y = pad + s(chrome::TITLEBAR_H);

    // -- Card 0: the banner. Contributes NO height when hidden -- `y` is
    // not advanced and the returned rect has zero height at that `y`.
    let card0 = if ui.external_change {
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
    let row_h = list_row_height(ui.list, dpi);
    let border = 2 * GetSystemMetricsForDpi(SM_CYBORDER, dpi);
    let want = list_header_height(ui.list, dpi) + row_h * tok::ROWS + border;
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
    // `card_pad` (16 px at 96 DPI) more room than the card can actually
    // afford, and card 2 draws exactly that far over card 3 -- worst with
    // the banner up, where there is no slack left to absorb it, because the
    // 76 px card 0 would otherwise have taken swallows a shortfall that
    // small with room to spare. Simulated, not seen: nothing on the
    // machine this was written on can display the window.
    let editor_min = card2_h;
    let room = clamp(kb_y - gap_card - list_top);
    let list_h = clamp(want.min(clamp(room - gap_card - card_pad - editor_min)));
    let card1_h = card_pad * 2 + ctl + gap + list_h;
    let card1 = card(y, card1_h);
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
    let card2 = card(y, card2_h);

    [card0, card1, card2, card3]
}

/// The four card rects, for `WM_PAINT` -- see `compute_card_rects` for the
/// arithmetic, which is the ONE this function and `layout` both run.
///
/// Takes its own one-time `UI` borrow, dropped on this line, independent of
/// `layout`'s: it is called from a different place (`WM_PAINT`) at a
/// different time, so sharing the arithmetic does not mean sharing a
/// borrow, and this function must obey the same "ONE borrow, dropped
/// immediately" rule on its own.
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
/// **The LIST is the one thing that flexes.** See `compute_card_rects`'s
/// own comment on why, and on `editor_min`/`room`/`y.min(kb_y)` — that
/// arithmetic lives there now, not here; this function only reads
/// `card1`'s already-resolved height back out.
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
    let mut arc = RECT::default();
    let combo_h = if GetWindowRect(ui.app, &mut arc).is_ok() {
        let ah = arc.bottom - arc.top;
        if ah > 0 && ah < ctl && ah >= text_h + s(2) {
            Some(ah)
        } else {
            None
        }
    } else {
        None
    };
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

    // -- Card 0: the banner. Contributes NO height when hidden.
    if ui.external_change {
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
    // **This `GetClientRect` is `layout`'s fifth input, and the ONE the
    // `apply_state` guard does not track.** When a scroll bar is up the list
    // reports `C - SB`, so the columns get `C - 2*SB`; drop back under the
    // page size and the client returns to `C` while the columns keep the
    // narrower figure until the next resize, DPI change or banner flip --
    // roughly a 34 px gutter at 96 DPI, 52 at 150 %.
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
    // alongside the other two guards, NOT a wider `layout`.
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
    place_h(ui.app, fld_x, ly + edit_dy, fld_w, field_h * 9);
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

    // -- Card 3: the keyboard card. A caption line, then ONE content line,
    // left to right: the check box, then `Hold` and its three chips, then
    // `Tap` and its combo.
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
    // `glyph` -- the check box's own square plus the gap before its caption
    // -- is declared above, which sizes the editor card's four modifier
    // chips by the same rule. The two STATICs get a hair of slack instead,
    // for the reason the editor strip's labels do: SS_CENTERIMAGE clips
    // rather than wraps.
    let w_caps = tw(cap::CAPS) + glyph;
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
