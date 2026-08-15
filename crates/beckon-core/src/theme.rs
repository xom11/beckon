//! Colour as data, so the two CI jobs that are not Windows can check it.
//!
//! `beckon-windows` converts a token to `COLORREF` at its boundary and holds
//! no literal of its own. The contrast test at the bottom of this file is the
//! reason the table lives here: the first hand-written pass failed five pairs,
//! including a dark accent FILL too light to carry white text and a card
//! border invisible against the window ground.

/// Every colour the settings window draws, as `0xRRGGBB`.
///
/// `accent` and `accent_fill` are deliberately separate. A colour that reads
/// well as text on a card and a colour that carries white text on top of it
/// are different constraints, and in dark mode they resolve to different
/// values. Collapsing them is the defect this struct's shape prevents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub bg: u32,
    pub card: u32,
    pub card_border: u32,
    /// The trough the tab pills sit in.
    ///
    /// **In dark mode this is LIGHTER than the card, and that is forced by
    /// arithmetic rather than taste.** No colour darker than `DARK.bg` can
    /// clear the 1.2 border floor against it -- pure black, the far end,
    /// reaches only 1.171 -- so a dark trough has to move away from the
    /// ground upward. The light half is symmetric: pure white against
    /// `LIGHT.bg` is 1.101, which forecloses a near-white trough the same
    /// way. Both figures are pinned by
    /// `the_trough_can_only_move_away_from_the_ground_one_way`, because a
    /// reader who does not believe them will reach for exactly those two
    /// extremes first.
    pub strip: u32,
    /// Hover on an inactive pill.
    ///
    /// `LIGHT` is `#C2C9D8` and not the design's `#CBD1DE`, which measures
    /// 1.126 against `strip` -- under the 1.2 border floor, i.e. a hover
    /// state that cannot be seen. The design states the floor and then gives
    /// a value that fails it; the floor wins. Restoring `#CBD1DE` needs no
    /// new guard: it fails the `strip_hover on strip` row in `pairs()`.
    ///
    /// The ink changes with the state -- an inactive pill draws `text_muted`
    /// on `strip` and `text` on `strip_hover` -- and that swap is
    /// load-bearing rather than decorative. `text_muted` on `strip_hover`
    /// measures **3.700 / 4.304**, so a hover that moved only the ground
    /// would drop the label under 4.5 in both themes.
    ///
    /// **CORRECTED 2026-08-14.** The plan and spec quote that same pair as
    /// "4.015 / 4.304". 4.015 is `text_muted` on the design's rejected
    /// `#CBD1DE`, i.e. measured before this token moved; against the
    /// `#C2C9D8` that shipped it is 3.700. Recomputed with `contrast` in
    /// this file. The conclusion is unchanged and the margin is wider.
    pub strip_hover: u32,
    pub text: u32,
    pub text_muted: u32,
    pub text_faint: u32,
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_fill: u32,
    pub accent_on: u32,
    pub accent_soft: u32,
    pub field: u32,
    pub field_border: u32,
    pub keycap: u32,
    pub keycap_edge: u32,
    pub bad_bg: u32,
    pub bad: u32,
    pub warn_bg: u32,
    pub warn: u32,
    pub ok: u32,
    pub divider: u32,
}

pub const LIGHT: Palette = Palette {
    bg: 0xF2F4F8,
    card: 0xFFFFFF,
    card_border: 0xDCE0E8,
    strip: 0xD9DDE7,
    strip_hover: 0xC2C9D8,
    text: 0x15181E,
    text_muted: 0x5A6270,
    text_faint: 0x6F7785,
    accent: 0x2563EB,
    accent_hover: 0x1D4FD7,
    accent_fill: 0x2563EB,
    accent_on: 0xFFFFFF,
    accent_soft: 0xE8F0FF,
    field: 0xFFFFFF,
    field_border: 0xD2D8E3,
    keycap: 0xFFFFFF,
    keycap_edge: 0xB6BFCF,
    bad_bg: 0xFDE7E7,
    bad: 0xB42318,
    warn_bg: 0xFDF0D5,
    warn: 0x8A5406,
    ok: 0x067647,
    divider: 0xDDE1E9,
};

