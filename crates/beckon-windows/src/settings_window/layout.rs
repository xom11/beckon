//! Where every control goes. Behaviour is unchanged by the split that created
//! this file; see the module header in `mod.rs` for the rules `layout` obeys,
//! and note the one that matters most: `layout` calls `SetWindowPos` on the
//! populated App combo, which is the measured data-loss path. Nothing may add
//! a new call site for it on a keystroke path.

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
pub(super) mod tok {
    /// Surface padding — the margin between the client rect and content.
    pub const PAD: i32 = 16;
    /// Between two bands.
    pub const BAND: i32 = 14;
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
    ///
    /// The key list used to have a token of its own (`KEY_COL`, 140),
    /// derived rather than designed: band 4 was one line, the App combo
    /// absorbed whatever the other six controls left, and 60 px had to come
    /// from somewhere to pay for `Record` and `Reset` sharing that line.
    /// With App on a line of its own there is nothing left to starve, so the
    /// key list is back under this ceiling and the arithmetic is retired.
    pub const SHORTCUT_COL: i32 = 200;
    /// A modifier chip is never narrower than this, nor than its own caption
    /// plus `glyph` -- direction B's `.wtog { min-width:46px }`.
    ///
    /// It exists because `Alt` is three characters and `Shift` is five, and
    /// a row of keys whose widths follow the length of their letters does not
    /// read as a keyboard. Only `Alt` is actually short enough to hit the
    /// floor at 96 DPI; the other six size themselves.
    pub const CHIP_MIN: i32 = 46;
    /// List rows visible without scrolling.
    pub const ROWS: i32 = 8;
    /// Widest a tooltip may draw before it wraps. Comfortably narrower than
    /// `MIN_WIDTH`, so the balloon never overhangs the window that owns it,
    /// and wide enough that an ordinary `%APPDATA%` config path takes two
    /// lines rather than six.
    pub const TOOLTIP_MAX: i32 = 420;
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

/// Seven horizontal bands, top to bottom: the external-change banner (no
/// height when hidden), the section head, the list, the editor group, the
/// suggestion row (no control, no height, in this landing), the keyboard
/// group and the command bar.
///
/// Everything is placed from the client rect at the current DPI, so a
/// 150 % display is not an afterthought — `GetDpiForWindow` scales the
/// tokens rather than the tokens assuming 96.
///
/// **Vertical shape.** The command bar is anchored to the bottom and the
/// keyboard group sits directly above it; the top bands stack downward.
///
/// **The LIST is the one thing that flexes.** It wants `header + 8 rows` and
/// gives that up rather than let anything overlap when the window is short —
/// a shrunk list scrolls, an overlapped control is unreachable. Everything
/// below it is fixed: band 4 is a group box of a computed height (`grp_h`),
/// which band 3 reserves through `editor_min` before choosing its own.
///
/// The notes STATIC used to be the flexing band instead, which is what made
/// it a 1220x177 control holding one 258 px line at the default size. It is
/// now a fixed line inside the editor group — see `notes_height` — so a
/// vertical resize lands on the list, and once the list is at its full 8 rows
/// the surplus is simply slack above the keyboard group.
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
    let band = s(tok::BAND);
    let gap = s(tok::GAP);
    let lblgap = s(tok::LABEL);
    let ctl = s(tok::CTL);

    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let cx = pad;
    let cw = clamp(w - pad * 2);

    // Body, and only Body: every string measured in this function labels or
    // captions a Body control -- the three command-bar buttons, Add /
    // Remove / Reload / Keep mine, the two field labels, the whole keyboard
    // row, and the "Ag" that sizes the EDIT. The `Shortcuts` heading is the one
    // Subtitle in the window and its width is never measured; it takes
    // whatever Add and Remove leave it.
    //
    // Measured through `shown`, so a caption's `&` -- a mnemonic marker,
    // which is not drawn -- does not buy the control a character of width it
    // will never use.
    let tw = |t: &str| text_size(hwnd, ui.fonts.get(Role::Body), dpi, &shown(t)).0;
    let btn = |t: &str| s(tok::BTN).max(tw(t) + s(24));

