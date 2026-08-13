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
    pub keycap_border: u32,
    pub keycap_edge: u32,
    pub bad_bg: u32,
    pub bad: u32,
    pub warn_bg: u32,
    pub warn: u32,
    pub unk_bg: u32,
    pub unk: u32,
    pub ok: u32,
    pub divider: u32,
}

pub const LIGHT: Palette = Palette {
    bg: 0xF2F4F8,
    card: 0xFFFFFF,
    card_border: 0xDCE0E8,
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
    keycap_border: 0xCDD4E1,
    keycap_edge: 0xB6BFCF,
    bad_bg: 0xFDE7E7,
    bad: 0xB42318,
    warn_bg: 0xFDF0D5,
    warn: 0x8A5406,
    unk_bg: 0xEDEFF4,
    unk: 0x5A6270,
    ok: 0x067647,
    divider: 0xDDE1E9,
};

pub const DARK: Palette = Palette {
    bg: 0x15171C,
    card: 0x1D2027,
    card_border: 0x2B303A,
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
    keycap_border: 0x39404B,
    keycap_edge: 0x131519,
    bad_bg: 0x3A1C1C,
    bad: 0xFF9A92,
    warn_bg: 0x372911,
    warn: 0xF2C46B,
    unk_bg: 0x252932,
    unk: 0x9FA6B4,
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

/// The alpha for tier 2. 245/255 is visible against a busy wallpaper and
/// leaves text effectively solid.
pub const TIER2_ALPHA: u8 = 245;

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

pub fn backdrop(i: BackdropInputs) -> Backdrop {
    // Three refusals, each of which is correctness rather than taste.
    // High contrast: a translucent ground defeats the guaranteed contrast the
    // mode exists to provide. Remote session: every frame becomes a blend the
    // wire has to carry. Transparency off: the user already answered this
    // question in Settings.
    if i.high_contrast || i.remote_session || !i.transparency_enabled {
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
            ("unknown pill", p.unk, p.unk_bg, 4.5),
            ("ok note glyph", p.ok, p.card, 4.5),
            ("keycap letter", p.text, p.keycap, 4.5),
            ("card border on bg", p.card_border, p.bg, 1.2),
            ("field border on card", p.field_border, p.card, 1.2),
            ("divider on card", p.divider, p.card, 1.2),
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
