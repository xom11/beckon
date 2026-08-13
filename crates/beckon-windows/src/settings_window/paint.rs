//! Drawing. Behaviour is unchanged by the split that created this file --
//! every hazard comment below travelled with its code. Message *dispatch*
//! (`WM_DRAWITEM`, `WM_NOTIFY` / `NM_CUSTOMDRAW`) stays in `mod.rs`; this
//! file holds only the painters those handlers call into.

// GetSysColorBrush must not appear in this file. It returns a brush owned by
// Windows, while ThemeCache::brush returns one owned by us; a call site that
// cannot tell them apart is a double-free waiting for a theme switch. Every
// colour goes through `col`, which answers the high-contrast branch itself.

use super::*;

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
    // read as the most prominent thing in the band. `.wtog.dis` puts
    // `#f7f7f7` on a `#f3f3f3` window, i.e. it deliberately sinks BACK into
    // the surface. Only the ink and the face change; the box and its edge
    // stay, so the shape survives.
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
    // `CDRF_SKIPDEFAULT` means we own the background too, not only the text.
    // Getting this wrong shows up as a selected row with one un-highlighted
    // cell, which is worse than no keycaps at all. `theme_brush` returns a
    // brush this window owns, never a system one -- see the ban at the top
    // of this file. A selected row takes `accent_soft`, not the stronger
    // `accent_fill` an armed chip or `Save` gets: a full-strength fill this
    // close to a column of keycaps would fight them for attention.
    let bg = if sel {
        theme_col(|p| p.accent_soft, COLOR_HIGHLIGHT)
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
                theme_col(|p| p.accent_on, COLOR_HIGHLIGHTTEXT)
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
