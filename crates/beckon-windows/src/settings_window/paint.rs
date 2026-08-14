//! Drawing. Behaviour is unchanged by the split that created this file --
//! every hazard comment below travelled with its code. Message *dispatch*
//! (`WM_DRAWITEM`, `WM_NOTIFY` / `NM_CUSTOMDRAW`) stays in `mod.rs`; this
//! file holds only the painters those handlers call into.

// GetSysColorBrush must not appear in this file. It returns a brush owned by
// Windows, while ThemeCache::brush returns one owned by us; a call site that
// cannot tell them apart is a double-free waiting for a theme switch. Every
// colour goes through `col`, which answers the high-contrast branch itself.

use super::*;
// `ThemeCache` itself is never glob-imported into `mod.rs` -- every call
// site there spells the qualified `theme::ThemeCache` (see `PAINT_THEME`'s
// own declaration), so `use super::*` does not carry the bare name across.
// Named here for the same reason `chrome.rs` names it: `button`, `colours`,
// `field_border` and `draw_combo_item` all take `&mut ThemeCache` directly,
// the same convention `chrome::paint` uses and for the same reason -- see
// `button`'s own doc comment.
use super::theme::ThemeCache;

/// A card: rounded fill plus a 1 px border. No drop shadow -- Win11's own
/// cards use a border, and a GDI shadow costs a layered surface for an
/// effect nobody asked for.
///
/// **Reads the theme through `theme_col` / `theme_brush`, not a
/// `ThemeCache` parameter.** Every other painter in this file does the
/// same -- see the `GetSysColorBrush` ban at the top of it -- and it is
/// what lets `WM_PAINT` call this once per card without pre-borrowing
/// `PAINT_THEME` itself: each call takes and drops its own borrow, so four
/// calls in a row cannot collide with each other or with a borrow a
/// sibling call (`chrome::paint`) is holding, as long as none of them are
/// nested inside one another. `card_rects(hwnd)` (in `layout.rs`) is the
/// matching geometry -- the same arithmetic `layout` places controls
/// against, so the two cannot drift apart.
pub(super) unsafe fn card(hdc: HDC, rc: RECT, dpi: u32) {
    let r = tok::CARD_RADIUS * dpi as i32 / 96;
    let fill = theme_col(|p| p.card, COLOR_WINDOW);
    let edge = theme_col(|p| p.card_border, COLOR_BTNSHADOW);
    let br = theme_brush(fill);
    // Scaled like `r` on the line above, not a bare `1`. This border is the
    // ONLY thing separating a card from the window ground it sits on
    // (`card_border`/`bg` clears the 1.2 non-text floor by a hairline
    // itself), so at 200% a fixed 1-device-pixel pen renders as a fraction
    // of a logical pixel -- effectively invisible -- while the rest of the
    // card scales up around it.
    let pen = CreatePen(PS_SOLID, scale(1, dpi).max(1), edge);
    let old_br = SelectObject(hdc, HGDIOBJ(br.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    // RoundRect strokes with the pen AND fills with the brush in one call,
    // so the border lands exactly on the fill's edge with no seam.
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r * 2, r * 2);
    if !old_br.is_invalid() {
        SelectObject(hdc, old_br);
    }
    if !old_pen.is_invalid() {
        SelectObject(hdc, old_pen);
    }
    // The pen is ours; the brush belongs to the cache (`theme_brush`) and
    // must NOT be deleted here.
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

/// Lay a run of keycaps out inside `cell`, or report that it does not fit.
///
/// Two callers, and `style` says which: the **Shortcut column**, where this
/// is why the config's own spelling never reaches the screen -- the user
/// pressed three physical keys, so the column draws three keys, and `super`
/// is a valid TOML token and a word on no keyboard -- and the **seven toggle
/// chips**, which pass one cap each.
///
/// **Returns `false` when the caps do not fit, and the caller falls back to
/// the display string with an ellipsis.** That fallback is structural rather
/// than defensive: `tok::SHORTCUT_COL` is 200 px but `layout` caps the column
/// at `inner / 2`, and a five-modifier chord, a narrow window and a high DPI
/// each reach the limit on their own. A clipped keycap reads as a rendering
/// fault; an ellipsis reads as a narrow column.
///
/// **Every colour comes from `col`.** A literal would fight the row
/// highlight four pixels away and would be the first crack in this window's
/// theme; `col(|p| p.accent_fill, COLOR_HIGHLIGHT)` is the user's own accent
/// in Light/Dark and is already right in high contrast, since `col` falls
/// back to `GetSysColor` there on its own. `hc` changes the *shape* only --
/// see `high_contrast`.
unsafe fn draw_keycaps(
    hdc: HDC,
    cell: RECT,
    caps: &[String],
    font: HFONT,
    dpi: u32,
    hc: bool,
    style: CapStyle,
) -> bool {
    if caps.is_empty() {
        return false;
    }
    let toggle = matches!(style, CapStyle::Toggle { .. });
    // **Two sets of metrics, from direction B's own two rules.** The board
    // gives the column cap (`.wcap`) `height:19px; padding:0 5px` and the
    // toggle chip (`.wtog`) `height:28px; padding:0 10px; min-width:46px` --
    // a chip is a key you press, a column cap is a key you read, and sizing
    // both from the column's numbers is what made the shipped chips look
    // like small grey buttons. A chip therefore takes its control's whole
    // height rather than the column's 19 px ceiling.
    let pad = scale(if toggle { 10 } else { 5 }, dpi);
    let gap = scale(3, dpi);
    let inset = scale(if toggle { 2 } else { 4 }, dpi);
    let row_h = cell.bottom - cell.top;
    let cap_h = if toggle {
        (row_h - inset * 2).max(scale(16, dpi))
    } else {
        (row_h - scale(6, dpi))
            .min(scale(19, dpi))
            .max(scale(12, dpi))
    };
    // The bottom edge, and the whole reason a box reads as a key. `.wcap` and
    // `.wtog` both carry `border-bottom:2px` against a 1 px everywhere else.
    let edge_h = scale(2, dpi).max(1);

    let prev_font = SelectObject(hdc, HGDIOBJ(font.0));

    // Measure the whole set before drawing any of it: the fallback is a
    // decision about the set, not something to discover halfway along it with
    // two caps already on screen.
    let mut widths = Vec::with_capacity(caps.len());
    let mut total = gap * (caps.len() as i32 - 1);
    for c in caps {
        // **Measured through `shown` for a chip and verbatim for a cell**,
        // which is the same split `layout`'s `tw` already makes. A chip's
        // caption carries a mnemonic marker -- `C&trl` -- and the `&` is not
        // drawn, so a cap sized for it is a cap one character too wide. A
        // cell's text is data and has no mnemonic to strip.
        let m = if toggle { shown(c) } else { c.clone() };
        let t = wide(&m);
        let mut sz = SIZE::default();
        // `wide` appends a NUL and this API takes a length, so the NUL would
        // be measured as a character -- same rule as `text_size`.
        let w = if GetTextExtentPoint32W(hdc, &t[..t.len() - 1], &mut sz).as_bool() {
            sz.cx + pad * 2
        } else {
            scale(8, dpi) * m.chars().count() as i32 + pad * 2
        };
        total += w;
        widths.push(w);
    }
    let room = cell.right - cell.left - inset * 2;
    if total > room {
        if !prev_font.is_invalid() {
            SelectObject(hdc, prev_font);
        }
        return false;
    }
    // **A chip is one cap and it owns its control, so it takes the whole
    // width** instead of shrinking to its caption. `min-width:46px` on
    // `.wtog` says the same thing in CSS: a row of keys whose sizes follow
    // the length of their letters does not read as a keyboard. `layout`
    // already floors each chip's control at `tok::CHIP_MIN`, so this is
    // where that floor becomes visible.
    //
    // Sized independently of the text, which is also what keeps a chip from
    // resizing when it is toggled -- the measurement above is now only the
    // fit test.
    if toggle {
        total = room;
        if let Some(w) = widths.first_mut() {
            *w = room;
        }
    }

    // **A pressed key goes DOWN**: one pixel, and no bottom edge. That is the
    // whole effect, and it is the ONLY click feedback these chips have --
    // Windows draws none of its own for an owner-draw button, so without it a
    // chip held under the mouse looks identical to one that is not.
    let press = match style {
        CapStyle::Toggle { pressed: true, .. } => scale(1, dpi),
        _ => 0,
    };
    let top = cell.top + (row_h - cap_h) / 2 + press;
    // Where the run starts. A chip owns its whole control rect and centres in
    // it; a cell is one column of many rows, and those line up down the
    // column, so a chord starts at a fixed inset instead.
    let mut x = match style {
        CapStyle::Chord => cell.left + inset,
        CapStyle::Toggle { .. } => cell.left + ((cell.right - cell.left) - total) / 2,
    };
    // **Every colour comes from `col`, or is derived from one.** An armed
    // chip's face is `col(|p| p.accent_fill, COLOR_HIGHLIGHT)` -- the user's
    // own accent in Light/Dark, already correct in high contrast because
    // `col` falls back to `GetSysColor` there on its own -- and its edge is
    // that same colour through `shade`. Direction B's `#2563eb` / `#1d4fc4`
    // pair is what the ratio was read off, not a colour to hard-code.
    let armed_face = theme_col(|p| p.accent_fill, COLOR_HIGHLIGHT);
    let (edge_col, border_col) = match style {
        _ if hc => {
            let c = theme_col(|p| p.text, COLOR_WINDOWTEXT);
            (c, c)
        }
        CapStyle::Toggle { armed: true, .. } => {
            let e = shade(armed_face, 4, 5);
            (e, e)
        }
        // A disabled chip keeps its shape and its depth and loses only its
        // ink -- see the face table below for why it does not also keep the
        // light face. Its edge takes `text_faint`, the same muted tone every
        // OTHER disabled element in this window uses -- not `keycap_edge`,
        // which is tuned to nearly vanish into a dark-mode keycap on purpose
        // (it is a shadow, not an outline) and would say nothing about being
        // disabled there.
        CapStyle::Toggle { disabled: true, .. } => {
            let c = theme_col(|p| p.text_faint, COLOR_BTNSHADOW);
            (c, c)
        }
        _ => {
            let c = theme_col(|p| p.keycap_edge, COLOR_BTNSHADOW);
            (c, c)
        }
    };
    let pen = CreatePen(PS_SOLID, 1, border_col);
    let prev_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    SetBkMode(hdc, TRANSPARENT);
    // **A chip's resting face is `keycap`, not the window's own `bg`**, and
    // that one substitution is most of why the shipped chips disappeared:
    // `bg` IS the window's own background, so an unarmed chip was a grey box
    // on a grey surface with only a hairline to prove it existed. Direction
    // B puts `--w-cap:#fafafa` on a `--w-bg:#f3f3f3` window -- the key is
    // LIGHTER than what it sits on, which is how a physical keycap catches
    // light. `keycap` is its own palette token rather than a re-use of
    // `card`, because in dark mode the two differ on purpose --
    // `DARK.keycap` is deliberately the lighter of the two, which is what
    // keeps the "catches light" effect true there as well.
    //
    // **Greyed outranks armed, and a disabled chip keeps the window's own
    // `bg`.** The light face is what makes an OPERABLE key stand off the
    // surface, so giving it to a disabled one inverts the whole point --
    // measured on a14: with `keyboard.caps` off, three white `Hold` keys
    // read as the most prominent thing in the band.
    //
    // **CORRECTED: the CSS reference does not describe what this now
    // paints.** This used to cite `.wtog.dis`'s `#f7f7f7` on a `#f3f3f3`
    // window as the model -- a face four hex steps LIGHTER than its own
    // ground, "sinking back" only barely. Task 8 moved the ground these
    // chips sit on from `bg` to `card`, and `p.bg` is measurably DARKER
    // than `p.card` in both themes (Dark: `#15171C` vs `#1D2027`; Light:
    // `#F2F4F8` vs `#FFFFFF`), not a near-miss lighter shade of it. A
    // disabled chip now visibly drops below the card ground rather than
    // barely blending into it -- the CSS analogy no longer applies, and
    // should not be re-added without re-deriving it against `card`. Only
    // the ink and the face change; the box and its edge stay, so the shape
    // survives.
    //
    // What a disabled chip stops saying is which way it is set. That is a
    // real loss on the three `Hold` chips, which are greyed whenever Caps is
    // off while still describing what Caps would do. No accent-on-grey
    // pairing exists in the palette to settle it; it wants eyes rather than
    // another argument here.
    let (face, text_colour) = match style {
        CapStyle::Chord => (None, theme_col(|p| p.text, COLOR_BTNTEXT)),
        CapStyle::Toggle { disabled: true, .. } => (
            Some(theme_col(|p| p.bg, COLOR_BTNFACE)),
            theme_col(|p| p.text_faint, COLOR_GRAYTEXT),
        ),
        CapStyle::Toggle { armed: true, .. } => (
            Some(armed_face),
            theme_col(|p| p.accent_on, COLOR_HIGHLIGHTTEXT),
        ),
        CapStyle::Toggle { .. } => (
            Some(theme_col(|p| p.keycap, COLOR_WINDOW)),
            theme_col(|p| p.text, COLOR_BTNTEXT),
        ),
    };
    SetTextColor(hdc, text_colour);
    // A cell's text is data and its `&` is a character; a chip's caption
    // carries a mnemonic, and whether the underline SHOWS is the window's UI
    // state to say, not this function's -- see `draw_chip`.
    let text_flags = match style {
        CapStyle::Chord => DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        CapStyle::Toggle { hide_accel, .. } => {
            let base = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
            if hide_accel {
                base | DT_HIDEPREFIX
            } else {
                base
            }
        }
    };

    for (i, c) in caps.iter().enumerate() {
        let w = widths[i];
        // For a chord: the main key is last, and it is the one actually
        // pressed, so it takes the window fill and reads brighter than the
        // modifiers holding it down. Every row in this column shares the same
        // three-modifier prefix, so the key is the only part worth finding at
        // a glance. A chip is one cap and has no such last, which is why the
        // armed fill is decided above and simply wins here.
        let fill = match face {
            Some(f) => f,
            None if i + 1 == caps.len() => theme_col(|p| p.keycap, COLOR_WINDOW),
            None => theme_col(|p| p.bg, COLOR_BTNFACE),
        };
        if hc {
            // Flat, hard and no depth: a high-contrast theme is built on
            // solid fills and hard borders, and a soft edge under one reads
            // as a rendering artefact rather than as a key.
            let brush = CreateSolidBrush(fill);
            let prev_brush = SelectObject(hdc, HGDIOBJ(brush.0));
            let _ = Rectangle(hdc, x, top, x + w, top + cap_h);
            if !prev_brush.is_invalid() {
                SelectObject(hdc, prev_brush);
            }
            let _ = DeleteObject(HGDIOBJ(brush.0));
        } else {
            // **Two rounded rects, not a rect plus a line.** The edge is a
            // 2 px BORDER in CSS, so it follows the corner radius; the old
            // inset hairline sat inside the box and read as an underline
            // rather than as the side of a key. Painting the taller shape in
            // the edge colour first and the face over it, `edge_h` shorter,
            // leaves exactly that border showing along the bottom.
            //
            // A pressed key skips it and drops a pixel: at the bottom of its
            // travel there is no side left to see.
            let r = scale(5, dpi) * 2;
            if press == 0 {
                let eb = CreateSolidBrush(edge_col);
                let pb = SelectObject(hdc, HGDIOBJ(eb.0));
                let _ = RoundRect(hdc, x, top, x + w, top + cap_h, r, r);
                if !pb.is_invalid() {
                    SelectObject(hdc, pb);
                }
                let _ = DeleteObject(HGDIOBJ(eb.0));
            }
            let brush = CreateSolidBrush(fill);
            let prev_brush = SelectObject(hdc, HGDIOBJ(brush.0));
            let _ = RoundRect(hdc, x, top, x + w, top + cap_h - edge_h, r, r);
            if !prev_brush.is_invalid() {
                SelectObject(hdc, prev_brush);
            }
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }

        // Centred in the FACE, not in the whole cap: the bottom edge is the
        // side of the key, and text centred over it sits low.
        let mut tr = RECT {
            left: x,
            top,
            right: x + w,
            bottom: top + cap_h - if hc || press > 0 { 0 } else { edge_h },
        };
        // The RAW caption, `&` intact: `text_flags` decides whether it marks
        // a mnemonic or is drawn. Only the MEASUREMENT above strips it.
        let mut t = wide(c);
        let n = t.len() - 1;
        DrawTextW(hdc, &mut t[..n], &mut tr, text_flags);
        x += w + gap;
    }

    if !prev_pen.is_invalid() {
        SelectObject(hdc, prev_pen);
    }
    let _ = DeleteObject(HGDIOBJ(pen.0));
    if !prev_font.is_invalid() {
        SelectObject(hdc, prev_font);
    }
    true
}

/// The pill colours for a flag: `(fill, ink)`.
///
/// **The one place in this window with no `GetSysColor` fallback that means
/// anything, and the reason is that the system palette has no opinion here.**
/// There is no `COLOR_WARNING`; Windows' own shell draws these states with
/// semantic colours of its own. `col`'s `sys` argument is still supplied,
/// because the signature requires it, but it is dead in practice: the only
/// caller (`draw_flag_pill`) already returns before reaching this function
/// whenever the theme is high contrast, so the branch that would consult it
/// never runs. `COLOR_WINDOW`/`COLOR_WINDOWTEXT` are named there as the
/// least-wrong stand-ins, not as a considered high-contrast answer.
///
/// **`None` means no pill**, and the caller leaves comctl32's own text
/// showing. That is what `FlagTone::Neutral` gets -- `custom`, which is true
/// and not a problem -- and it is also what the caller substitutes in high
/// contrast and on a selected row, where a pale fill would be either a lie
/// about the theme or unreadable on the accent.
fn flag_colours(t: FlagTone) -> Option<(COLORREF, COLORREF)> {
    match t {
        FlagTone::Bad => Some((
            theme_col(|p| p.bad_bg, COLOR_WINDOW),
            theme_col(|p| p.bad, COLOR_WINDOWTEXT),
        )),
        FlagTone::Warn => Some((
            theme_col(|p| p.warn_bg, COLOR_WINDOW),
            theme_col(|p| p.warn, COLOR_WINDOWTEXT),
        )),
        FlagTone::Neutral => None,
    }
}

/// The selected row's own mark (Task 10): a 2 px `accent` bar down the row's
/// left edge. Drawn in the SAME postpaint pass as the flag pill and for the
/// same reason -- an overlay on top of comctl32's own already-finished draw,
/// never a takeover of it, so the check box a few pixels to its right is
/// never at risk. Not a full accent fill on the row: that would fight the
/// keycaps (`accent_soft`, subitem 1) and the status pill (subitem 0's own
/// colours) for the same cell -- the same reasoning `list_custom_draw`
/// already gives for using `accent_soft` rather than `accent_fill` there.
///
/// **`LVM_GETITEMSTATE`, not `nmcd.uItemState`** -- same rule, same reason,
/// as `list_custom_draw`'s own selection read: at the SUBITEM stage
/// comctl32 reports every row as selected regardless of the real state.
unsafe fn draw_selection_bar(cd: &NMLVCUSTOMDRAW) {
    let list = cd.nmcd.hdr.hwndFrom;
    let row = cd.nmcd.dwItemSpec;
    let sel = SendMessageW(
        list,
        LVM_GETITEMSTATE,
        Some(WPARAM(row)),
        Some(LPARAM(LVIS_SELECTED.0 as isize)),
    )
    .0 != 0;
    if !sel {
        return;
    }
    let mut rc = RECT {
        left: LVIR_BOUNDS as i32,
        ..Default::default()
    };
    let ok = SendMessageW(
        list,
        LVM_GETITEMRECT,
        Some(WPARAM(row)),
        Some(LPARAM(&mut rc as *mut RECT as isize)),
    );
    if ok.0 == 0 || rc.right <= rc.left {
        return;
    }
    let hdc = cd.nmcd.hdc;
    let dpi = GetDpiForWindow(list).max(96);
    let w = scale(2, dpi).max(1);
    let bar = RECT {
        right: rc.left + w,
        ..rc
    };
    FillRect(
        hdc,
        &bar,
        theme_brush(theme_col(|p| p.accent, COLOR_HIGHLIGHT)),
    );
}

/// Lay a coloured pill over the flag comctl32 has just drawn in the App cell.
///
/// **Additive, never a takeover**, and that is the whole design. comctl32 has
/// already drawn the check box, the selection, the ellipsis and the whole
/// cell's text by the time this runs; all this does is cover the flag's own
/// characters with an opaque pill and redraw them in its colour. Nothing here
/// can cost a tick, which is the delete path -- see `list_custom_draw` for
/// what taking the cell over actually did on hardware.
///
/// **It runs only when it will change something**: a row with no flag, a
/// selected row, a high-contrast theme and `FlagTone::Neutral` all leave
/// comctl32's text exactly as it is. So the common case -- a healthy row --
/// costs one string compare.
///
/// **Nothing here reads `UI`**, on `list_custom_draw`'s rule: the text comes
/// from the control, the tone from the flag word through
/// `beckon_core::settings::flag_tone`, the Caption font from `CAP_FONT`.
///
/// The flag's x is derived by measuring `name + FLAG_SEP` in the LIST's own
/// font -- the font comctl32 drew it with -- so the pill lands exactly where
/// those characters are rather than where this function would have put them.
unsafe fn draw_flag_pill(hwnd: HWND, cd: &NMLVCUSTOMDRAW) -> isize {
    let list = cd.nmcd.hdr.hwndFrom;
    let row = cd.nmcd.dwItemSpec;
    let cell = subitem_text(list, row, 0);
    if cell.is_empty() {
        return CDRF_DODEFAULT as isize;
    }
    let (name, flag) = beckon_core::settings::split_app_cell(&cell);
    let Some(flag) = flag else {
        return CDRF_DODEFAULT as isize;
    };
    // A selected row is white-on-accent and a high-contrast theme has no
    // pale fills; in both, comctl32's own text is the right answer.
    let sel = SendMessageW(
        list,
        LVM_GETITEMSTATE,
        Some(WPARAM(row)),
        Some(LPARAM(LVIS_SELECTED.0 as isize)),
    )
    .0 != 0;
    if sel || high_contrast() {
        return CDRF_DODEFAULT as isize;
    }
    let Some((fill, ink)) = flag_colours(beckon_core::settings::flag_tone(flag)) else {
        return CDRF_DODEFAULT as isize;
    };
    let Some(cap) = cap_font() else {
        return CDRF_DODEFAULT as isize;
    };

    // `LVIR_LABEL` on the ITEM: in a report view this is column 0's text
    // area, i.e. past the state image. `LVM_GETSUBITEMRECT` is NOT usable
    // here -- with a subitem of 0 it answers for the whole ITEM, every
    // column, which is how an earlier version of this came to erase the
    // Shortcut keycaps of the row it was drawing.
    let mut rc = RECT {
        left: LVIR_LABEL as i32,
        ..Default::default()
    };
    let ok = SendMessageW(
        list,
        LVM_GETITEMRECT,
        Some(WPARAM(row)),
        Some(LPARAM(&mut rc as *mut RECT as isize)),
    );
    if ok.0 == 0 || rc.right <= rc.left {
        return CDRF_DODEFAULT as isize;
    }

    let hdc = cd.nmcd.hdc;
    let dpi = GetDpiForWindow(hwnd).max(96);
    SetBkMode(hdc, TRANSPARENT);

    // Where the flag's characters START, measured in the list's own font --
    // taken from the control rather than from `Fonts`, for the reason the
    // chips do it.
    let body = HFONT(
        SendMessageW(list, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0
            as *mut core::ffi::c_void,
    );
    let prev = if body.is_invalid() {
        HGDIOBJ::default()
    } else {
        SelectObject(hdc, HGDIOBJ(body.0))
    };
    let lead = format!("{name}{}", beckon_core::settings::FLAG_SEP);
    let lt = wide(&lead);
    let mut nw = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &lt[..lt.len() - 1], &mut nw);
    // What comctl32 drew the flag in, so the pill is guaranteed to cover it
    // even though the pill's own text is Caption and narrower.
    let ft_body = wide(flag);
    let mut body_fw = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &ft_body[..ft_body.len() - 1], &mut body_fw);
    if !prev.is_invalid() {
        SelectObject(hdc, prev);
    }

    let prev_cap = SelectObject(hdc, HGDIOBJ(cap.0));
    let mut fw = SIZE::default();
    let ft = wide(flag);
    let _ = GetTextExtentPoint32W(hdc, &ft[..ft.len() - 1], &mut fw);
    // The pill is padded from the CAPTION width but must never be narrower
    // than the Body text underneath it, or the old characters peek out of
    // both ends.
    let padx = scale(7, dpi);
    let pill_w = (fw.cx + padx * 2).max(body_fw.cx + padx);
    let px = rc.left + nw.cx - padx / 2;
    let pill_h = (fw.cy + scale(4, dpi)).min(rc.bottom - rc.top);
    let py = rc.top + (rc.bottom - rc.top - pill_h) / 2;
    // Nothing is drawn at all if the pill would not fit the column: half a
    // pill over half a word is worse than the plain text comctl32 drew.
    if px + pill_w <= rc.right {
        let brush = CreateSolidBrush(fill);
        let pb = SelectObject(hdc, HGDIOBJ(brush.0));
        let pen = CreatePen(PS_SOLID, 1, fill);
        let pp = SelectObject(hdc, HGDIOBJ(pen.0));
        // A radius of the pill's own height is what makes the ends round
        // rather than merely soft -- `.chip { border-radius:10px }` on a
        // 10 px-tall pill.
        let r = pill_h;
        let _ = RoundRect(hdc, px, py, px + pill_w, py + pill_h, r, r);
        if !pp.is_invalid() {
            SelectObject(hdc, pp);
        }
        let _ = DeleteObject(HGDIOBJ(pen.0));
        if !pb.is_invalid() {
            SelectObject(hdc, pb);
        }
        let _ = DeleteObject(HGDIOBJ(brush.0));

        SetTextColor(hdc, ink);
        let mut ftr = RECT {
            left: px,
            top: py,
            right: px + pill_w,
            bottom: py + pill_h,
        };
        let mut fbuf = wide(flag);
        let f = fbuf.len() - 1;
        DrawTextW(
            hdc,
            &mut fbuf[..f],
            &mut ftr,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
    if !prev_cap.is_invalid() {
        SelectObject(hdc, prev_cap);
    }
    CDRF_DODEFAULT as isize
}

/// Paint the Shortcut column as keycaps. Subitem 1, and only subitem 1.
///
/// **Both subitems are drawn now.** Subitem 0 carries `LVS_EX_CHECKBOXES`'
/// state image -- the tick that makes `Remove` a multi-delete -- and whether
/// `CDRF_SKIPDEFAULT` there takes the tick with it was an open hardware
/// question until `examples/customdraw_probe.rs` was finally run on a14
/// (2026-08-13, `VERDICT=TICK_SURVIVES`). See `draw_app_cell`.
///
/// **Nothing here reads `UI`.** Everything it needs comes from the
/// notification itself or from a `Cell`: the list handle from `hdr.hwndFrom`,
/// the chord from the cell's own text, the font from `CAP_FONT`. A paint can
/// arrive while `UI` is borrowed, and it does.
pub(super) unsafe fn list_custom_draw(hwnd: HWND, p: *const NMLVCUSTOMDRAW) -> isize {
    let cd = &*p;
    let stage = cd.nmcd.dwDrawStage;
    if stage == CDDS_PREPAINT {
        return CDRF_NOTIFYITEMDRAW as isize;
    }
    if stage == CDDS_ITEMPREPAINT {
        return CDRF_NOTIFYSUBITEMDRAW as isize;
    }
    // `NMCUSTOMDRAW_DRAW_STAGE` has no `BitOr` in `windows` 0.61 -- unlike the
    // flag types it is a bare newtype, not a generated bitmask type. Compare
    // the raw u32s; `examples/customdraw_probe.rs` found this the hard way.
    if stage.0 == CDDS_ITEMPOSTPAINT.0 | CDDS_SUBITEM.0 {
        return if cd.iSubItem == 0 {
            draw_selection_bar(cd);
            draw_flag_pill(hwnd, cd)
        } else {
            CDRF_DODEFAULT as isize
        };
    }
    if stage.0 != CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0 {
        return CDRF_DODEFAULT as isize;
    }
    // **Subitem 0 asks to be called back AFTER comctl32 has drawn it**, and
    // never takes it over.
    //
    // `customdraw_probe` answered `TICK_SURVIVES` for `CDRF_SKIPDEFAULT` on
    // subitem 0, and **taking that as permission to own the cell was wrong**
    // -- measured on a14 2026-08-13, in this window rather than in the
    // probe's own: every row that returned `SKIPDEFAULT` lost its check box,
    // and the selected row lost its keycaps as well. The probe builds a
    // ListView of its own with no owner-drawn neighbours, so what it proved
    // is narrower than what it was read to prove. It is a measurement of the
    // probe's window, not a licence for this one.
    //
    // `CDRF_NOTIFYPOSTPAINT` sidesteps the whole question: comctl32 draws the
    // tick, the selection, the ellipsis and the text exactly as it always
    // has, and `draw_flag_pill` only lays a pill over the flag afterwards.
    // Nothing this window draws can cost a tick, which is the delete path.
    if cd.iSubItem == 0 {
        return CDRF_NOTIFYPOSTPAINT as isize;
    }
    if cd.iSubItem != 1 {
        return CDRF_DODEFAULT as isize;
    }
    let Some(font) = cap_font() else {
        return CDRF_DODEFAULT as isize;
    };

    let list = cd.nmcd.hdr.hwndFrom;
    let row = cd.nmcd.dwItemSpec;
    let shown = subitem_text(list, row, 1);
    if shown.is_empty() {
        return CDRF_DODEFAULT as isize;
    }

    // `LVM_GETSUBITEMRECT` rather than `nmcd.rc`: the message is unambiguous
    // about which rect it returns, and it takes the subitem in `rc.top` and
    // the part in `rc.left`, which is the documented calling convention rather
    // than a quirk.
    let mut rc = RECT {
        left: LVIR_BOUNDS as i32,
        top: 1,
        right: 0,
        bottom: 0,
    };
    let ok = SendMessageW(
        list,
        LVM_GETSUBITEMRECT,
        Some(WPARAM(row)),
        Some(LPARAM(&mut rc as *mut RECT as isize)),
    );
    if ok.0 == 0 || rc.right <= rc.left {
        return CDRF_DODEFAULT as isize;
    }

    let hdc = cd.nmcd.hdc;
    // **`LVM_GETITEMSTATE`, not `nmcd.uItemState`.** At the SUBITEM stage
    // comctl32 reports `CDIS_SELECTED` for every row regardless of the real
    // selection -- measured on a14: with nothing selected, the whole Shortcut
    // column painted `COLOR_HIGHLIGHT`. The control's own answer is the only
    // one worth asking at this stage.
    let sel = SendMessageW(
        list,
        LVM_GETITEMSTATE,
        Some(WPARAM(row)),
        Some(LPARAM(LVIS_SELECTED.0 as isize)),
    )
    .0 != 0;
    // The row under the mouse (Task 10) -- `LVM_GETHOTITEM`, not
    // `nmcd.uItemState`'s `CDIS_HOT` bit. Not independently measured, but
    // read the same skeptical way the comment above already reads
    // selection: this stage's `uItemState` is proven wrong for SELECTED,
    // and there is no reason to trust it more for HOT. Comparing this row's
    // own index against the control's own idea of which one is hot sidesteps
    // the question entirely, the way `LVM_GETITEMSTATE` does for `sel`.
    let hot_row = SendMessageW(list, LVM_GETHOTITEM, Some(WPARAM(0)), Some(LPARAM(0))).0;
    let hot = !sel && hot_row >= 0 && hot_row as usize == row;
    // `CDRF_SKIPDEFAULT` means we own the background too, not only the text.
    // Getting this wrong shows up as a selected row with one un-highlighted
    // cell, which is worse than no keycaps at all. `theme_brush` returns a
    // brush this window owns, never a system one -- see the ban at the top
    // of this file. A selected row takes `accent_soft`, not the stronger
    // `accent_fill` an armed chip or `Save` gets: a full-strength fill this
    // close to a column of keycaps would fight them for attention.
    let bg = if sel {
        theme_col(|p| p.accent_soft, COLOR_HIGHLIGHT)
    } else if hot && !high_contrast() {
        // "Reduced weight": `accent_soft` blended halfway toward the resting
        // `card` ground, so a hovered row reads as a hint rather than the
        // stronger, unblended fill a selected row gets. Skipped outright
        // under high contrast -- `blend`'s own doc explains why a blend of
        // two `GetSysColor` answers has no guaranteed relationship to
        // anything; falling through to the same `card`/`COLOR_WINDOW` pair
        // every resting row already uses is the safe answer.
        blend(
            theme_col(|p| p.accent_soft, COLOR_HIGHLIGHT),
            theme_col(|p| p.card, COLOR_WINDOW),
            1,
            2,
        )
    } else {
        theme_col(|p| p.card, COLOR_WINDOW)
    };
    FillRect(hdc, &rc, theme_brush(bg));

    // The cell holds `combo_display`'s output, so splitting on its separator
    // recovers exactly the caps `combo_caps` would have produced -- without a
    // second source of truth to keep in step.
    let caps: Vec<String> = shown.split(" + ").map(|s| s.to_string()).collect();
    let dpi = GetDpiForWindow(hwnd).max(96);
    if !draw_keycaps(hdc, rc, &caps, font, dpi, high_contrast(), CapStyle::Chord) {
        let mut tr = RECT {
            left: rc.left + scale(6, dpi),
            ..rc
        };
        let mut t = wide(&shown);
        let n = t.len() - 1;
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(
            hdc,
            if sel {
                theme_col(|p| p.text, COLOR_HIGHLIGHTTEXT)
            } else {
                theme_col(|p| p.text, COLOR_WINDOWTEXT)
            },
        );
        DrawTextW(
            hdc,
            &mut t[..n],
            &mut tr,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
    CDRF_SKIPDEFAULT as isize
}

/// Paint one Header item -- `IDC_LIST`'s own column-header row (Task 10): a
/// `card` ground, its caption in `text_muted` at `Role::BodyStrong`, and a
/// 1 px `divider` along the bottom, so the header reads as part of the card
/// rather than as the system's own grey button strip. No sort glyphs:
/// `IDC_LIST` already carries `LVS_NOSORTHEADER`, and `CDRF_SKIPDEFAULT`
/// below means comctl32 never gets to draw one regardless.
///
/// **Reached through message reflection, not a dedicated control id.** The
/// Header is a child of `IDC_LIST`, never of `hwnd` -- `set_header_font`'s
/// own reason -- so its `WM_NOTIFY`s carry `hwndFrom` equal to the HEADER's
/// own HWND rather than `IDC_LIST`'s; `mod.rs`'s dispatcher (`header_of`)
/// tells the two custom-draw sources apart by handle, not by `idFrom`.
///
/// **The caption comes from `LIST_COLUMNS`, not a live read of the
/// control.** The same reason `draw_combo_item` reads `key_table()` /
/// `cap::TAP_ITEMS` rather than the control they populated: this text
/// cannot go stale because nothing ever rewrites it after `build_children`
/// inserts the two columns from this exact array. The alignment
/// (`LVCFMT_RIGHT` for Shortcut) comes from the same array, so the header
/// caption and the column's own cells can never disagree about which way
/// they lean.
pub(super) unsafe fn header_custom_draw(hwnd: HWND, p: *const NMCUSTOMDRAW) -> isize {
    let cd = &*p;
    if cd.dwDrawStage == CDDS_PREPAINT {
        return CDRF_NOTIFYITEMDRAW as isize;
    }
    if cd.dwDrawStage != CDDS_ITEMPREPAINT {
        return CDRF_DODEFAULT as isize;
    }
    let Some((title, fmt)) = LIST_COLUMNS.get(cd.dwItemSpec).copied() else {
        return CDRF_DODEFAULT as isize;
    };

    let hdc = cd.hdc;
    let rc = cd.rc;
    FillRect(hdc, &rc, theme_brush(theme_col(|p| p.card, COLOR_WINDOW)));

    // The bottom pixel row, in the divider colour: one line per column, and
    // every column shares the same top/bottom, so the sum reads as one rule
    // the width of the header rather than a dashed one.
    let div = RECT {
        top: rc.bottom - 1,
        ..rc
    };
    FillRect(
        hdc,
        &div,
        theme_brush(theme_col(|p| p.divider, COLOR_BTNSHADOW)),
    );

    // The control's own font (`set_header_font` puts `Role::BodyStrong` on
    // it, at creation and on every `WM_DPICHANGED`), read back rather than
    // asked of `Fonts` -- `draw_combo_item`'s own reason: a paint can arrive
    // while `UI` is borrowed, and it does.
    let font = HFONT(
        SendMessageW(
            cd.hdr.hwndFrom,
            WM_GETFONT,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        )
        .0 as *mut core::ffi::c_void,
    );
    let prev = if font.is_invalid() {
        HGDIOBJ::default()
    } else {
        SelectObject(hdc, HGDIOBJ(font.0))
    };
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, theme_col(|p| p.text_muted, COLOR_WINDOWTEXT));
    let dpi = GetDpiForWindow(hwnd).max(96);
    let pad = scale(6, dpi);
    let right = fmt == LVCFMT_RIGHT;
    let mut tr = RECT {
        left: rc.left + if right { 0 } else { pad },
        right: rc.right - if right { pad } else { 0 },
        ..rc
    };
    let mut t = wide(title);
    let n = t.len() - 1;
    DrawTextW(
        hdc,
        &mut t[..n],
        &mut tr,
        (if right { DT_RIGHT } else { DT_LEFT }) | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    if !prev.is_invalid() {
        SelectObject(hdc, prev);
    }
    CDRF_SKIPDEFAULT as isize
}

/// Paint one toggle chip as a keycap. The four modifier chips and the three
/// `Hold` chips, and nothing else in this window is owner-draw.
///
/// **Nothing here reads `UI`**, for `list_custom_draw`'s reason: a paint can
/// arrive while `UI` is borrowed, and it does. Everything comes out of the
/// `DRAWITEMSTRUCT` or out of the control itself -- the caption through
/// `text_of`, the font through `WM_GETFONT`, the armed bit from `CHIPS`,
/// which is a `Cell` precisely so this path cannot be contended.
///
/// **The font is asked of the control, not of `Fonts`.** `child` put the
/// role's font on it at creation and `WM_DPICHANGED` rebroadcasts a new one,
/// so `WM_GETFONT` is the live answer and there is no third copy of the
/// mapping to keep in step. `layout` measures these captions in Body through
/// `tw`, which is the same font, which is what makes the fit check below a
/// real one.
///
/// **Whether the mnemonic underline shows is the WINDOW's UI state**, read
/// with `WM_QUERYUISTATE` rather than `SPI_GETKEYBOARDCUES`. The SPI is the
/// global default; the per-window flags are the live answer, and they are
/// what Windows itself moves -- through `WM_UPDATEUISTATE` -- the moment the
/// user presses Alt or navigates by keyboard. Reading the SPI would leave
/// these three chips underlined while every real control beside them was
/// not. The same read answers the focus rect, which owner-draw also has to
/// draw for itself or the keyboard route is silently lost.
pub(super) unsafe fn draw_chip(hwnd: HWND, di: &DRAWITEMSTRUCT) -> bool {
    if di.CtlType != ODT_BUTTON {
        return false;
    }
    let Some(bit) = chip_bit(di.CtlID as i32) else {
        return false;
    };
    let hdc = di.hDC;
    let rc = di.rcItem;
    // The parent's background, first and over the WHOLE rect. An owner-draw
    // button draws nothing of itself, its background included, so any pixel
    // this function leaves alone keeps whatever the last frame put there --
    // and the cap is deliberately narrower than its control, so there are
    // plenty of them. `bg` is what the window class registers; `theme_brush`
    // returns a brush this window owns, never a system one -- see the ban at
    // the top of this file.
    FillRect(hdc, &rc, theme_brush(theme_col(|p| p.bg, COLOR_BTNFACE)));

    // Never zero in practice -- `child` sets a font on every control it
    // creates -- but a null `HFONT` would make `SelectObject` fail and leave
    // the cap in the DC's own stock font, which at this size is unreadable
    // rather than merely wrong.
    let font = HFONT(
        SendMessageW(di.hwndItem, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0
            as *mut core::ffi::c_void,
    );
    let font = if font.is_invalid() {
        HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
    } else {
        font
    };
    let ui_state = SendMessageW(
        di.hwndItem,
        WM_QUERYUISTATE,
        Some(WPARAM(0)),
        Some(LPARAM(0)),
    )
    .0 as u32;
    let style = CapStyle::Toggle {
        armed: chip_armed(bit),
        pressed: di.itemState.0 & ODS_SELECTED.0 != 0,
        disabled: di.itemState.0 & ODS_DISABLED.0 != 0,
        hide_accel: ui_state & UISF_HIDEACCEL != 0,
    };
    let dpi = GetDpiForWindow(hwnd).max(96);
    // Read back from the CONTROL, not from `mod cap`, for `subitem_text`'s
    // reason: what is drawn and what an accessibility client reads out are
    // then the same string by construction, rather than by two code paths
    // agreeing.
    let caps = [text_of(di.hwndItem)];
    if !draw_keycaps(hdc, rc, &caps, font, dpi, high_contrast(), style) {
        // The same fallback the Shortcut column takes, and for the same
        // reason: a clipped keycap reads as a rendering fault, plain text
        // reads as a narrow control. `layout` sizes each chip from its own
        // caption so this should be unreachable -- which is exactly why it
        // must not be an empty control if it ever is.
        let prev = SelectObject(hdc, HGDIOBJ(font.0));
        let mut tr = rc;
        let mut t = wide(&caps[0]);
        let n = t.len() - 1;
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(
            hdc,
            if di.itemState.0 & ODS_DISABLED.0 != 0 {
                theme_col(|p| p.text_faint, COLOR_GRAYTEXT)
            } else {
                theme_col(|p| p.text, COLOR_BTNTEXT)
            },
        );
        let mut flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
        if ui_state & UISF_HIDEACCEL != 0 {
            flags |= DT_HIDEPREFIX;
        }
        DrawTextW(hdc, &mut t[..n], &mut tr, flags);
        if !prev.is_invalid() {
            SelectObject(hdc, prev);
        }
    }
    // XOR-drawn, so it goes on LAST or the fill eats it. Suppressed while the
    // window says cues are hidden, which is the state a mouse-driven session
    // stays in -- the same flag word that decides the underline.
    if di.itemState.0 & ODS_FOCUS.0 != 0 && ui_state & UISF_HIDEFOCUS == 0 {
        let d = scale(1, dpi);
        let f = RECT {
            left: rc.left + d,
            top: rc.top + d,
            right: rc.right - d,
            bottom: rc.bottom - d,
        };
        let _ = DrawFocusRect(hdc, &f);
    }
    true
}

/// The corner radius every push button and every field decoration shares,
/// at 96 DPI. One constant rather than a literal `6` at each call site --
/// see `scale`'s own rule.
const BTN_RADIUS: i32 = 6;

/// Which visual family a push button paints as.
///
/// `Accent` is `Save` alone. `Secondary` is every plain command (`Add`,
/// `Remove`, `Reload`, `Open config file`, `Close`, `Keep mine`). `Outline`
/// and `Danger` are the capture strip's two commands -- `Reset` is always
/// `Outline`; `Record` is `Outline` while idle and `Danger` once armed,
/// wearing `Stop`. Neither carries a fill, which is what keeps the strip
/// reading lighter than the command bar around it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BtnTier {
    Accent,
    Secondary,
    Outline,
    Danger,
}

/// `(fill, border, ink)` for one tier in its current state.
///
/// **CORRECTED: `fill` is `None` for a resting `Outline`/`Danger` button,
/// and that does NOT mean the card underneath shows through.** This
/// comment used to claim that; it is false, and `button` (the sole caller,
/// below) disproves it in its own first line: it fills the WHOLE control
/// rect with `p.bg` before this function is even called, unconditionally,
/// every tier, every state. So a resting `Record`/`Reset` is a solid `bg`
/// box on the `card` ground six of the nine push buttons (and all seven
/// chips) sit on -- dark `#15171C` on `#1D2027` -- and every OTHER tier
/// leaks that same `bg` at its four rounded corners, since `RoundRect`'s
/// fill only covers the rounded interior of a rect `FillRect` already
/// painted square a moment before. Only the command bar's three buttons
/// sit on `bg` to begin with, so there alone the leak is invisible, by
/// coincidence rather than by design. Fixing the code (passing the real
/// surrounding surface in) is a change to `button`'s own painting, not
/// tracked here; this fixes the comment to stop asserting the wrong thing.
///
/// **Disabled overrides every tier the same way**: a neutral `field`
/// surface, its own `field_border` edge, and `text_faint` ink -- the row
/// this task's brief gives every tier. For `Secondary` that is coincidence
/// rather than a special case: its own resting fill and border ALREADY are
/// `field`/`field_border`, so disabled only changes the ink there. That is
/// not a gap; `Secondary` never claimed a stronger surface than a field to
/// begin with.
///
/// **`shade`, not `accent_hover`, for `Accent`'s hot/pressed fill.**
/// Carried into this task from Task 5's review: `Save`'s hot state used to
/// darken with `shade(accent, 9, 10)` while `DARK.accent_hover` is
/// deliberately LIGHTER than `DARK.accent_fill`, which read as the wrong
/// token for the job. Measured before changing it: `accent_hover` is
/// calibrated as an INK, not as a FILL under a fixed white ink -- on a card
/// it clears 6.66:1 (Light) / 6.77:1 (Dark), stronger than `accent` itself,
/// but used as `Save`'s own fill with `accent_on` on top it is 6.66:1 Light
/// (fine) and **2.41:1 Dark** -- a real WCAG failure, because
/// `DARK.accent_hover` (`#7AA7F9`) sits close enough to white that white
/// text on it nearly disappears. `shade` always darkens, which is the safe
/// direction against a FIXED white ink regardless of theme: hot
/// `shade(_, 9, 10)` measures 6.11:1 Light / 5.43:1 Dark, pressed
/// `shade(_, 4, 5)` measures 7.25:1 Light / 6.50:1 Dark -- both tiers, both
/// themes, comfortably clear. `Secondary`'s hot/pressed fill reuses the same
/// two ratios on `field` for the same reason: its ink (`text`) already
/// flips dark-on-light/light-on-dark with the theme, so darkening the fill
/// in EITHER theme only widens the gap (Light 14.11:1/11.07:1, Dark
/// 13.11:1/13.71:1, hot/pressed) -- unlike `Accent`, whose ink is a FIXED
/// white in both themes.
///
/// **`accent_hover` earns its keep on `Outline` instead.** Its own ink, on
/// its own resting fill-less surface (the card), is exactly the pairing it
/// is calibrated for: hovering/pressing brightens `Record`/`Reset`'s
/// border+text from `accent` (5.17:1 Light / 5.36:1 Dark on card) to
/// `accent_hover` (6.66:1 / 6.77:1) -- an improvement, not a risk, in both
/// themes. Pressed additionally washes the fill to `accent_soft`, an
/// already CI-enforced pairing (`beckon_core::theme`'s own
/// `"accent text on soft fill"` test); `accent_hover` on `accent_soft`
/// measures 5.82:1 Light / 5.94:1 Dark, still comfortably clear.
///
/// **`Danger`'s pressed wash reuses `bad_bg`**, the SAME token
/// `draw_flag_pill`'s `flag_colours` already pairs with `bad` for the "bad"
/// flag pill (`beckon_core::theme`'s `"bad pill"` test: `p.bad` on
/// `p.bad_bg` >= 4.5) -- 5.56:1 Light / 7.55:1 Dark measured here again on
/// the button-sized area. No new token, no new pairing.
///
/// Every fallback below is a matched SYSTEM pair under high contrast, never
/// a mix: `COLOR_HIGHLIGHT`+`COLOR_HIGHLIGHTTEXT` for `Accent` (already
/// shipped), `COLOR_BTNFACE`+`COLOR_BTNTEXT` for `Secondary` and for the
/// disabled row, `COLOR_WINDOW`+`COLOR_WINDOWTEXT` for `Outline` (the same
/// pairing `chrome::paint` already uses for `accent`-coloured text directly
/// on a plain background) and for `Danger`'s resting/pressed ink -- there is
/// no system "danger" colour, so `Danger` reads as plain, matched text under
/// high contrast rather than inventing an unmatched red.
fn colours(
    tier: BtnTier,
    disabled: bool,
    pressed: bool,
    hot: bool,
    cache: &ThemeCache,
) -> (Option<COLORREF>, COLORREF, COLORREF) {
    if disabled {
        let fill = cache.col(|p| p.field, COLOR_BTNFACE);
        let border = cache.col(|p| p.field_border, COLOR_BTNSHADOW);
        let ink = cache.col(|p| p.text_faint, COLOR_GRAYTEXT);
        return (Some(fill), border, ink);
    }
    match tier {
        BtnTier::Accent => {
            let accent = cache.col(|p| p.accent_fill, COLOR_HIGHLIGHT);
            let on = cache.col(|p| p.accent_on, COLOR_HIGHLIGHTTEXT);
            let fill = if pressed {
                shade(accent, 4, 5)
            } else if hot {
                shade(accent, 9, 10)
            } else {
                accent
            };
            (Some(fill), fill, on)
        }
        BtnTier::Secondary => {
            let field = cache.col(|p| p.field, COLOR_BTNFACE);
            let border = cache.col(|p| p.field_border, COLOR_BTNSHADOW);
            let text = cache.col(|p| p.text, COLOR_BTNTEXT);
            let fill = if pressed {
                shade(field, 4, 5)
            } else if hot {
                shade(field, 9, 10)
            } else {
                field
            };
            (Some(fill), border, text)
        }
        BtnTier::Outline => {
            let ink = if hot || pressed {
                cache.col(|p| p.accent_hover, COLOR_HIGHLIGHT)
            } else {
                cache.col(|p| p.accent, COLOR_HIGHLIGHT)
            };
            // Under high contrast, `col` ignores the palette closure and returns
            // GetSysColor(sys). The fill and ink MUST use different sys indices to
            // avoid a 1:1 contrast collision where caption text vanishes into its own
            // background. `COLOR_WINDOW` for fill and `COLOR_HIGHLIGHT` for ink form a
            // valid matched pair — the same pairing this window already uses for
            // accent-coloured text directly on a plain background elsewhere.
            let fill = pressed.then(|| cache.col(|p| p.accent_soft, COLOR_WINDOW));
            (fill, ink, ink)
        }
        BtnTier::Danger => {
            let ink = cache.col(|p| p.bad, COLOR_WINDOWTEXT);
            let fill = pressed.then(|| cache.col(|p| p.bad_bg, COLOR_WINDOW));
            (fill, ink, ink)
        }
    }
}

/// Paint one push button: fill (if any), border, caption, then -- LAST --
/// the focus ring.
///
/// **Every push button in this window paints through here, `Save`
/// included, but `Save` still ARRIVES through `NM_CUSTOMDRAW`, not
/// `WM_DRAWITEM`.** `BS_OWNERDRAW` REPLACES a button's TYPE -- it is a
/// different value of the same 4-bit field `BS_DEFPUSHBUTTON` occupies, not
/// a flag beside it -- and `Save` (like every one of the nine
/// `PUSH_BUTTONS`) can carry the default ring, which `set_default_id` moves
/// with a `BM_SETSTYLE` read-modify-write through that same field. Making
/// any of them owner-draw would take that machinery with it: the exact
/// "Enter on Reload saves" defect this window already shipped once. So
/// `mod.rs`'s `push_button_custom_draw` builds a synthetic `DRAWITEMSTRUCT`
/// out of the `NMCUSTOMDRAW` comctl32 hands it and calls this function --
/// one painter, reached by two honest callers, rather than a painter that
/// can only be reached by the controls it was safe to convert.
///
/// `cache` is the caller's own borrow of `PAINT_THEME`, taken once and
/// passed down -- `chrome::paint`'s own rule, and for the same reason:
/// re-reading it through `theme_col`/`theme_brush` here would try to borrow
/// the same `RefCell` a second time and panic. `cache.theme()` is read
/// directly for the same reason `chrome::paint` reads it rather than the
/// separate `HIGH_CONTRAST` `Cell` `draw_keycaps` and its callers use: that
/// `Cell` only refreshes on `WM_SETTINGCHANGE(SPI_SETHIGHCONTRAST)`, while
/// `WM_THEMECHANGED` alone already rebuilds `ThemeCache` and invalidates the
/// window, so a paint that races the two message could see a `Cell` not yet
/// caught up. Gating on `cache.theme()` removes that race instead of
/// narrowing it.
pub(super) unsafe fn button(di: &DRAWITEMSTRUCT, tier: BtnTier, cache: &mut ThemeCache, dpi: u32) {
    let hdc = di.hDC;
    let rc = di.rcItem;
    let hc = cache.theme() == beckon_core::theme::Theme::HighContrast;
    let disabled = di.itemState.0 & ODS_DISABLED.0 != 0;
    let pressed = di.itemState.0 & ODS_SELECTED.0 != 0;
    let hot = di.itemState.0 & ODS_HOTLIGHT.0 != 0;

    // The parent's surface first: a button with no fill (resting
    // `Outline`/`Danger`) or rounded corners (every tier) leaves pixels
    // showing that the tier itself does not paint, and whatever was there
    // last frame would stay there otherwise.
    let bg = cache.col(|p| p.bg, COLOR_BTNFACE);
    let bg_brush = cache.brush(bg);
    FillRect(hdc, &rc, bg_brush);

    let (fill, border, ink) = colours(tier, disabled, pressed, hot, cache);

    let brush = match fill {
        Some(f) => HGDIOBJ(CreateSolidBrush(f).0),
        None => GetStockObject(NULL_BRUSH),
    };
    let pen = CreatePen(PS_SOLID, 1, border);
    let prev_brush = SelectObject(hdc, brush);
    let prev_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    if hc {
        let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
    } else {
        let r = scale(BTN_RADIUS, dpi) * 2;
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);
    }
    if !prev_pen.is_invalid() {
        SelectObject(hdc, prev_pen);
    }
    if !prev_brush.is_invalid() {
        SelectObject(hdc, prev_brush);
    }
    let _ = DeleteObject(HGDIOBJ(pen.0));
    if fill.is_some() {
        let _ = DeleteObject(brush);
    }

    let font = HFONT(
        SendMessageW(di.hwndItem, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0
            as *mut core::ffi::c_void,
    );
    let font = if font.is_invalid() {
        HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
    } else {
        font
    };
    let prev_font = SelectObject(hdc, HGDIOBJ(font.0));
    let ui_state = SendMessageW(
        di.hwndItem,
        WM_QUERYUISTATE,
        Some(WPARAM(0)),
        Some(LPARAM(0)),
    )
    .0 as u32;
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, ink);
    let mut flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
    if ui_state & UISF_HIDEACCEL != 0 {
        flags |= DT_HIDEPREFIX;
    }
    let caption = text_of(di.hwndItem);
    let mut t = wide(&caption);
    let n = t.len() - 1;
    let mut tr = rc;
    DrawTextW(hdc, &mut t[..n], &mut tr, flags);
    if !prev_font.is_invalid() {
        SelectObject(hdc, prev_font);
    }

    // Last, per the brief: a 2 px accent ring, inset 2 px, so it never
    // fights the caption for the same pixels.
    if !disabled && di.itemState.0 & ODS_FOCUS.0 != 0 && ui_state & UISF_HIDEFOCUS == 0 {
        // `Accent` alone substitutes `accent_on` for the ring. `accent` on
        // `Accent`'s own `accent_fill` is 1.00:1 in Light -- IDENTICAL hex,
        // `#2563EB` on `#2563EB` -- and 1.49:1 in Dark, both well under even
        // the 3:1 non-text floor: a ring drawn in the tier's own colour is
        // invisible on the one tier whose fill IS that colour. `accent_on`
        // is `Save`'s own ink and already guaranteed >= 4.5 against
        // `accent_fill` by `beckon_core::theme`'s `"white on accent fill"`
        // test, so it is guaranteed visible here too, in both themes.
        let ring = if tier == BtnTier::Accent {
            cache.col(|p| p.accent_on, COLOR_HIGHLIGHTTEXT)
        } else {
            cache.col(|p| p.accent, COLOR_HIGHLIGHT)
        };
        let inset = scale(2, dpi);
        let ring_rc = RECT {
            left: rc.left + inset,
            top: rc.top + inset,
            right: rc.right - inset,
            bottom: rc.bottom - inset,
        };
        let ring_pen = CreatePen(PS_SOLID, scale(2, dpi), ring);
        let null_brush = GetStockObject(NULL_BRUSH);
        let prev_pen = SelectObject(hdc, HGDIOBJ(ring_pen.0));
        let prev_brush = SelectObject(hdc, null_brush);
        if hc {
            let _ = Rectangle(
                hdc,
                ring_rc.left,
                ring_rc.top,
                ring_rc.right,
                ring_rc.bottom,
            );
        } else {
            let r = scale(BTN_RADIUS - 2, dpi).max(0) * 2;
            let _ = RoundRect(
                hdc,
                ring_rc.left,
                ring_rc.top,
                ring_rc.right,
                ring_rc.bottom,
                r,
                r,
            );
        }
        if !prev_pen.is_invalid() {
            SelectObject(hdc, prev_pen);
        }
        if !prev_brush.is_invalid() {
            SelectObject(hdc, prev_brush);
        }
        let _ = DeleteObject(HGDIOBJ(ring_pen.0));
    }
}

/// Paint `IDC_CAPS` -- the one toggle switch in this window -- as a 40x20
/// track with a sliding knob, in place of the native check box glyph.
///
/// **`IDC_CAPS` stays `BS_AUTOCHECKBOX`, reached through `NM_CUSTOMDRAW`,
/// exactly `button`'s own pattern one function up.** `BS_OWNERDRAW` is a
/// different VALUE of the same 4-bit type field `BS_DEFPUSHBUTTON` and
/// `BS_AUTOCHECKBOX` occupy (`BS_TYPEMASK_BITS`), not a flag beside it, so
/// adopting it would throw away the check box state machine `handle_command`'s
/// `(IDC_CAPS, _)` arm and `apply_state`'s own `check(hwnd, IDC_CAPS, ...)`
/// both depend on, and the UIA role a screen reader announces for the
/// control. Custom draw leaves `BM_GETCHECK`/`BM_SETCHECK`, `Space` and the
/// accessible role exactly as they are and only replaces the pixels --
/// `caps_custom_draw` (`mod.rs`) is the translation, one `NMCUSTOMDRAW` in,
/// one full repaint out, no `DRAWITEMSTRUCT` needed because nothing here
/// reads one.
///
/// **High contrast: knob and track never share a `sys` index.** `col`
/// ignores the palette closure under `Theme::HighContrast` and answers
/// `GetSysColor(sys)` regardless of state, so the FALLBACK argument is what
/// a high-contrast user actually sees, and the knob is drawn ON the track --
/// exactly the shape three earlier defects on this branch already got wrong.
/// Every state below pairs two DIFFERENT indices: `COLOR_BTNFACE` (track) /
/// `COLOR_BTNTEXT` (knob) off, `COLOR_HIGHLIGHT` (track) /
/// `COLOR_HIGHLIGHTTEXT` (knob) on, `COLOR_BTNFACE` (track) /
/// `COLOR_GRAYTEXT` (knob) disabled -- the last of those is the same pairing
/// `button`'s own disabled row already ships for `field`/`text_faint`, not a
/// new one invented here. The background wash behind the caption is
/// `COLOR_WINDOW`, paired with the caption's own `COLOR_WINDOWTEXT` --
/// `card`'s usual fallback everywhere else in this file (`card`, the
/// resting list row, the flag pill's least-wrong stand-in), not the
/// `COLOR_BTNFACE` the pre-Task-11 `WM_CTLCOLORSTATIC` arm used for the same
/// token: that arm never reaches this control any more once `CDRF_SKIPDEFAULT`
/// takes its whole paint over here, the same way it already stopped reaching
/// any of the nine `PUSH_BUTTONS`.
///
/// **No `0`/`1` in the knob.** VKey draws one; the knob's own position --
/// left when off, right when on -- already says everything a digit would.
///
/// **Geometry: `layout` gives `IDC_CAPS` its own budget, not `glyph`.**
/// `w_caps = tw(cap::CAPS) + toggle_glyph`, where `toggle_glyph` (`layout.rs`,
/// `s(50)`) covers everything this function draws before the caption: the
/// track's own left inset (`off`, 2 px -- see the track-rect comment above),
/// the 40 px track itself, and `tok::GAP` (6 px) before the text. Those sum
/// to 48, so the budget covers them with 2 logical px left over -- 2 to 6
/// physical across the standard scale steps, derived step by step in the
/// comment beside `toggle_glyph`. The caption's `DrawTextW` box is therefore
/// never narrower than its own measured width, and is in fact that much
/// wider, so `DT_END_ELLIPSIS` below is a true fallback (matching
/// `list_custom_draw`'s Shortcut column and `draw_chip`'s empty-cap path)
/// rather than the guaranteed truncation an earlier pass through this task
/// left in place.
///
/// **CORRECTED 2026-08-14: this said `tok::GAP` is 8 px**, which it was until
/// the compaction pass (`1f46335`); at 8 the three terms summed to exactly
/// 50 and the budget was tight rather than loose. Keep this paragraph and
/// `toggle_glyph`'s in step. They are two prose copies of one budget, and
/// the reason the number went stale here is that only one copy was
/// re-derived when the token moved.
pub(super) unsafe fn toggle(
    nm: &NMCUSTOMDRAW,
    on: bool,
    enabled: bool,
    focused: bool,
    cache: &mut ThemeCache,
    dpi: u32,
) {
    let hdc = nm.hdc;
    let rc = nm.rc;
    let hc = cache.theme() == beckon_core::theme::Theme::HighContrast;

    // The parent's own surface first, exactly `button`'s own first line and
    // for the same reason: rounded corners and the gap between the track
    // and the caption both leave pixels this function's own shapes never
    // touch, and whatever the LAST frame put there stays unless something
    // repaints it. `COLOR_WINDOW`, not `COLOR_BTNFACE` -- see the doc above.
    let bg = cache.col(|p| p.card, COLOR_WINDOW);
    FillRect(hdc, &rc, cache.brush(bg));

    let track_w = scale(40, dpi);
    let track_h = scale(20, dpi);
    let top = rc.top + (rc.bottom - rc.top - track_h) / 2;
    // `off` is the focus ring's own outset below (2 px) -- the ring grows
    // OUTWARD from the track by `off` on every side, and `NM_CUSTOMDRAW`'s
    // `hdc` is clipped to this control's own `rc`. A track flush against
    // `rc.left` left the ring's left edge (and both left arcs) `off` px
    // past `rc.left` with nothing to clip into -- cut off. Inset the track
    // by `off` on the left instead, so `ring_rc.left` (`track.left - off`)
    // lands back exactly on `rc.left`. This mirrors `button`'s own ring
    // painter (`ring_rc` in `button`, paint.rs:1254), which computes its
    // ring as an INSET from
    // the full `rc` rather than an outset from an inner shape -- `button`
    // has margin on every side to shrink into; this track does not, so it
    // has to make its own margin on the one side (left) that had none.
    let off = scale(2, dpi);
    let track = RECT {
        left: rc.left + off,
        top,
        right: rc.left + off + track_w,
        bottom: top + track_h,
    };

    // `(fill, edge, knob)` -- disabled outranks on/off, the same precedence
    // `button`'s own `colours` gives its four tiers.
    let (fill, edge, knob) = if !enabled {
        (
            cache.col(|p| p.field, COLOR_BTNFACE),
            cache.col(|p| p.field_border, COLOR_BTNSHADOW),
            cache.col(|p| p.text_faint, COLOR_GRAYTEXT),
        )
    } else if on {
        let accent = cache.col(|p| p.accent_fill, COLOR_HIGHLIGHT);
        // No separate border colour for the filled state -- the same choice
        // `BtnTier::Accent` makes in `colours` above, where the edge pen is
        // the fill colour itself and so draws no visible seam.
        (
            accent,
            accent,
            cache.col(|p| p.accent_on, COLOR_HIGHLIGHTTEXT),
        )
    } else {
        (
            cache.col(|p| p.field, COLOR_BTNFACE),
            cache.col(|p| p.field_border, COLOR_BTNSHADOW),
            cache.col(|p| p.text_muted, COLOR_BTNTEXT),
        )
    };

    let track_brush = CreateSolidBrush(fill);
    let track_pen = CreatePen(PS_SOLID, 1, edge);
    let prev_brush = SelectObject(hdc, HGDIOBJ(track_brush.0));
    let prev_pen = SelectObject(hdc, HGDIOBJ(track_pen.0));
    let r = scale(10, dpi) * 2;
    if hc {
        let _ = Rectangle(hdc, track.left, track.top, track.right, track.bottom);
    } else {
        let _ = RoundRect(hdc, track.left, track.top, track.right, track.bottom, r, r);
    }
    if !prev_pen.is_invalid() {
        SelectObject(hdc, prev_pen);
    }
    if !prev_brush.is_invalid() {
        SelectObject(hdc, prev_brush);
    }
    let _ = DeleteObject(HGDIOBJ(track_pen.0));
    let _ = DeleteObject(HGDIOBJ(track_brush.0));

    // The knob: 14 px, inset 2 px from whichever edge it rests against --
    // left off, right on. The pen matches the fill, the same "no visible
    // seam" choice the track's own filled state makes above, so the circle
    // reads as one shape rather than a ring around a disc.
    let knob_d = scale(14, dpi);
    let inset = scale(2, dpi);
    let knob_top = track.top + (track_h - knob_d) / 2;
    let knob_left = if on {
        track.right - inset - knob_d
    } else {
        track.left + inset
    };
    let knob_brush = CreateSolidBrush(knob);
    let knob_pen = CreatePen(PS_SOLID, 1, knob);
    let prev_kb = SelectObject(hdc, HGDIOBJ(knob_brush.0));
    let prev_kp = SelectObject(hdc, HGDIOBJ(knob_pen.0));
    let _ = Ellipse(
        hdc,
        knob_left,
        knob_top,
        knob_left + knob_d,
        knob_top + knob_d,
    );
    if !prev_kp.is_invalid() {
        SelectObject(hdc, prev_kp);
    }
    if !prev_kb.is_invalid() {
        SelectObject(hdc, prev_kb);
    }
    let _ = DeleteObject(HGDIOBJ(knob_pen.0));
    let _ = DeleteObject(HGDIOBJ(knob_brush.0));

    // The caption, to the right of the track at this window's usual
    // control-to-control gap (`tok::GAP`). The raw caption text (mnemonic
    // `&` intact) with prefix processing left ON -- `button`'s own choice,
    // for the same reason: `IDC_CAPS`' caption carries `&Use`, and whether
    // the underline SHOWS is the window's UI state to say, not this
    // function's.
    let font = HFONT(
        SendMessageW(
            nm.hdr.hwndFrom,
            WM_GETFONT,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        )
        .0 as *mut core::ffi::c_void,
    );
    let prev_font = if font.is_invalid() {
        HGDIOBJ::default()
    } else {
        SelectObject(hdc, HGDIOBJ(font.0))
    };
    let ink = if enabled {
        cache.col(|p| p.text, COLOR_WINDOWTEXT)
    } else {
        cache.col(|p| p.text_faint, COLOR_GRAYTEXT)
    };
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, ink);
    let gap = scale(tok::GAP, dpi);
    let mut tr = RECT {
        left: track.right + gap,
        ..rc
    };
    let ui_state = SendMessageW(
        nm.hdr.hwndFrom,
        WM_QUERYUISTATE,
        Some(WPARAM(0)),
        Some(LPARAM(0)),
    )
    .0 as u32;
    let mut flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
    if ui_state & UISF_HIDEACCEL != 0 {
        flags |= DT_HIDEPREFIX;
    }
    let mut t = wide(&text_of(nm.hdr.hwndFrom));
    let n = t.len() - 1;
    DrawTextW(hdc, &mut t[..n], &mut tr, flags);
    if !prev_font.is_invalid() {
        SelectObject(hdc, prev_font);
    }

    // Focus ring, LAST, per `button`'s own rule -- so it never fights the
    // knob or the caption for the same pixels. 2 px `accent`, offset 2,
    // around the TRACK alone: a ring around the whole control would run
    // through the caption's own baseline. Reuses the same `off` the track
    // rect inset by above -- both the inset and this outset have to be the
    // identical value, or the ring's left edge stops landing on `rc.left`.
    if enabled && focused {
        let ring = cache.col(|p| p.accent, COLOR_HIGHLIGHT);
        let ring_rc = RECT {
            left: track.left - off,
            top: track.top - off,
            right: track.right + off,
            bottom: track.bottom + off,
        };
        let ring_pen = CreatePen(PS_SOLID, scale(2, dpi), ring);
        let null_brush = GetStockObject(NULL_BRUSH);
        let prev_pen = SelectObject(hdc, HGDIOBJ(ring_pen.0));
        let prev_brush = SelectObject(hdc, null_brush);
        if hc {
            let _ = Rectangle(
                hdc,
                ring_rc.left,
                ring_rc.top,
                ring_rc.right,
                ring_rc.bottom,
            );
        } else {
            let rr = (r + off * 2).max(0);
            let _ = RoundRect(
                hdc,
                ring_rc.left,
                ring_rc.top,
                ring_rc.right,
                ring_rc.bottom,
                rr,
                rr,
            );
        }
        if !prev_pen.is_invalid() {
            SelectObject(hdc, prev_pen);
        }
        if !prev_brush.is_invalid() {
            SelectObject(hdc, prev_brush);
        }
        let _ = DeleteObject(HGDIOBJ(ring_pen.0));
    }
}

/// The tab strip's trough: one rounded fill behind the four pills, drawn
/// from the PARENT's `WM_PAINT`.
///
/// **It is not a card and must not go through `card`.** `card` fills with
/// `p.card` and strokes a `card_border`; the trough is `p.strip` with no
/// border at all, and `compute_card_rects` deliberately does not return its
/// rect (see `strip_rect`, which is where the geometry lives). Sharing the
/// painter would mean the two surfaces stopped being able to differ.
///
/// The pills sit on top and each one fills its whole control rect with the
/// same `strip` before drawing anything (`tab_pill`), so the seam between the
/// trough this draws and the margin a pill draws is invisible by
/// construction rather than by the two rects lining up.
///
/// **`rc` is the whole band, so the trough runs the window's width. The
/// mockup's does not, and that is a known deviation rather than an
/// oversight.** `.trough` there is a shrink-to-fit flex item, i.e. it hugs
/// the four pills and reads as a segmented control; this reads as a bar with
/// four pills at its left. The caller passes `strip_rect`, which is the
/// geometry `layout` places the pills from and `compute_card_rects` takes the
/// first card's `y` out of -- one source, and the one whose left/right inset
/// carries `strip_rect`'s own resize-edge argument. Hugging the run would
/// need the run's WIDTH, which only the placement loop in `layout` computes,
/// so closing this means a second shared geometry function beside
/// `strip_rect` (the shape `compute_card_rects` already sets) rather than a
/// number invented here. Deferred deliberately: it is a look, it is one
/// function when someone decides it, and nothing about the pills, the badge
/// or the dot depends on which way it goes.
///
/// **`COLOR_BTNFACE` under high contrast, and the consequence is accepted.**
/// `accent_fill` is `COLOR_HIGHLIGHT` for the active pill, so the inactive
/// family has to differ; `COLOR_WINDOW` collides with `card` at eight sites
/// and would make the trough read as a card; and reaching for a ninth,
/// otherwise-unused index would give it no sibling site to be checked
/// against, which is exactly how the five invisible-text collisions on the
/// last redesign happened. So under HC the trough vanishes into the window
/// ground and only the active pill's `COLOR_HIGHLIGHT` distinguishes the
/// strip. Spec 6.3, gate G-S4, to be confirmed by screenshot.
pub(super) unsafe fn trough(hdc: HDC, rc: RECT, dpi: u32) {
    if rc.right <= rc.left || rc.bottom <= rc.top {
        return;
    }
    let fill = theme_col(|p| p.strip, COLOR_BTNFACE);
    let br = theme_brush(fill);
    // A pen of the fill colour, not `NULL_PEN`: `RoundRect` strokes its
    // outline with the current pen, and the stock black one would draw a
    // 1 px black frame around the trough in both themes.
    let pen = CreatePen(PS_SOLID, 1, fill);
    let prev_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let prev_brush = SelectObject(hdc, HGDIOBJ(br.0));
    let r = scale(tok::CARD_RADIUS, dpi) * 2;
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);
    if !prev_brush.is_invalid() {
        SelectObject(hdc, prev_brush);
    }
    if !prev_pen.is_invalid() {
        SelectObject(hdc, prev_pen);
    }
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

/// The corner radius of a tab pill, at 96 DPI.
///
/// 8 rather than `BTN_RADIUS`'s 6, which is the design's own pair (mockup:
/// `.trough{border-radius:10px}`, `.pill{border-radius:8px}`) sitting one
/// step inside `tok::CARD_RADIUS`. A pill is a wider, shorter shape than a
/// push button and shares no edge with one, so the two numbers never appear
/// side by side.
const PILL_RADIUS: i32 = 8;

/// A tab pill's warn dot, at 96 DPI. The mockup's `.dot{width:7px}`.
const PILL_DOT: i32 = 7;

/// Paint one of the four tab pills: the trough behind it, the pill, its
/// caption, the Shortcuts pill's count badge, its warn dot, and -- LAST --
/// the focus ring.
///
/// **A SIBLING of `button`, not a branch inside it, and the dispatch proves
/// it rather than the comment.** The pills are deliberately absent from
/// `PUSH_BUTTONS` (see `TABS` for the two reasons: `set_button_type` would
/// rewrite `BS_AUTORADIOBUTTON` into `BS_PUSHBUTTON` the first time the
/// default ring moved, and every member has to name a `DefaultButton`), and
/// `push_button_custom_draw`'s arm is gated on `is_push_button`, so a pill
/// could never have reached `button` even if the two shapes had been close
/// enough to share. They are not: a pill has no border, no tier, three
/// states rather than four, a badge and a dot.
///
/// **`active` comes from `is_checked`, never from a bit on `nm`.** A check
/// box's -- and an auto-radio's -- `NMCUSTOMDRAW` carries `CDIS_DISABLED` /
/// `CDIS_FOCUS` / `CDIS_SELECTED` / `CDIS_HOT` and nothing that means
/// "ticked"; `CDIS_SELECTED` means the mouse is DOWN on it. `caps_custom_draw`
/// took the identical decision for `IDC_CAPS` and for the identical reason,
/// and `BM_GETCHECK` answering an auto-radio 1/0 was measured on a14
/// 2026-08-14 (gate G-S3) rather than assumed.
///
/// **Three states, and the ink swaps with the ground on the hover one.**
/// Active is `accent_fill` under `accent_on`; inactive is `strip` under
/// `text_muted`; hover is `strip_hover` under **`text`**. That last swap is
/// load-bearing, not decorative: `text_muted` on `strip_hover` measures
/// 3.700 Light / 4.304 Dark, so a hover that moved only the ground would drop
/// the label under 4.5 in both themes. `Palette::strip_hover` carries the
/// figures; `pairs()` carries the guard.
///
/// **The fill is `accent_fill` and never `accent`.** `accent_on` on
/// `DARK.accent` measures 3.044, and no row in `theme::pairs` covers that
/// combination -- so a pill filled with `accent` would carry text under the
/// 4.5 floor with every test still green.
///
/// `CDIS_HOT` reaching a `BS_AUTORADIOBUTTON | BS_PUSHLIKE` at all is gate
/// G2, **passed on a14 2026-08-14 under comctl32 6.16**, with a plain
/// `BS_PUSHBUTTON` reporting hot in the same run as the control that makes
/// a clean result mean anything (`examples/pill_probe.rs`).
///
/// **High contrast is read as `cache.theme()`, never `high_contrast()`** --
/// `button`'s own rule and reason: that `Cell` refreshes only on
/// `WM_SETTINGCHANGE(SPI_SETHIGHCONTRAST)`, while `WM_THEMECHANGED` alone
/// already rebuilds `ThemeCache`, so a paint racing the two would see a stale
/// value. Under HC the pill flattens to `Rectangle`, as six other sites in
/// this file do -- a soft edge under a theme built on flat fills and hard
/// borders reads as a rendering artefact rather than as a control.
///
/// **There is no disabled state and nothing disables a pill.** The strip is
/// chrome: it is absent from `PAGE_CONTROLS`, `show_page_controls` never
/// touches it, and no `enable` call in `apply_state` names a `IDC_TAB_*`. If
/// one ever does, this function needs a `CDIS_DISABLED` arm and `pairs()`
/// needs the row to go with it; today it would draw a live-looking pill.
pub(super) unsafe fn tab_pill(
    nm: &NMCUSTOMDRAW,
    active: bool,
    badge: Option<usize>,
    warn: bool,
    cache: &mut ThemeCache,
    dpi: u32,
) {
    let hdc = nm.hdc;
    let rc = nm.rc;
    let hc = cache.theme() == beckon_core::theme::Theme::HighContrast;
    let hot = nm.uItemState.0 & CDIS_HOT.0 != 0;
    let focused = nm.uItemState.0 & CDIS_FOCUS.0 != 0;

    // The trough first, across the WHOLE control rect. `CDRF_SKIPDEFAULT`
    // means nothing else ever paints these pixels, and the control is
    // deliberately bigger than the pill: `layout` gives it `FOCUS_SLACK` of
    // margin on all four sides, which is where the focus ring lives and
    // which is also how two neighbouring pills -- placed with no gap between
    // their controls -- come to look 6 px apart. That margin is trough, so
    // this is the trough colour, not `bg`.
    let strip = cache.col(|p| p.strip, COLOR_BTNFACE);
    FillRect(hdc, &rc, cache.brush(strip));

    let slack = scale(tok::FOCUS_SLACK, dpi);
    let pill = RECT {
        left: rc.left + slack,
        top: rc.top + slack,
        right: rc.right - slack,
        bottom: rc.bottom - slack,
    };

    // `(fill, ink)`. Active outranks hover, which is why a lit pill does not
    // change under the pointer: it is already the answer.
    let (fill, ink) = if active {
        (
            cache.col(|p| p.accent_fill, COLOR_HIGHLIGHT),
            cache.col(|p| p.accent_on, COLOR_HIGHLIGHTTEXT),
        )
    } else if hot {
        (
            cache.col(|p| p.strip_hover, COLOR_BTNFACE),
            cache.col(|p| p.text, COLOR_BTNTEXT),
        )
    } else {
        // Ground and ink are both the resting pair, and the fill is drawn
        // even though it equals the `FillRect` above: an inactive pill leaving
        // its interior to that fill would be right today and wrong the moment
        // the trough and the resting pill stop being the same token.
        (strip, cache.col(|p| p.text_muted, COLOR_BTNTEXT))
    };

    let brush = CreateSolidBrush(fill);
    // The outline pen is the fill colour, so a pill draws as one shape rather
    // than as a ring around one -- `toggle`'s track and `BtnTier::Accent`
    // both make the same choice, and for the same reason: there is no border
    // token in this design's pill.
    let pen = CreatePen(PS_SOLID, 1, fill);
    let prev_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let prev_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    if hc {
        let _ = Rectangle(hdc, pill.left, pill.top, pill.right, pill.bottom);
    } else {
        let r = scale(PILL_RADIUS, dpi) * 2;
        let _ = RoundRect(hdc, pill.left, pill.top, pill.right, pill.bottom, r, r);
    }
    if !prev_pen.is_invalid() {
        SelectObject(hdc, prev_pen);
    }
    if !prev_brush.is_invalid() {
        SelectObject(hdc, prev_brush);
    }
    let _ = DeleteObject(HGDIOBJ(pen.0));
    let _ = DeleteObject(HGDIOBJ(brush.0));

    // The content box: the pill less its own left/right padding. The badge
    // takes a FIXED slot off the right of it -- fixed, because `layout` sized
    // this control with the same slot reserved and neither of them may vary
    // with the count. A slot that grew with the number would make the pill's
    // width a function of the data, and the only way to apply a new width is
    // `layout`, which is `SetWindowPos` on the populated App combo: the
    // measured data-loss call (`Ui::shown_external`). `badge_slot_w` is the
    // one arithmetic both sides run.
    let pad = scale(tok::TAB_PAD_X, dpi);
    let parent = GetParent(nm.hdr.hwndFrom).unwrap_or(nm.hdr.hwndFrom);
    let slot = if badge.is_some() {
        badge_slot_w(parent, dpi)
    } else {
        0
    };
    let content = RECT {
        left: pill.left + pad,
        top: pill.top,
        right: pill.right - pad,
        bottom: pill.bottom,
    };

    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, ink);
    let ui_state = SendMessageW(
        nm.hdr.hwndFrom,
        WM_QUERYUISTATE,
        Some(WPARAM(0)),
        Some(LPARAM(0)),
    )
    .0 as u32;
    let mut flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
    if ui_state & UISF_HIDEACCEL != 0 {
        flags |= DT_HIDEPREFIX;
    }
    // The caption from the control, one string, no second field -- `button`'s
    // rule. It carries no `&` today (see `mod cap`: four unique mnemonics do
    // not exist), and the prefix handling above does not depend on that
    // staying true.
    let font = HFONT(
        SendMessageW(
            nm.hdr.hwndFrom,
            WM_GETFONT,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        )
        .0 as *mut core::ffi::c_void,
    );
    let prev_font = if font.is_invalid() {
        HGDIOBJ::default()
    } else {
        SelectObject(hdc, HGDIOBJ(font.0))
    };
    let mut caption_rc = RECT {
        right: content.right - slot,
        ..content
    };
    let mut t = wide(&text_of(nm.hdr.hwndFrom));
    let n = t.len() - 1;
    DrawTextW(hdc, &mut t[..n], &mut caption_rc, flags);
    if !prev_font.is_invalid() {
        SelectObject(hdc, prev_font);
    }

    // The badge, in the slot, in the window's one small face. Same ink as the
    // caption: the mockup dims it with `opacity:.75`, which GDI text has no
    // equivalent for, and a fifth colour would need its own `pairs()` row
    // against all three grounds for a purely decorative tint. The size step
    // (Keycap 11 against Body 14) is what separates the number from the word.
    //
    // **`cap_font()`, and that is what makes the slot honest**: `badge_slot_w`
    // measures in the same handle, so the reserved width and the drawn width
    // are the same measurement rather than two that agree today. See
    // `role_of`'s `Hold` chip arm for the same rule stated once already.
    if let Some(count) = badge {
        let bf = cap_font().unwrap_or(font);
        let prev = if bf.is_invalid() {
            HGDIOBJ::default()
        } else {
            SelectObject(hdc, HGDIOBJ(bf.0))
        };
        let mut badge_rc = RECT {
            left: content.right - slot,
            ..content
        };
        let mut b = wide(&count.to_string());
        let bn = b.len() - 1;
        DrawTextW(
            hdc,
            &mut b[..bn],
            &mut badge_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        if !prev.is_invalid() {
            SelectObject(hdc, prev);
        }
    }

    // The warn dot, in the pill's top-right corner.
    //
    // **A drawn `Ellipse`, never the character U+25CF.** This window carries a
    // text face, not a symbol one, and a face without the glyph draws a box --
    // which reads as a rendering bug rather than as a warning. beckon has been
    // here once already: an em-dash written to `serve --log` came back as
    // `?"` under Windows PowerShell 5.1's ANSI default.
    //
    // **It costs no width, and that is the point of putting it in the
    // corner.** The dot appears and disappears with `external_change` -- a
    // data push -- so a dot that took horizontal room would make the pill's
    // width data-dependent, and applying a new width means `layout`. The
    // corner is empty: the caption is centred in a box already inset by
    // `TAB_PAD_X`, so the nearest ink is `pad - PILL_DOT - 2*inset` away.
    //
    // It fits INSIDE the rounded corner rather than being clipped by it. At
    // 96 DPI the arc's centre is `PILL_RADIUS` (8) in from each edge and the
    // dot's is 5.5 in from each, so the dot's centre is 3.54 from the arc's
    // centre and its far edge 7.04 -- inside 8, with the margin shrinking to
    // nothing only if `PILL_DOT` grows past 9.
    //
    // Never drawn on a lit pill, and that is structural: `warn_dot_shown` is
    // the complement of `banner_shown` within `external_change`, so the door
    // whose pill would carry a dot is the door showing the banner instead.
    // `warn` on `accent_fill` measures 1.212 in Light and has no `pairs()`
    // row; `settings::the_dot_is_never_on_the_door_that_is_open` is what keeps
    // it that way. On `strip` it is 4.609 / 7.857 and on `strip_hover`
    // 3.772 / 6.457, both past SC 1.4.11's 3.0 non-text floor.
    if warn {
        let d = scale(PILL_DOT, dpi);
        let inset = scale(2, dpi);
        let x = pill.right - inset - d;
        let y = pill.top + inset;
        // `COLOR_WINDOWTEXT` under high contrast: the pill's own ground is
        // `COLOR_BTNFACE` there (inactive, which is the only state a dot is
        // drawn in), and `COLOR_BTNTEXT` is already the caption beside it --
        // a warning that draws in the caption's colour is not a warning. The
        // two indices are a matched system pair, per this file's rule that
        // fill and ink never share one.
        let c = cache.col(|p| p.warn, COLOR_WINDOWTEXT);
        let dot_brush = CreateSolidBrush(c);
        let dot_pen = CreatePen(PS_SOLID, 1, c);
        let pb = SelectObject(hdc, HGDIOBJ(dot_brush.0));
        let pp = SelectObject(hdc, HGDIOBJ(dot_pen.0));
        let _ = Ellipse(hdc, x, y, x + d, y + d);
        if !pp.is_invalid() {
            SelectObject(hdc, pp);
        }
        if !pb.is_invalid() {
            SelectObject(hdc, pb);
        }
        let _ = DeleteObject(HGDIOBJ(dot_pen.0));
        let _ = DeleteObject(HGDIOBJ(dot_brush.0));
    }

    // The focus ring, LAST, per `button`'s rule -- so it never fights the
    // caption, the badge or the dot for the same pixels.
    //
    // **Drawn in the `FOCUS_SLACK` margin, which is what the token is named
    // for.** `button` insets its ring INTO the control because it has margin
    // on every side to shrink into; a pill has none -- its content box is
    // exactly its caption -- so the ring goes outside the pill instead, in the
    // 3 px `layout` left around it. Inset by 1 from the control rect rather
    // than drawn on it, so the whole 2 px stroke lands inside `rc` and none of
    // it is clipped: `NM_CUSTOMDRAW`'s `hdc` is clipped to this control.
    //
    // **CORRECTED 2026-08-14: `accent` in every state, lit included, and the
    // `accent_on` swap this used to make was a pair nothing measured.** The
    // swap came from `BtnTier::Accent`, where it is right and for a reason
    // that does not travel: that ring is inset 2 px INTO a control whose
    // whole rect the tier has already filled with `accent_fill`, so it is
    // genuinely accent-on-accent -- 1.000 in Light (identical hex), 1.489 in
    // Dark -- and white is the only ink that survives there.
    //
    // This ring is never on that fill. `FillRect(&rc, strip)` at the top of
    // this function paints the whole control; the pill is `rc` inset by
    // `FOCUS_SLACK`; and the stroke reaches at most `scale(1) + scale(2)/2`
    // in from `rc` -- 2 of 3 at 96 DPI, 2 of 3 at 120, 3 of 4 at 144, 4 of 6
    // at 192, 5 of 7 at 240 -- so at every DPI it stops at least one device
    // pixel short of the pill and its ground is the TROUGH in all three
    // states. `accent_on` on `strip` measures **1.360** in Light: the lit
    // pill's only keyboard indication was invisible, and no row in
    // `theme::pairs` covered the combination, which is why every test stayed
    // green. It measures 12.806 on `DARK.strip`, so a dark-mode screenshot
    // would not have caught it either.
    //
    // `accent` on `strip` is **3.802 Light / 4.208 Dark**, past SC 1.4.11's
    // 3.0 non-text floor in both themes and covered by `pairs()`'s "pill
    // focus ring on strip" -- one row, because there is one ground. Figures
    // re-derived with `beckon_core::theme::contrast`.
    //
    // It closes a high-contrast collision as well, by the same argument. Under
    // HC `col` ignores the palette and returns the system index, so the ring
    // was `COLOR_HIGHLIGHTTEXT` on a margin painted `COLOR_BTNFACE` -- and in
    // the High Contrast White scheme those are both white. `COLOR_HIGHLIGHT`
    // on `COLOR_BTNFACE` is the pairing selection already depends on being
    // visible, in every scheme. This is the file's own "a fill and its ink
    // must not share a `GetSysColor` index" rule, failing one index apart from
    // where it is usually checked.
    if focused && ui_state & UISF_HIDEFOCUS == 0 {
        let ring = cache.col(|p| p.accent, COLOR_HIGHLIGHT);
        let d = scale(1, dpi);
        let ring_rc = RECT {
            left: rc.left + d,
            top: rc.top + d,
            right: rc.right - d,
            bottom: rc.bottom - d,
        };
        let ring_pen = CreatePen(PS_SOLID, scale(2, dpi), ring);
        let null_brush = GetStockObject(NULL_BRUSH);
        let pp = SelectObject(hdc, HGDIOBJ(ring_pen.0));
        let pb = SelectObject(hdc, null_brush);
        if hc {
            let _ = Rectangle(
                hdc,
                ring_rc.left,
                ring_rc.top,
                ring_rc.right,
                ring_rc.bottom,
            );
        } else {
            // The ring's rect is `FOCUS_SLACK - 1` px outside the pill's on
            // every side, so its radius is the pill's plus that -- derived,
            // not chosen, or the ring's arcs stop being concentric with the
            // corners they trace.
            let r = scale(PILL_RADIUS + tok::FOCUS_SLACK - 1, dpi) * 2;
            let _ = RoundRect(
                hdc,
                ring_rc.left,
                ring_rc.top,
                ring_rc.right,
                ring_rc.bottom,
                r,
                r,
            );
        }
        if !pp.is_invalid() {
            SelectObject(hdc, pp);
        }
        if !pb.is_invalid() {
            SelectObject(hdc, pb);
        }
        let _ = DeleteObject(HGDIOBJ(ring_pen.0));
    }
}

/// A rounded 1 px border around `ctl`'s own rect, stroked from the PARENT.
///
/// **`IDC_APP` and `IDC_FILTER` are never owner-drawn**, and this function
/// is why that is still enough to look themed. Both keep their native
/// EDIT/COMBOBOX rendering in full -- comctl32 owns their interior, their
/// caret, their selection and, for `IDC_APP`, its edit child's typing path,
/// exactly as before this task; colour comes from `WM_CTLCOLOREDIT` /
/// `WM_CTLCOLORLISTBOX` in `mod.rs`, not from here. This function touches
/// only the PARENT's own device context, at a rect it computes itself and
/// then strokes with `NULL_BRUSH` selected -- nothing here fills, so the
/// control's own later repaint of its interior can never be erased by it.
/// Rounding the rect OUTWARD by one device pixel, rather than stroking the
/// control's own edge, is what keeps the border entirely outside the
/// control's bounds so the two paints cannot collide.
///
/// An owner-drawn `CBS_DROPDOWN` with an edit child is the exact shape that
/// produced this project's measured data-loss defect (`Ui::shown_external`,
/// see the module header). This design cannot reach the same place, because
/// it never asks Windows to hand either control's own paint over to it.
///
/// `focused` widens the stroke to 2 px `accent`, the same weight `button`'s
/// own focus ring uses. The caller (`WM_PAINT`) reads `GetFocus()` fresh
/// each time it runs, so this never needs its own notification plumbing to
/// stay correct -- `handle_command`'s `CBN_SETFOCUS`/`EN_SETFOCUS` arms
/// exist only to ask for the repaint, never to hand this function an
/// answer.
///
/// **`IsWindowVisible`, and it is load-bearing since the tab strip.** Both
/// controls this is called for live on the Shortcuts page, and a hidden
/// window keeps its window rect -- so without this test the parent would
/// keep stroking two rounded rectangles at the App combo's and the filter
/// box's last positions from every other page, with nothing inside them.
/// The control's own `WM_PAINT` stops when it is hidden; this border is
/// drawn by the PARENT, which is exactly why it does not stop on its own.
pub(super) unsafe fn field_border(
    hdc: HDC,
    ctl: HWND,
    parent: HWND,
    cache: &mut ThemeCache,
    focused: bool,
    dpi: u32,
) {
    if ctl.is_invalid() || !IsWindowVisible(ctl).as_bool() {
        return;
    }
    let mut wr = RECT::default();
    if GetWindowRect(ctl, &mut wr).is_err() {
        return;
    }
    let mut tl = POINT {
        x: wr.left,
        y: wr.top,
    };
    let mut br = POINT {
        x: wr.right,
        y: wr.bottom,
    };
    if !ScreenToClient(parent, &mut tl).as_bool() || !ScreenToClient(parent, &mut br).as_bool() {
        return;
    }
    let out = scale(1, dpi);
    let rc = RECT {
        left: tl.x - out,
        top: tl.y - out,
        right: br.x + out,
        bottom: br.y + out,
    };
    let hc = cache.theme() == beckon_core::theme::Theme::HighContrast;
    let (edge, w) = if focused {
        (cache.col(|p| p.accent, COLOR_HIGHLIGHT), scale(2, dpi))
    } else {
        (
            cache.col(|p| p.field_border, COLOR_BTNSHADOW),
            scale(1, dpi),
        )
    };
    let pen = CreatePen(PS_SOLID, w, edge);
    let null_brush = GetStockObject(NULL_BRUSH);
    let prev_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let prev_brush = SelectObject(hdc, null_brush);
    if hc {
        let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
    } else {
        let r = scale(BTN_RADIUS, dpi) * 2;
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);
    }
    if !prev_pen.is_invalid() {
        SelectObject(hdc, prev_pen);
    }
    if !prev_brush.is_invalid() {
        SelectObject(hdc, prev_brush);
    }
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

/// Ask the parent to repaint the decorative ring `field_border` draws
/// around `ctl`.
///
/// Neither `IDC_APP` nor `IDC_FILTER` repaints that pixel itself on a focus
/// change -- it is the PARENT's paint, not the control's -- so without this
/// the ring drawn on arrival would keep showing the old state until
/// something unrelated invalidated the window. The padding is generous and
/// unscaled on purpose: over-invalidating by a few device pixels costs one
/// slightly larger `WM_PAINT`, while under-invalidating leaves a stale ring
/// on screen, and this is called from a focus notification, not a paint --
/// there is no `dpi` in scope to size it precisely without a `GetDpiForWindow`
/// call this cheap operation does not need.
pub(super) unsafe fn invalidate_field_border(parent: HWND, ctl: HWND) {
    if ctl.is_invalid() {
        return;
    }
    let mut wr = RECT::default();
    if GetWindowRect(ctl, &mut wr).is_err() {
        return;
    }
    let mut tl = POINT {
        x: wr.left,
        y: wr.top,
    };
    let mut br = POINT {
        x: wr.right,
        y: wr.bottom,
    };
    if !ScreenToClient(parent, &mut tl).as_bool() || !ScreenToClient(parent, &mut br).as_bool() {
        return;
    }
    let pad = 8;
    let rc = RECT {
        left: tl.x - pad,
        top: tl.y - pad,
        right: br.x + pad,
        bottom: br.y + pad,
    };
    let _ = InvalidateRect(Some(parent), Some(&rc), false);
}

/// Paint one row of `IDC_COMBO` (the key list) or `IDC_TAP` (the Caps-tap
/// list) -- and, with the same call, the closed box's own display, which
/// Windows draws through this same message for whichever item is currently
/// selected.
///
/// **Both are `CBS_DROPDOWNLIST` with `CBS_OWNERDRAWFIXED` and no edit
/// child.** Unlike `IDC_APP` there is nothing here that can go stale: the
/// text comes back from the SAME constant table each control was populated
/// from (`key_table()`, `cap::TAP_ITEMS`), indexed by `di.itemID` -- never
/// read back from the control (no `CBS_HASSTRINGS` is needed for that
/// reason) and never from `UI` (`CAP_FONT`'s reason: a paint can arrive
/// while it is borrowed, and it does). Both were filled WITHOUT `CBS_SORT`
/// (see each creation site's own comment), which is what makes `di.itemID`
/// a safe index into these same arrays in the same order.
///
/// `di.itemID == u32::MAX` is `CB_ERR`'s own value, sent for the closed
/// display of a combo with nothing selected yet -- filled with the row's
/// own surface and left blank, the same "nothing to draw" shape
/// `draw_keycaps`' empty check takes.
///
/// `ODS_SELECTED` here means the LISTBOX sense -- "this row is the
/// highlighted one" -- not the BUTTON sense `button` reads as "currently
/// pressed"; same bit name, different control, different meaning, so this
/// function names its own local `picked` rather than reusing `pressed`.
pub(super) unsafe fn draw_combo_item(di: &DRAWITEMSTRUCT, cache: &mut ThemeCache, dpi: u32) {
    if di.CtlType != ODT_COMBOBOX {
        return;
    }
    let hdc = di.hDC;
    let rc = di.rcItem;
    let id = di.CtlID as i32;
    let disabled = di.itemState.0 & ODS_DISABLED.0 != 0;
    let picked = di.itemState.0 & ODS_SELECTED.0 != 0;

    let field = cache.col(|p| p.field, COLOR_WINDOW);
    let (fill, ink) = if disabled {
        (field, cache.col(|p| p.text_faint, COLOR_GRAYTEXT))
    } else if picked {
        // The same treatment the Shortcut column's own selected row takes
        // (`list_custom_draw`): `accent_soft`, not the stronger
        // `accent_fill` an armed chip or `Save` gets, and `p.text` rather
        // than `p.accent` for the ink -- already shipped, already read as
        // legible on that pale a tint in both themes.
        (
            cache.col(|p| p.accent_soft, COLOR_HIGHLIGHT),
            cache.col(|p| p.text, COLOR_HIGHLIGHTTEXT),
        )
    } else {
        (field, cache.col(|p| p.text, COLOR_WINDOWTEXT))
    };
    FillRect(hdc, &rc, cache.brush(fill));

    if di.itemID == u32::MAX {
        return;
    }
    let text = if id == IDC_COMBO {
        key_table().get(di.itemID as usize).map(|k| k.name.clone())
    } else {
        cap::TAP_ITEMS
            .get(di.itemID as usize)
            .map(|s| s.to_string())
    };
    let Some(text) = text else {
        return;
    };

    let font = HFONT(
        SendMessageW(di.hwndItem, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0
            as *mut core::ffi::c_void,
    );
    let font = if font.is_invalid() {
        HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
    } else {
        font
    };
    let prev_font = SelectObject(hdc, HGDIOBJ(font.0));
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, ink);
    let pad = scale(6, dpi);
    let mut tr = RECT {
        left: rc.left + pad,
        ..rc
    };
    let mut t = wide(&text);
    let n = t.len() - 1;
    DrawTextW(
        hdc,
        &mut t[..n],
        &mut tr,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    if !prev_font.is_invalid() {
        SelectObject(hdc, prev_font);
    }
}

/// The dot's diameter, at 96 DPI. `scale`d like every other literal in this
/// file.
const NOTE_DOT_D: i32 = 7;

/// The gap between the dot's right edge and where a note's text starts.
/// Matches `pad` everywhere else in this file a leading glyph precedes body
/// text (`draw_combo_item`, `header_custom_draw`'s title, the Shortcut
/// column's own ellipsis fallback) -- one number, not a fresh guess here.
const NOTE_TEXT_GAP: i32 = 6;

/// A severity WORD for `IDC_NOTES` under high contrast ONLY, prepended to a
/// note's own text in place of the dot `draw_notes` skips there. See
/// `draw_notes`'s own doc for why high contrast does not lean on colour
/// here at all.
///
/// **Not `mark_glyph`.** That function (deleted, Task 12) served every
/// theme and existed to keep a proportional-font glyph column aligned by
/// padding the shorter string -- a problem this task removed by drawing a
/// dot at a fixed x instead. This is a NEW, narrowly-scoped decision for one
/// theme branch, and it says a WORD rather than a symbol precisely because a
/// high-contrast user, who is disproportionately likely to be running a
/// screen reader, is exactly the audience an ASCII glyph like `!!` served
/// worst. `Ok` stays silent, matching the "a healthy row says nothing" rule
/// the deleted function documented for the very same mark.
fn hc_severity_word(m: Mark) -> &'static str {
    match m {
        Mark::Ok => "",
        Mark::Warn => "Warning: ",
        Mark::Bad => "Error: ",
        Mark::Unknown => "Note: ",
    }
}

/// Paint `IDC_NOTES` (Task 12): one line per note, a coloured dot at a fixed
/// x, then the note's own text in `Role::Caption` at `text_muted`.
///
/// **Owner-draw now covers the WHOLE control, background included** --
/// `draw_chip`'s own rule, for the same reason: `SS_OWNERDRAW` is a
/// different VALUE of a STATIC's type field, not a flag beside `SS_LEFT`, so
/// nothing paints this control's background but this function.
/// `WM_CTLCOLORSTATIC` no longer reaches `IDC_NOTES` at all -- an owner-draw
/// static never asks for one -- so it is removed from that arm's id list in
/// `mod.rs`. `COLOR_WINDOW`, not `COLOR_BTNFACE`: `card`'s fallback
/// everywhere else THIS FILE paints it (`card`, `list_custom_draw`'s resting
/// row, `toggle`'s own wash, whose doc names this exact reasoning), not the
/// `COLOR_BTNFACE` the old `WM_CTLCOLORSTATIC` arm used, which this control
/// no longer reaches.
///
/// **The dot's x is fixed, never measured.** The old `!`/`!!` scheme kept
/// two notes aligned with a trailing space baked into the shorter glyph,
/// which the deleted `mark_glyph`'s own doc measured as never quite equal
/// across four marks and two DPIs (up to 15 px of drift). A dot painted at
/// `rc.left` cannot drift: every note's text starts at `rc.left + dot
/// diameter + gap` whether its mark is `Ok` or `Bad`.
///
/// **High contrast: colour does not distinguish the four marks; the word
/// does, and no dot is drawn at all.** `ThemeCache::col` answers
/// `GetSysColor` for every one of the four dot colours under
/// `Theme::HighContrast`, and the system palette an actual high-contrast
/// theme ships is a small, THEME-CHOSEN set -- there is no portable way to
/// prove four `sys` indices resolve to four visually distinct colours on
/// every high-contrast theme a user might have picked, and this host cannot
/// run Windows to check one. So this function does not gamble on it: the
/// dot is skipped entirely under high contrast (the `sys` fallbacks passed
/// to `cache.col` below for the four marks are consequently dead code on
/// that branch, kept only because `col`'s signature requires one --
/// `flag_colours`' own doc names the same situation), and `hc_severity_word`
/// prepends a WORD to the note's text instead. This shares the philosophy
/// `draw_flag_pill` uses for the App column's flag (do not lean on colour
/// alone under high contrast), but not its mechanism: `draw_flag_pill` returns
/// `CDRF_DODEFAULT` to let comctl32 render existing text, while `draw_notes`
/// synthesizes the severity word because this control is fully `SS_OWNERDRAW`
/// and would render nothing otherwise.
///
/// Font and line height come from `di.hwndItem`'s own `WM_GETFONT` and an
/// "Ag" measurement, matching `notes_height`'s own calculation in the same
/// `Role::Caption` font -- so what is painted here never exceeds the
/// two-line budget that function reserves, as long as `notes` holds at most
/// two entries (`show_notes`'s own cap in `mod.rs`, unchanged by this task).
pub(super) unsafe fn draw_notes(
    di: &DRAWITEMSTRUCT,
    notes: &[Note],
    cache: &mut ThemeCache,
    dpi: u32,
) {
    let hdc = di.hDC;
    let rc = di.rcItem;
    let hc = cache.theme() == beckon_core::theme::Theme::HighContrast;

    let bg = cache.col(|p| p.card, COLOR_WINDOW);
    FillRect(hdc, &rc, cache.brush(bg));

    if notes.is_empty() {
        return;
    }

    let font = HFONT(
        SendMessageW(di.hwndItem, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0
            as *mut core::ffi::c_void,
    );
    let font = if font.is_invalid() {
        HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
    } else {
        font
    };
    let prev_font = SelectObject(hdc, HGDIOBJ(font.0));

    // The same "Ag" measurement `notes_height` uses, in the SAME font --
    // so this can never paint more than the two lines that function
    // budgeted, as long as the caller respects the two-entry cap.
    let mut sz = SIZE::default();
    let ag = wide("Ag");
    let _ = GetTextExtentPoint32W(hdc, &ag[..ag.len() - 1], &mut sz);
    let line_h = if sz.cy > 0 { sz.cy } else { scale(16, dpi) };

    let dot_d = scale(NOTE_DOT_D, dpi);
    let dot_x = rc.left;
    let text_x = dot_x + dot_d + scale(NOTE_TEXT_GAP, dpi);
    let ink = cache.col(|p| p.text_muted, COLOR_WINDOWTEXT);

    SetBkMode(hdc, TRANSPARENT);
    let mut y = rc.top;
    for n in notes {
        if !hc {
            let dot = match n.mark {
                Mark::Ok => cache.col(|p| p.ok, COLOR_WINDOWTEXT),
                Mark::Warn => cache.col(|p| p.warn, COLOR_WINDOWTEXT),
                Mark::Bad => cache.col(|p| p.bad, COLOR_WINDOWTEXT),
                Mark::Unknown => cache.col(|p| p.text_faint, COLOR_GRAYTEXT),
            };
            let dot_top = y + (line_h - dot_d) / 2;
            let brush = CreateSolidBrush(dot);
            let pen = CreatePen(PS_SOLID, 1, dot);
            let prev_brush = SelectObject(hdc, HGDIOBJ(brush.0));
            let prev_pen = SelectObject(hdc, HGDIOBJ(pen.0));
            let _ = Ellipse(hdc, dot_x, dot_top, dot_x + dot_d, dot_top + dot_d);
            if !prev_pen.is_invalid() {
                SelectObject(hdc, prev_pen);
            }
            if !prev_brush.is_invalid() {
                SelectObject(hdc, prev_brush);
            }
            let _ = DeleteObject(HGDIOBJ(pen.0));
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }

        SetTextColor(hdc, ink);
        let mut tr = RECT {
            left: text_x,
            top: y,
            right: rc.right,
            bottom: y + line_h,
        };
        let line = if hc {
            format!("{}{}", hc_severity_word(n.mark), n.text)
        } else {
            n.text.clone()
        };
        let mut t = wide(&line);
        let tn = t.len() - 1;
        DrawTextW(
            hdc,
            &mut t[..tn],
            &mut tr,
            DT_LEFT | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        y += line_h;
    }

    if !prev_font.is_invalid() {
        SelectObject(hdc, prev_font);
    }
}
