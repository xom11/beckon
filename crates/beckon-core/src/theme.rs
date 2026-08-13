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
    divider: 0xE8EBF1,
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
    divider: 0x272B33,
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
}