    let place = |id: i32, x: i32, y: i32, cxx: i32, cy: i32| {
        if let Ok(c) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowPos(c, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    };
    let place_h = |h_: HWND, x: i32, y: i32, cxx: i32, cy: i32| {
        let _ = SetWindowPos(h_, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
    };

    // The two bottom bands are anchored, not stacked, so the window's
    // bottom edge is where they stay however tall the content above is.
    let bar_y = clamp(h - pad - ctl);
    // Caption inset, ONE control line, then a bottom inset the same size as
    // the gap -- the shape band 4's `grp_h` follows too, so the two group
    // boxes in this window are one rule. It was two lines while the group
    // held a check box over three radios; the Caps row is one line, so the
    // group is one `ctl + gap` shorter. Those pixels used to go to the
    // flexing notes band; now they raise `kb_y`, which is what band 3 sizes
    // the list against.
    let kb_h = s(24) + ctl + gap;
    let kb_y = clamp(bar_y - band - kb_h);

    let mut y = pad;

    // Field geometry, computed before band 2 because the filter box needs it
    // there and the editor strip needs it in band 4. `combo_h` is therefore
    // read BEFORE the combo is placed this pass, i.e. it is the height the
    // combo had on the PREVIOUS pass. That is sound: the value is the theme's
    // choice for a font and a DPI, so it moves only on WM_DPICHANGED or a
    // font change, both of which run `layout` again immediately. The one pass
    // that can read a not-yet-snapped height is the first, and the floor
    // below falls back to the font-derived height there.
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
    // which is why it is computed here rather than in a band: `IDC_CAPS` is
    // a real `BS_AUTOCHECKBOX` and this is its square plus the gap before
    // its text, while for the seven keycap chips it is the padding around
    // the letters -- `.wtog { padding:0 10px }` plus a hair, since
    // `draw_keycaps` fills whatever width the chip control is given.
    let glyph = s(24);
    // One chip's width: its caption plus that slack, never below
    // `tok::CHIP_MIN`. Both chip rows go through this, so the four modifier
    // chips and the three `Hold` chips cannot drift apart -- and `draw_chip`
    // fills whatever width it is given, so this closure alone decides how
    // big a key is.
    let chip = |c: &str| (tw(c) + glyph).max(s(tok::CHIP_MIN));

    // -- Band 1: the banner. Contributes NO height when hidden.
    if ui.external_change {
        let bw_reload = btn(cap::RELOAD);
        let bw_keep = btn(cap::KEEP_MINE);
        let buttons = bw_reload + gap + bw_keep;
        place_h(ui.banner, cx, y, clamp(cw - buttons - gap), ctl);
        place_h(ui.reload, cx + clamp(cw - buttons), y, bw_reload, ctl);
        place_h(ui.keep, cx + clamp(cw - bw_keep), y, bw_keep, ctl);
        y += ctl + band;
    }

    // -- Band 2: the section head. `Shortcuts` leading, then the filter,
    // then Remove and Add right-aligned.
    let bw_add = btn(cap::ADD);
    let bw_remove = btn(cap::REMOVE);
    // Capped at a third of the width, the same ceiling band 4 puts on the
    // key list, so the boxes narrow together. The HEADING
    // takes what is left, which makes it -- not the filter -- the first
    // thing to run out. At 96 DPI, with `Add`/`Remove` both pinned to
    // `tok::BTN` (88 px -- neither caption needs more), `heading_w` reduces
    // to `clamp(cw - 200 - filter_w)`. Below `cw = 600` (where `filter_w` is
    // itself `floor(cw / 3)`, not the 200 px cap) that clamps to zero at
    // `cw = 300` -- the raw client width `w` this comes from, before the
    // PAD margins, is 332. `MIN_WIDTH` is a WINDOW floor, not a `cw` one:
    // `WM_GETMINMAXINFO` sets `ptMinTrackSize.x`, which bounds the whole
    // window including the OS's own frame, so its 720 does not translate
    // into an exact `cw` this file computes -- only into "hundreds of
    // pixels clear of the 332 px zero point under any frame the OS adds,"
    // which is the actual reason a drag can never reach it. Every
    // subtraction is clamped, so the intermediate rects `WM_DPICHANGED` can
    // suggest below that floor produce a hidden heading rather than a
    // negative width.
    let filter_w = s(tok::SHORTCUT_COL).min(clamp(cw / 3));
    let filter_x = cx + clamp(cw - bw_add - gap - bw_remove - gap - filter_w);
    place(IDC_ADD, cx + clamp(cw - bw_add), y, bw_add, ctl);
    place(
        IDC_REMOVE,
        cx + clamp(cw - bw_add - gap - bw_remove),
        y,
        bw_remove,
        ctl,
    );
    place_h(ui.filter, filter_x, y + edit_dy, filter_w, edit_h);
    // **The heading is measured now**, where it never used to be: it shares
    // its line with the count, so it has to end somewhere definite rather
    // than taking everything up to the filter. Measured in SUBTITLE -- the
    // only string in `layout` that is not Body, and `tw` would under-measure
    // it by a third and put the count on top of it.
    //
    // The count keeps the leftover, so the heading is still the last thing to
    // run out and a narrow window clips the count first -- which is the right
    // order: `Shortcuts` names the band, `· 18 bindings` decorates it.
    let head_w = text_size(hwnd, ui.fonts.get(Role::Subtitle), dpi, "Shortcuts").0 + s(4);
    let head_w = head_w.min(clamp(filter_x - gap - cx));
    place(IDC_LBL_SECTION, cx, y, head_w, ctl);
    place(
        IDC_LBL_COUNT,
        cx + head_w + lblgap,
        y,
        clamp(filter_x - gap - (cx + head_w + lblgap)),
        ctl,
    );
    // A control gap, not a band gap: the head labels the list directly
    // below it, so the two read as one group.
    y += ctl + gap;

    // Band 4's height, computed HERE because band 3 has to yield to it and
    // the two must not each hold an opinion about how tall the editor is --
    // the same reason `glyph` is computed above band 1 rather than twice.
    //
    // Caption inset, two content lines, the notes, then a bottom inset the
    // size of the gap: the same shape band 6's `kb_h` uses, so the two group
    // boxes in this window are one rule. Fixed, not flexing -- see
    // `notes_height`.
    let notes_h = notes_height(hwnd, &ui, dpi);
    let grp_h = s(24) + ctl + gap + ctl + gap + notes_h + gap;

    // -- Band 3: the list.
    let row_h = list_row_height(ui.list, dpi);
    // `want` is a WINDOW height (it feeds SetWindowPos below), but the list
    // carries WS_BORDER, so its client height -- where header_height + 8
    // rows actually get drawn -- is 2*SM_CYBORDER less than that. Without
    // this the 8th row was clipped by the border and comctl32 drew a sliver
    // of a 9th.
    let border = 2 * GetSystemMetricsForDpi(SM_CYBORDER, dpi);
    let want = list_header_height(ui.list, dpi) + row_h * tok::ROWS + border;
    // The editor below is a fixed-height GROUP now, not a two-line strip, so
    // the figure the list must yield to is the whole of `grp_h` -- caption
    // inset, both lines, the notes and the bottom inset. It used to be
    // `ctl + gap + ctl`, which was right when the notes flexed into whatever
    // was left; against a fixed group that under-reserves by the caption
    // inset plus a line, and the group draws over the keyboard group instead
    // of the list giving up a row. **This is the guard**: `y.min(kb_y)` at
    // the end of band 4 cannot help, because `grp_y` is bound before the
    // group is placed and clamping `y` afterwards moves nothing.
    let editor_min = grp_h;
    let room = clamp(kb_y - band - y);
    let list_h = clamp(want.min(clamp(room - band - editor_min)));
    place_h(ui.list, cx, y, cw, list_h);

    // Columns, sized from the list's OWN client width now that it has one,
    // minus a vertical scroll bar's width whether or not one is showing.
    // That subtraction is what makes overflow structurally impossible: a
    // scroll bar appearing later steals client width the columns have
    // already been told not to use. Measured before this change: 561 px of
    // columns inside a 482 px list, i.e. a horizontal scroll bar shipped.
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
    y += list_h + band;
    // `list_h` clamps to 0 when `room` itself clamped negative -- reachable
    // only by an intermediate resize below MIN_HEIGHT that WM_DPICHANGED's
    // suggested rect can hand us without asking WM_GETMINMAXINFO (dragging
    // can't reach it; a 0x0 client rect clamps everything to 0 and is fine).
    // In that state `y` here can still land past `kb_y`, and this line is
    // what stops it: band 4 reads `y` straight into `grp_y`, so this is the
    // editor group box's TOP edge, and band 4's height is the fixed `grp_h`
    // -- not something `clamp` shrinks the way it shrinks `list_h` above.
    //
    // **It bounds the top, and only the top.** What keeps the group's BOTTOM
    // off `kb_y` at and above `MIN_HEIGHT` is `editor_min` above, which
    // reserves the whole of `grp_h` before the list takes any height at all.
    // This line cannot do that job -- it runs before `grp_h` is added, so it
    // can pin `grp_y` to `kb_y` but never pull the group's bottom back up.
    // Drop `editor_min` back to the `ctl + gap + ctl` the one-line strip used
    // and this line will not save you: simulated at `MIN_HEIGHT` itself, the
    // group draws 16 px over the keyboard group with the banner down and
    // 62 px with it up (22 / 91 at 150 %; re-simulated for Task 9's floor of
    // 550 -- `notes_height`'s real body costs 4 px more than the stub these
    // figures were first taken against). That is the ordinary minimum drag
    // size, not a sub-floor `WM_DPICHANGED` edge case.
    y = y.min(kb_y);

    // -- Band 4: the editor group. TWO lines inside a titled BS_GROUPBOX,
    // then the notes on a third line inside the same group.
    //
    //   +- Editing "Windows Terminal" ----------------------------------+
    //   |  App       [ ..................................... v ]        |
    //   |  Shortcut  [ ]Ctrl [ ]Win [ ]Alt [ ]Shift [ key v ]  [R] [R]  |
    //   |  ok  Registered. Press Ctrl + Win + Alt + T to focus it.      |
    //   +---------------------------------------------------------------+
    //
    // **App gets a line of its own, and that is the whole point.** On one
    // line it was the control that absorbed whatever the other six left --
    // about 209 px at 860, and ~59 px at MIN_WIDTH. Two derived tokens
    // (`tok::KEY_COL`, `tok::BTN_SM`) existed only to keep that figure above
    // zero, and Task 7 retires both.
    //
    // Bound once, and named rather than left as `y`, because the group's top
    // edge is now a coordinate other controls are placed against -- the
    // empty-state STATIC is the next reader. `y` is a running cursor that
    // three bands above have already moved and a fourth may yet move; this is
    // a fixed point, and the two must not be spelled the same.
    let grp_y = y;
    let grp_x = cx;
    let grp_w = cw;
    // Caption inset, then the content, then a bottom inset the size of the
    // gap -- `grp_h` itself is computed above band 3, which has to yield its
    // own height to it.
    let ins_x = grp_x + gap;
    let ins_w = clamp(grp_w - gap * 2);
    place(IDC_GRP_EDITOR, grp_x, grp_y, grp_w, grp_h);

    // Both lines share one label column, so `App` and `Shortcut` left-align
    // with each other instead of each starting wherever its own line does.
    //
    // A hair of slack past the measured width: a STATIC clips to its rect,
    // and SS_CENTERIMAGE clips harder because it also refuses to wrap.
    let lw_lbl = tw("Shortcut").max(tw("App")) + s(4);
    let fld_x = ins_x + lw_lbl + lblgap;
    let fld_w = clamp(ins_x + ins_w - fld_x);

    // Line 1: App, full width.
    let mut ly = grp_y + s(24);
    place(IDC_LBL_APP, ins_x, ly, lw_lbl, ctl);
    // A COMBOBOX's `cy` is the height of its DROPPED-DOWN list, not of the
    // closed control -- and under comctl32 v6 even that is capped by
    // `build_children`'s CB_SETMINVISIBLE(8). The closed height is the
    // system's to choose from the font, which is why `combo_h` above asks
    // what it took rather than guessing a chrome delta the next font change
    // would invalidate.
    //
    // A single-line EDIT draws its text at the TOP of its client rect --
    // Win32 gives it no vertical centring at all -- so the fields are centred
    // within their line (`edit_dy`) rather than stretched to it, and take the
    // height the COMBOBOX's theme picked. `field_h` is what the font alone
    // justifies, and remains the fallback for when the combo cannot be
    // measured -- plus the unit of the dropped-down list's height.
    place_h(ui.app, fld_x, ly + edit_dy, fld_w, field_h * 9);
    ly += ctl + gap;

    // Line 2: the shortcut. Chips left, then the key list, then the two
    // commands right-aligned -- the same "commands close the line" rule band
    // 2's Add/Remove follow.
    place(IDC_LBL_SHORTCUT, ins_x, ly, lw_lbl, ctl);
    // Sized from `RECORD`, never from `STOP`: the armed caption is the
    // narrower of the two, so a caption flip cannot clip and `layout` never
    // has to run on the capture path -- which matters, because `layout` means
    // `SetWindowPos` on the populated App combo, the measured data-loss call
    // (`Ui::shown_external`).
    let bw_record = btn(cap::RECORD);
    let bw_reset = btn(cap::RESET);
    let res_x = ins_x + clamp(ins_w - bw_reset);
    let rec_x = ins_x + clamp(ins_w - bw_reset - gap - bw_record);
    // Each chip is its caption plus `glyph`, floored at `tok::CHIP_MIN` --
    // exactly as band 6's `Hold` chips are sized, same two constants, one
    // rule. `chip` is declared above band 4 so both rows read from it.
    let w_mod_ctrl = chip(cap::MOD_CTRL);
    let w_mod_win = chip(cap::MOD_WIN);
    let w_mod_alt = chip(cap::MOD_ALT);
    let w_mod_shift = chip(cap::MOD_SHIFT);
    // Chips and key list share the fields' midline (`edit_dy`) and their
    // height, so App, the key list and the filter are ONE box repeated
    // rather than three boxes that happen to be concentric. Measured at
    // 144 DPI before the fields were unified: EDIT 43 px against the
    // combo's 36, centres agreeing to within half a pixel -- which reads as
    // a mistake rather than as a pair. `draw_chip` centres its keycap inside
    // whatever rect the chip is given, both ways, so `edit_h` needs no
    // separate rule for the four of them.
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
    // the shortcut column's ceiling. It no longer needs a token of its own:
    // with App on line 1 there is nothing left on this line for it to starve.
    //
    // **Where line 2 runs out.** The key list is now what this line leaves
    // over, so it is the figure worth writing down -- the role `app_w` used
    // to play. At 96 DPI, with `Record` and `Reset` both pinned to `tok::BTN`
    // (88 px; neither caption needs more, which is what makes `tok::BTN_SM`
    // redundant), the fixed part of the line is
    //
    //   lw_lbl(~54) + lblgap(12) + four chips(~190) + 6*gap(48)
    //     + bw_record(88) + bw_reset(88)   =   ~480 px of `ins_w`
    //
    // -- SIX gaps, not five: three between the chips, one after `Shift`, one
    // between the key list and `Record`, one between the two commands. The
    // chip figure is the four `chip()` widths, i.e. `4 * glyph` (96 px) plus
    // the four measured captions, except that `Alt` is short enough to take
    // `tok::CHIP_MIN` instead; `lw_lbl` is `tw("Shortcut") + 4`, the wider of
    // the two labels.
    //
    // `ins_w` is `cw - 2*gap` and `cw` is `w - 2*pad`, so below its ceiling
    // this whole expression collapses to `key_w = min(200, w - 528)` -- the
    // key list clamps to zero at a raw client `w` of ~528, and above that it
    // IS the margin over that zero point. One number, read two ways.
    //
    // `MIN_WIDTH` is a WINDOW floor, not a `cw` one: `WM_GETMINMAXINFO` sets
    // `ptMinTrackSize.x`, which bounds the whole window including the OS's
    // own frame, so its 720 does not translate into an exact `cw` this file
    // computes. Compare like for like -- client against client -- and a
    // 720 px window with a 16 px frame gives `w = 704`, i.e. ~176 px clear of
    // the zero point, which is why a drag cannot reach it. **That margin is a
    // ceiling, not a floor**: a wider OS frame leaves a narrower client, so
    // the figure only ever falls from 185. (Do not compare the 720 against
    // the 519 directly for a "~200 px" margin -- that is the window-against-
    // client mistake this paragraph exists to avoid.)
    //
    // Concretely at `MIN_WIDTH`, then: the key list is 176 px of its 200 px
    // ceiling -- the same 176, necessarily, by the collapse above -- and the
    // App combo on line 1 has ~590 px, where the old one-line strip left it
    // 59. Every subtraction here is clamped regardless, because
    // `WM_DPICHANGED` can suggest a rect below that floor without asking
    // `WM_GETMINMAXINFO`.
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

    // Line 3: the notes, inside the group and beside what they describe.
    // Fixed height -- see `notes_height`. It used to take every pixel down to
    // the keyboard group, which measured as a 1220x177 control holding one
    // 258 px line.
    place_h(ui.notes, ins_x, ly, ins_w, notes_h);

    // **`y` deliberately stops here.** Band 5 has no control and bands 6 and
    // 7 are anchored to `kb_y` / `bar_y`, so the `y += grp_h + band` that
    // would close this band is a store nothing reads -- and the compiler says
    // so (`unused_assignments`). Restore it, with its `y.min(kb_y)` guard, the
    // moment band 5 grows a control.
    //
    // The guard THAT deleted line used to carry -- not band 3's surviving
    // `y.min(kb_y)`, which still bounds `grp_y` and is still needed -- has
    // moved to band 3's `editor_min`. That is the only place it can still do
    // anything: `grp_y` is read before the group is placed, so clamping `y`
    // afterwards moves nothing.

    // -- Band 5: the suggestion row. No control, no height.

    // -- Band 6: the keyboard group. ONE line, left to right: the check box,
    // then `Hold` and its three chips, then `Tap` and its combo.
    place(IDC_GRP_KEYBOARD, cx, kb_y, cw, kb_h);
    let inner_x = cx + gap;
    let ry = kb_y + s(24);
    // Every width on this line comes from the caption it has to hold. The
    // s(190)/s(70)/s(90) constants the radios used were sized for one font
    // at one DPI and clipped the moment either changed.
    //
    // `glyph` -- the check box's own square plus the gap before its caption
    // -- is declared above band 4, which sizes its four modifier chips by
    // the same rule. The two STATICs get a hair of slack instead, for the
    // reason the editor strip's labels do: SS_CENTERIMAGE clips rather than
    // wraps.
    let w_caps = tw(cap::CAPS) + glyph;
    let w_hold = tw(cap::HOLD) + s(4);
    let w_ctrl = chip(cap::HOLD_CTRL);
    let w_win = chip(cap::HOLD_WIN);
    let w_alt = chip(cap::HOLD_ALT);
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
    // and the key list take, so every box in the window narrows
    // together. Clamped like every other subtraction here: a window dragged
    // narrow must produce a combo with no width, never a negative one.
    //
    // The `cy` is the DROPPED-DOWN height, not the closed one -- see the App
    // combo in band 4. Three items need far less than the eight that combo
    // asks for, so there is no CB_SETMINVISIBLE to go with it.
    let tap_w = s(tok::SHORTCUT_COL).min(clamp(cx + cw - gap - kx));
    place(IDC_TAP, kx, ry + edit_dy, tap_w, field_h * 5);

    // -- Band 7: the command bar. Save is the outermost button on the right,
    // Close inboard of it, `Open config file` hard left -- as far from Save
    // as the bar allows.
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