pub const DARK: Palette = Palette {
    bg: 0x15171C,
    card: 0x1D2027,
    card_border: 0x2B303A,
    strip: 0x2E323D,
    strip_hover: 0x3A3F4C,
    text: 0xE7E9EE,
    text_muted: 0x9FA6B4,
    text_faint: 0x7F8795,
    accent: 0x5B92F7,
    accent_hover: 0x7AA7F9,
    accent_fill: 0x3970E6,
    accent_on: 0xFFFFFF,
    accent_soft: 0x1B2A47,
    field: 0x23262E,
    field_border: 0x353A45,
    keycap: 0x292D36,
    keycap_edge: 0x131519,
    bad_bg: 0x3A1C1C,
    bad: 0xFF9A92,
    warn_bg: 0x372911,
    warn: 0xF2C46B,
    ok: 0x5CCB92,
    divider: 0x333944,
};

fn channel(c: u32) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(rgb: u32) -> f64 {
    let r = channel((rgb >> 16) & 0xFF);
    let g = channel((rgb >> 8) & 0xFF);
    let b = channel(rgb & 0xFF);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG 2.x relative contrast. Order-independent.
pub fn contrast(fg: u32, bg: u32) -> f64 {
    let (a, b) = (luminance(fg), luminance(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    /// Reads `GetSysColor` on Windows, exactly as the window did before this
    /// design existed. There is no `Palette` for it, and that is the point:
    /// the branch cannot accidentally acquire a literal.
    HighContrast,
}

impl Theme {
    pub fn palette(self) -> Option<&'static Palette> {
        match self {
            Theme::Light => Some(&LIGHT),
            Theme::Dark => Some(&DARK),
            Theme::HighContrast => None,
        }
    }
}

/// What the OS reports. `apps_use_light_theme` is `None` when the registry
/// value is absent, which is the state of a fresh profile and means light.
#[derive(Clone, Copy, Debug)]
pub struct ThemeInputs {
    pub high_contrast: bool,
    pub apps_use_light_theme: Option<u32>,
}

pub fn resolve(i: ThemeInputs) -> Theme {
    // High contrast outranks the registry unconditionally. A user in high
    // contrast has asked the OS for specific colours; a palette of ours would
    // override exactly the thing they turned on.
    if i.high_contrast {
        return Theme::HighContrast;
    }
    match i.apps_use_light_theme {
        Some(0) => Theme::Dark,
        _ => Theme::Light,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// Tier 1: DWM composites Mica behind an unpainted client area.
    Mica,
    /// Tier 2: one uniform alpha over the whole window.
    Alpha(u8),
    /// Tier 3: no transparency at all.
    Opaque,
}

/// Windows 11 22H2. `DWMWA_SYSTEMBACKDROP_TYPE` is ignored below this.
pub const MICA_MIN_BUILD: u32 = 22621;

/// The alpha for tier 2.
///
/// **250 (98 %), and the two attempts before it are the argument.** 245 was
/// picked without seeing it, back when tier 2 was a fallback nobody expected
/// to reach. After Gate 01 demoted Mica it became the only transparency the
/// window has, so it went to 232 (91 %) to be visible — and that was tried on
/// a real desktop and rejected: *"trong suốt quá đà, và không có làm mờ nên
/// rất khó nhìn do xuyên qua."*
///
/// The finding under that: **a uniform alpha is not glass.** Mica and Acrylic
/// blur what is behind them, which is what stops the window underneath from
/// competing with the text on top. `SetLayeredWindowAttributes` cannot blur —
/// it only dims — so every step of visible transparency is a step of legible
/// clutter, with nothing gained. Tier 2 is therefore a hint of depth at the
/// window's edges and nothing more, until a backdrop that actually blurs is
/// reachable from a GDI client.
pub const TIER2_ALPHA: u8 = 250;

#[derive(Clone, Copy, Debug)]
pub struct BackdropInputs {
    pub build: u32,
    pub high_contrast: bool,
    pub remote_session: bool,
    /// `Themes\Personalize\EnableTransparency`. False means the user turned
    /// transparency off in Settings.
    pub transparency_enabled: bool,
    /// Cleared by the caller once tier 1 has been shown not to work on this
    /// machine, so the decision has one home rather than two.
    pub mica_supported: bool,
}

/// Why this machine may not be transparent at all.
///
/// **One variant per refusal in `transparency_block`, and the point of the
/// enum is that the caller can SAY which.** Design §3.3 forces the System
/// page's transparency slider off under any of the three and requires the
/// reason in the control's own slot on the same line -- "never a tooltip,
/// because a disabled Win32 control receives no mouse messages", so a
/// disabled control with no words beside it is a control that explains
/// itself nowhere. A `bool` would have been enough for `backdrop` and is not
/// enough for that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransparencyBlock {
    HighContrast,
    RemoteSession,
    /// `Themes\Personalize\EnableTransparency` is 0 -- the user turned
    /// transparency off in Settings.
    SystemSetting,
}

impl TransparencyBlock {
    /// What the slider's own slot reads instead of a percentage.
    ///
    /// **Each one names the cause, not the effect.** "Off" alone would send
    /// the reader looking for the switch that turned it off; every one of
    /// these three is somewhere else entirely, and only the third is
    /// somewhere the user can go and change.
    ///
    /// ASCII, like every display string in this window: a `serve --log`
    /// em-dash came back as `?"` once, and a text face draws a glyph it does
    /// not carry as a box.
    pub fn reason(self) -> &'static str {
        match self {
            TransparencyBlock::HighContrast => "Off in high contrast",
            TransparencyBlock::RemoteSession => "Off in a remote session",
            TransparencyBlock::SystemSetting => "Off in Windows settings",
        }
    }
}

/// May this machine be transparent at all, and if not, why not?
///
/// **The ONE copy of the three refusals.** `backdrop` below is one reader and
/// `settings::transparency` is the other; before this function existed the
/// tier decision owned them as an inline `||` and the System page would have
/// been the second place they were spelled -- which is how a slider comes to
/// be live on a machine whose window is already opaque, or greyed on one
/// where it is not.
///
/// Each refusal is correctness rather than taste. High contrast: a
/// translucent ground defeats the guaranteed contrast the mode exists to
/// provide. Remote session: every frame becomes a blend the wire has to
/// carry. Transparency off: the user already answered this question in
/// Settings.
///
/// **The order is the precedence**, and it is only observable through
/// `reason()` -- `backdrop` cannot tell the three apart. High contrast leads
/// because it is the one the OS is enforcing rather than merely preferring;
/// the registry setting is last because it is the only one the user can
/// change from where they are standing, so it is the least surprising thing
/// to be told when it is not the whole story.
pub fn transparency_block(i: BackdropInputs) -> Option<TransparencyBlock> {
    if i.high_contrast {
        return Some(TransparencyBlock::HighContrast);
    }
    if i.remote_session {
        return Some(TransparencyBlock::RemoteSession);
    }
    if !i.transparency_enabled {
        return Some(TransparencyBlock::SystemSetting);
    }
    None
}

pub fn backdrop(i: BackdropInputs) -> Backdrop {
    if transparency_block(i).is_some() {
        return Backdrop::Opaque;
    }
    if i.mica_supported && i.build >= MICA_MIN_BUILD {
        return Backdrop::Mica;
    }
    Backdrop::Alpha(TIER2_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every foreground/background pair the window actually puts on screen,
    /// with the floor it must clear: 4.5 for text, 1.2 for a border whose job
    /// is only to be visible as an edge.
    fn pairs(p: &Palette) -> Vec<(&'static str, u32, u32, f64)> {
        vec![
            ("body text on card", p.text, p.card, 4.5),
            ("muted text on card", p.text_muted, p.card, 4.5),
            ("faint text on card", p.text_faint, p.card, 4.5),
            ("muted text on window bg", p.text_muted, p.bg, 4.5),
            ("accent text on card", p.accent, p.card, 4.5),
            ("white on accent fill", p.accent_on, p.accent_fill, 4.5),
            ("accent text on soft fill", p.accent, p.accent_soft, 4.5),
            ("bad pill", p.bad, p.bad_bg, 4.5),
            ("warn pill", p.warn, p.warn_bg, 4.5),
            // The four `IDC_NOTES` severity-dot colours against the card
            // they sit on (beckon-windows `paint::draw_notes`, Task 12).
            // `Mark::Unknown`'s dot is `text_faint`, already covered by
            // "faint text on card" above -- not repeated here.
            ("ok note dot", p.ok, p.card, 4.5),
            ("warn note dot", p.warn, p.card, 4.5),
            ("bad note dot", p.bad, p.card, 4.5),
            ("keycap letter", p.text, p.keycap, 4.5),
            ("card border on bg", p.card_border, p.bg, 1.2),
            ("field border on card", p.field_border, p.card, 1.2),
            ("divider on card", p.divider, p.card, 1.2),
            // -- Added by the whole-branch review, 2026-08-13: nine pairs
            // that were drawn on screen but unchecked. Sites are all in
            // `beckon-windows/src/settings_window/{chrome,paint,mod}.rs`;
            // see that crate's source for the exact `col`/`theme_col` call
            // at each one.
            //
            // `chrome.rs`'s "beckon" wordmark, in the accent colour, on the
            // title bar's own `bg` fill.
            ("accent title on chrome bg", p.accent, p.bg, 4.5),
            // Two sites, same token pair: `chrome.rs`'s minimize/close
            // caption ink at rest, on the title bar's `bg` fill; and
            // `paint.rs`'s `draw_keycaps`, the Shortcut column's chord
            // cells -- every key EXCEPT the last (main) one takes `bg` as
            // its face, all sharing the row's one `text` ink.
            ("body text on window bg", p.text, p.bg, 4.5),
            // Three sites, same token pair, all LIVE (non-disabled) text:
            // `mod.rs`'s `WM_CTLCOLOREDIT` arm (the App combo and the
            // filter box while enabled), `paint.rs`'s `draw_combo_item`'s
            // plain (non-disabled, non-picked) branch, and `paint.rs`'s
            // `BtnTier::Secondary` ink (`Add`, `Remove`, `Reload`, `Open
            // config file`, `Close`, `Keep mine`).
            ("body text on field", p.text, p.field, 4.5),
            // Two sites, same token pair: `paint.rs`'s `draw_combo_item`'s
            // picked branch (a selected dropdown item) and
            // `list_custom_draw`'s selected-row fallback (a selected
            // Shortcut cell whose caps did not fit and fell back to plain
            // text).
            ("body text on accent_soft fill", p.text, p.accent_soft, 4.5),
            // `paint.rs`'s `BtnTier::Outline` (`Record` idle, `Revert`),
            // hovered or pressed-but-not-filled: ink is `accent_hover`.
            //
            // **Ground is `bg`, not `card`.** The pre-fix version of this
            // finding paired this ink against `card`, matching both the
            // shipped design-intent comment on `colours` (now corrected,
            // see S1) and the assumption that a resting/hot `Outline`
            // button lets the surrounding card show through. It does not:
            // `button` (the sole caller) fills the WHOLE control rect with
            // `p.bg` before this tier's fill-less state is painted, so `bg`
            // is the surface this ink is literally drawn on. Both clear the
            // floor regardless (measured: card 6.66/6.77, bg 6.05/7.45);
            // `bg` is the one actually on screen.
            (
                "outline-tier hover/hot ink on bg",
                p.accent_hover,
                p.bg,
                4.5,
            ),
            // `paint.rs`'s `BtnTier::Outline`, PRESSED: fill becomes
            // `accent_soft` -- an actual `RoundRect` fill this time, not
            // the bare `bg` above -- ink stays `accent_hover`.
            (
                "outline-tier pressed ink on accent_soft",
                p.accent_hover,
                p.accent_soft,
                4.5,
            ),
            // `paint.rs`'s `draw_keycaps`, resting (non-armed,
            // non-disabled) toggle chip: the chip's own border colour
            // against its own `keycap` face. A border-visibility floor,
            // like the other borders above, not a text floor.
            ("keycap edge on keycap face", p.keycap_edge, p.keycap, 1.2),
            // -- The tab strip, 2026-08-14. Four `BS_AUTORADIOBUTTON |
            // BS_PUSHLIKE` pills in a painted trough between the
            // client-drawn title bar and the first card.
            //
            // **These rows land before either site exists**, which is the
            // order the design asks for: the palette is checked first and
            // the drawing is written against tokens already known to clear
            // their floors. The sites will be `beckon-windows`'s
            // `paint::tab_pill` and the trough fill in `WM_PAINT`.
            //
            // **Four of these ten measurements clear by under 0.04.** The
            // tightest is the DARK half of "strip_hover on strip" at
            // 1.2168, +0.017; the other three are LIGHT -- 4.5222 (+0.022),
            // 1.2346 (+0.035), 1.2220 (+0.022). They are correct and they
            // are fragile: a future move of `text_muted`, `bg` or either
            // strip token can break one, and these rows exist so that break
            // is a test failure rather than a screenshot.
            //
            // (The spec's table bolds three cells as the near-floor ones,
            // all LIGHT. Recomputed here, the DARK half of `strip_hover` is
            // narrower than any of them and is the one to watch.)
            //
            // An inactive pill's ink changes with the state, and the swap
            // is load-bearing: `text_muted` on `strip_hover` measures
            // 3.700 / 4.304, so a hover that moved only the ground would
            // drop the label under 4.5. See `Palette::strip_hover`.
            ("inactive pill label on strip", p.text_muted, p.strip, 4.5),
            (
                "hovered pill label on strip_hover",
                p.text,
                p.strip_hover,
                4.5,
            ),
            ("strip on window bg", p.strip, p.bg, 1.2),
            ("strip_hover on strip", p.strip_hover, p.strip, 1.2),
            // The active pill's FILL, against the trough it sits in. Its
            // INK already has a row -- "white on accent fill" above -- and
            // that row is the reason the fill must be `accent_fill` and
            // never `accent`: `accent_on` on `DARK.accent` measures 3.044,
            // and no row in this table covers that pair, so the failure
            // would ship unseen.
            ("active pill fill on strip", p.accent_fill, p.strip, 1.2),
            // The Shortcuts pill's warn dot -- a drawn `Ellipse`, never the
            // character U+25CF, because this window carries a text face and a
            // missing glyph draws as a box. It says the config file moved on
            // disk while the user is behind one of the other three doors.
            //
            // **3.0, not the 4.5 the four `IDC_NOTES` dots above carry, and
            // that is a floor chosen rather than a floor dodged.** Those four
            // sit in a line of prose on a card and were given the text floor
            // they already clear. This one is a standalone non-text indicator,
            // which is WCAG 2.1 SC 1.4.11's 3.0. It matters because the ink is
            // shared: `warn` measures 4.609 / 7.857 at rest on `strip` --
            // comfortably past 4.5 -- and 3.772 / 6.457 on `strip_hover`, so
            // holding the hover state to 4.5 would mean moving `LIGHT.warn`,
            // which is also the warn pill's and the warn note dot's ink. The
            // hover ground is the binding one in both themes because
            // `strip_hover` is the darker of the two in Light and the lighter
            // in Dark, i.e. it moves toward the ink either way.
            //
            // There is deliberately no `warn on accent_fill` row: the dot is
            // never drawn on a lit pill. `warn_dot_shown` is the complement of
            // `banner_shown` within `external_change`, so the door whose pill
            // would carry the dot is exactly the door showing the banner
            // instead -- pinned by `settings::the_dot_is_never_on_the_door_
            // that_is_open`. That pair measures 1.212 in Light, so the
            // structure is what keeps it off screen, not this table.
            ("warn dot on strip (SC 1.4.11)", p.warn, p.strip, 3.0),
            (
                "warn dot on strip_hover (SC 1.4.11)",
                p.warn,
                p.strip_hover,
                3.0,
            ),
            // The pill's own focus ring, drawn in the `FOCUS_SLACK` margin
            // between the control's edge and the pill anyone sees. Non-text
            // again, so 3.0 again.
            //
            // **ONE row, for all three states -- CORRECTED 2026-08-14.** This
            // was two rows, and it said that a LIT pill's ring is `accent_on`
            // rather than `accent` because `accent` on `accent_fill` is 1.00:1
            // in Light. Both halves of that are true of `paint::button`'s
            // `Accent` tier, which insets its ring INTO a control already
            // filled with `accent_fill`; neither is true here. `tab_pill`
            // fills the whole control rect with `strip`, insets the pill by
            // `FOCUS_SLACK`, and stops the ring's stroke a device pixel short
            // of the pill at every DPI -- so the ring's ground is the TROUGH
            // whether the pill is lit, hovered or at rest, and the `accent_on`
            // that shipped measured **1.360** against it in Light. An
            // invisible ring, uncovered by any row here, which is how it
            // shipped green. See
            // `the_lit_pills_ring_is_measured_against_the_trough`.
            //
            // The `strip_hover` row went with the swap for the same reason:
            // the hover fill covers the PILL and never the margin, so it is
            // not a ground this ink is ever on. (The warn dot's two rows above
            // are the other way round -- the dot is drawn INSIDE the pill, so
            // it does take the pill's own fill, hover included.)
            ("pill focus ring on strip", p.accent, p.strip, 3.0),
            // -- Exemptions. WCAG 2.1 SC 1.4.3 itself excepts "text ... that
            // is part of an inactive user interface component" from the 4.5
            // floor. These two rows are the disabled-state ink the review's
            // Must-Fix list found under 4.5; every site was traced by hand
            // and is a genuinely disabled control, not live text -- see
            // `final-fix-report.md` for the per-site reasoning. The floor
            // is lowered, not removed: a future edit that drives either
            // pair toward 1:1 (effectively invisible, not merely exempt)
            // still fails.
            //
            // `text_faint` on `bg`: the one LIVE site on this exact token
            // pair was `chrome.rs`'s title-bar version string, fixed (M1)
            // to `text_muted` -- now covered by "muted text on window bg"
            // above instead. The remaining site is `paint.rs`'s
            // `draw_keycaps`, a disabled toggle chip's ink on its own
            // disabled `bg` face.
            (
                "disabled chip ink on bg (exempt, SC 1.4.3)",
                p.text_faint,
                p.bg,
                3.0,
            ),
            // `text_faint` on `field`: `mod.rs`'s `WM_CTLCOLORSTATIC` arm
            // for a disabled App combo / filter box, `paint.rs`'s
            // `draw_combo_item` disabled branch (a disabled dropdown item),
            // and `paint.rs`'s `colours()` disabled branch (every push
            // button's disabled ink, all four tiers) -- three sites, all
            // disabled controls.
            (
                "disabled field/combo/button ink on field (exempt, SC 1.4.3)",
                p.text_faint,
                p.field,
                3.0,
            ),
        ]
    }

    #[test]
    fn every_pair_clears_its_floor_in_both_themes() {
        let mut failures = Vec::new();
        for (name, p) in [("light", &LIGHT), ("dark", &DARK)] {
            for (label, fg, bg, floor) in pairs(p) {
                let r = contrast(fg, bg);
                if r < floor {
                    failures.push(format!(
                        "{name}: {label} = {r:.2} (need {floor}) \
                         #{fg:06X} on #{bg:06X}"
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "contrast failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn contrast_matches_known_values() {
        // White on black is the WCAG maximum, 21:1.
        assert!((contrast(0xFFFFFF, 0x000000) - 21.0).abs() < 0.001);
        // A colour against itself is 1:1.
        assert!((contrast(0x2563EB, 0x2563EB) - 1.0).abs() < 0.001);
        // Order does not matter.
        assert!((contrast(0x2563EB, 0xFFFFFF) - contrast(0xFFFFFF, 0x2563EB)).abs() < 1e-9);
    }

    /// The two tokens exist because one hex cannot do both jobs. If a future
    /// edit makes them equal in DARK, the reason for the split has been lost.
    #[test]
    fn accent_and_accent_fill_are_distinct_in_dark() {
        assert_ne!(DARK.accent, DARK.accent_fill);
    }

    /// `Palette::strip`'s "the dark trough is lighter than the card" claim,
    /// as arithmetic rather than as prose.
    ///
    /// A reader who doubts it reaches for the far end of the obvious
    /// direction -- black under a dark theme, white under a light one -- and
    /// those two are the best case, not a middling one. Neither reaches the
    /// 1.2 that `pairs()` holds `strip` to against `bg`, so the trough has
    /// nowhere to go but away from the ground. `pairs()` cannot cover this:
    /// black and white are not palette tokens, and the point is about
    /// colours the palette deliberately does NOT contain.
    #[test]
    fn the_trough_can_only_move_away_from_the_ground_one_way() {
        assert!(contrast(0x000000, DARK.bg) < 1.2);
        assert!(contrast(0xFFFFFF, LIGHT.bg) < 1.2);
        // The two figures the doc comment quotes, so a `bg` edit that
        // invalidates the prose fails here rather than being believed.
        assert!((contrast(0x000000, DARK.bg) - 1.171).abs() < 0.001);
        assert!((contrast(0xFFFFFF, LIGHT.bg) - 1.101).abs() < 0.001);
        // And the trough did move that way: lighter than the card in DARK,
        // darker than the card in LIGHT.
        assert!(luminance(DARK.strip) > luminance(DARK.card));
        assert!(luminance(LIGHT.strip) < luminance(LIGHT.card));
    }

    /// Why `paint::tab_pill`'s focus ring is `accent` in every state, lit
    /// included, and why the `accent_on` it shipped with read as sound.
    ///
    /// The ring is drawn in the `FOCUS_SLACK` margin, and that margin is
    /// trough: the pill's own fill starts a device pixel further in, at every
    /// DPI. So `strip` is the ring's ground in all three states -- and white
    /// on the trough is invisible, while white on the FILL, the surface the
    /// shipped comment named, is comfortably clear. That is the whole trap,
    /// and both halves are pinned here so the swap cannot come back as a
    /// plausible sentence: the number that makes it wrong and the number that
    /// makes it sound like it might be right.
    ///
    /// `pairs()` cannot carry this. It asserts floors for pairs that ARE
    /// drawn; the defect was a pair that was drawn and had no row, and the
    /// only way to state that is to name the pair and its measurement here.
    #[test]
    fn the_lit_pills_ring_is_measured_against_the_trough() {
        // The ink that shipped, on the ground it was actually on. Under the
        // 3.0 non-text floor, and by a distance -- this is not a near miss.
        assert!(contrast(LIGHT.accent_on, LIGHT.strip) < 3.0);
        assert!((contrast(LIGHT.accent_on, LIGHT.strip) - 1.360).abs() < 0.001);
        // The same ink on the ground the comment CLAIMED. Sound, and not
        // where the ring is -- which is why the reasoning survived review.
        assert!(contrast(LIGHT.accent_on, LIGHT.accent_fill) >= 4.5);
        assert!(contrast(DARK.accent_on, DARK.accent_fill) >= 4.5);
        // And why a screenshot would not have found it either: in Dark the
        // wrong ink is the most legible thing on the strip.
        assert!(contrast(DARK.accent_on, DARK.strip) > 10.0);
        // The ink that ships instead, on the ground it is on. `pairs()` holds
        // this too; it is repeated because this test is the one that says why
        // that is a single row rather than one per state.
        assert!(contrast(LIGHT.accent, LIGHT.strip) >= 3.0);
        assert!(contrast(DARK.accent, DARK.strip) >= 3.0);
    }

    fn ti(hc: bool, light: Option<u32>) -> ThemeInputs {
        ThemeInputs {
            high_contrast: hc,
            apps_use_light_theme: light,
        }
    }

    #[test]
    fn registry_zero_is_dark_and_anything_else_is_light() {
        assert_eq!(resolve(ti(false, Some(0))), Theme::Dark);
        assert_eq!(resolve(ti(false, Some(1))), Theme::Light);
        // Absent on a fresh profile.
        assert_eq!(resolve(ti(false, None)), Theme::Light);
    }

    #[test]
    fn high_contrast_outranks_the_registry_both_ways() {
        assert_eq!(resolve(ti(true, Some(0))), Theme::HighContrast);
        assert_eq!(resolve(ti(true, Some(1))), Theme::HighContrast);
        assert_eq!(resolve(ti(true, None)), Theme::HighContrast);
    }

    #[test]
    fn high_contrast_has_no_palette_so_no_literal_can_reach_it() {
        assert!(Theme::HighContrast.palette().is_none());
        assert!(Theme::Light.palette().is_some());
        assert!(Theme::Dark.palette().is_some());
    }

    fn bi(build: u32) -> BackdropInputs {
        BackdropInputs {
            build,
            high_contrast: false,
            remote_session: false,
            transparency_enabled: true,
            mica_supported: true,
        }
    }

    #[test]
    fn mica_needs_22h2() {
        assert_eq!(backdrop(bi(MICA_MIN_BUILD)), Backdrop::Mica);
        assert_eq!(
            backdrop(bi(MICA_MIN_BUILD - 1)),
            Backdrop::Alpha(TIER2_ALPHA)
        );
        // Windows 10 21H2.
        assert_eq!(backdrop(bi(19044)), Backdrop::Alpha(TIER2_ALPHA));
    }

    #[test]
    fn a_hardware_failure_demotes_to_tier_two_without_touching_the_build() {
        let i = BackdropInputs {
            mica_supported: false,
            ..bi(26200)
        };
        assert_eq!(backdrop(i), Backdrop::Alpha(TIER2_ALPHA));
    }

    #[test]
    fn three_conditions_force_opaque_even_on_a_capable_build() {
        let capable = bi(26200);
        assert_eq!(
            backdrop(BackdropInputs {
                high_contrast: true,
                ..capable
            }),
            Backdrop::Opaque
        );
        assert_eq!(
            backdrop(BackdropInputs {
                remote_session: true,
                ..capable
            }),
            Backdrop::Opaque
        );
        assert_eq!(
            backdrop(BackdropInputs {
                transparency_enabled: false,
                ..capable
            }),
            Backdrop::Opaque
        );
    }

    /// Opaque wins over Mica, not the other way round.
    ///
    /// **CORRECTED 2026-08-13.** This used to claim the test was needed
    /// because "an `if` reordered during a refactor would still pass every
    /// test above". That is false, and a review disproved it by hand-trace:
    /// swapping the two `if` blocks in `backdrop` makes
    /// `three_conditions_force_opaque_even_on_a_capable_build` fail on all
    /// three of its sub-assertions, because its `bi(26200)` already sets
    /// `mica_supported: true`. The test above catches the reordering on its
    /// own.
    ///
    /// This test stays as a second net that says in its NAME what the
    /// ordering is, and it does fail independently under the same reversal.
    /// It is redundant coverage, deliberately kept — not the only coverage,
    /// as the old comment claimed.
    #[test]
    fn refusals_are_checked_before_capability() {
        let i = BackdropInputs {
            high_contrast: true,
            transparency_enabled: false,
            ..bi(26200)
        };
        assert_eq!(backdrop(i), Backdrop::Opaque);
    }

    /// `backdrop` is opaque EXACTLY when `transparency_block` refuses.
    ///
    /// The two are one predicate with two readers since the System page's
    /// slider arrived, and this is the assertion that keeps them one: it walks
    /// the eight combinations of the three refusal inputs against a build that
    /// would otherwise be capable, so a `backdrop` that grew a fourth
    /// condition of its own -- or a `transparency_block` that lost one --
    /// fails here rather than shipping a live slider on an opaque window.
    #[test]
    fn the_slider_is_blocked_exactly_when_the_window_is_opaque() {
        for hc in [false, true] {
            for remote in [false, true] {
                for on in [false, true] {
                    let i = BackdropInputs {
                        high_contrast: hc,
                        remote_session: remote,
                        transparency_enabled: on,
                        ..bi(26200)
                    };
                    assert_eq!(
                        transparency_block(i).is_some(),
                        backdrop(i) == Backdrop::Opaque,
                        "hc={hc} remote={remote} enabled={on}"
                    );
                }
            }
        }
    }

    /// The three reasons are distinguishable and ordered.
    ///
    /// `backdrop` cannot see the difference -- all three are `Opaque` -- so
    /// nothing above this line would notice the arms being reordered, and the
    /// order is what the System page's one-line reason is drawn from. A
    /// machine in high contrast over RDP with transparency off says the first
    /// of the three.
    #[test]
    fn the_reason_names_the_first_refusal_that_applies() {
        let all = BackdropInputs {
            high_contrast: true,
            remote_session: true,
            transparency_enabled: false,
            ..bi(26200)
        };
        assert_eq!(
            transparency_block(all),
            Some(TransparencyBlock::HighContrast)
        );
        assert_eq!(
            transparency_block(BackdropInputs {
                high_contrast: false,
                ..all
            }),
            Some(TransparencyBlock::RemoteSession)
        );
        assert_eq!(
            transparency_block(BackdropInputs {
                high_contrast: false,
                remote_session: false,
                ..all
            }),
            Some(TransparencyBlock::SystemSetting)
        );
        // Three distinct sentences, so the enum earns its variants.
        let words: Vec<&str> = [
            TransparencyBlock::HighContrast,
            TransparencyBlock::RemoteSession,
            TransparencyBlock::SystemSetting,
        ]
        .iter()
        .map(|b| b.reason())
        .collect();
        assert_eq!(words.len(), 3);
        for w in &words {
            assert!(w.is_ascii(), "{w} is not ASCII");
            assert_eq!(words.iter().filter(|o| *o == w).count(), 1, "{w} repeats");
        }
    }

    /// COLORREF is 0x00BBGGRR. The window converts at its boundary, but the
    /// swap is easy to write the wrong way round and produces a plausible
    /// wrong colour rather than an obvious one -- beckon's blue #2563EB comes
    /// back as a muddy teal.
    #[test]
    fn the_bgr_swap_is_documented_by_a_case() {
        fn to_colorref(rgb: u32) -> u32 {
            ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
        }
        assert_eq!(to_colorref(0x2563EB), 0x00EB6325);
        assert_eq!(to_colorref(0xFFFFFF), 0x00FFFFFF);
        assert_eq!(to_colorref(0x000000), 0x00000000);
        // Not a palindrome, so a no-op implementation fails.
        assert_ne!(to_colorref(LIGHT.accent), LIGHT.accent);
    }
}
