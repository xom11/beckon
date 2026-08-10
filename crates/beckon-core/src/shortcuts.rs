//! Shortcut-config model shared by `check` and `serve`: canonical key
//! table (name → Carbon virtual keycode / Win32 VK) and combo parsing.

use std::sync::OnceLock;

/// One canonical key: the name used in shortcut files, plus its Carbon
/// virtual keycode (macOS, HIToolbox Events.h `kVK_*`) and Win32 virtual-key
/// code. Every canonical key MUST have a code on BOTH OSes so `check`
/// works without knowing which target a file belongs to (hence f20 is the
/// ceiling: macOS has no F21+).
#[derive(Debug, PartialEq, Eq)]
pub struct KeyDef {
    pub name: String,
    pub mac: u16,
    pub win: u32,
}

fn build_table() -> Vec<KeyDef> {
    let mut v = Vec::with_capacity(81);
    // Letters. Carbon codes are layout-position based, not alphabetical.
    const MAC_LETTERS: [(char, u16); 26] = [
        ('a', 0x00),
        ('b', 0x0B),
        ('c', 0x08),
        ('d', 0x02),
        ('e', 0x0E),
        ('f', 0x03),
        ('g', 0x05),
        ('h', 0x04),
        ('i', 0x22),
        ('j', 0x26),
        ('k', 0x28),
        ('l', 0x25),
        ('m', 0x2E),
        ('n', 0x2D),
        ('o', 0x1F),
        ('p', 0x23),
        ('q', 0x0C),
        ('r', 0x0F),
        ('s', 0x01),
        ('t', 0x11),
        ('u', 0x20),
        ('v', 0x09),
        ('w', 0x0D),
        ('x', 0x07),
        ('y', 0x10),
        ('z', 0x06),
    ];
    for (c, mac) in MAC_LETTERS {
        v.push(KeyDef {
            name: c.to_string(),
            mac,
            win: 0x41 + (c as u32 - 'a' as u32),
        });
    }
    // Digits 0..9 (kVK_ANSI_0..9 are not contiguous).
    const MAC_DIGITS: [u16; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
    for d in 0..10u32 {
        v.push(KeyDef {
            name: d.to_string(),
            mac: MAC_DIGITS[d as usize],
            win: 0x30 + d,
        });
    }
    // f1..f20 (VK_F1 = 0x70 contiguous; Carbon scattered).
    const MAC_F: [u16; 20] = [
        0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, // f1..f10
        0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A, 0x40, 0x4F, 0x50, 0x5A, // f11..f20
    ];
    for i in 0..20u32 {
        v.push(KeyDef {
            name: format!("f{}", i + 1),
            mac: MAC_F[i as usize],
            win: 0x70 + i,
        });
    }
    // Named specials: (name, kVK_*, VK_*).
    const SPECIALS: [(&str, u16, u32); 25] = [
        ("comma", 0x2B, 0xBC),  // VK_OEM_COMMA
        ("period", 0x2F, 0xBE), // VK_OEM_PERIOD
        ("slash", 0x2C, 0xBF),  // VK_OEM_2
        ("space", 0x31, 0x20),
        ("minus", 0x1B, 0xBD),        // VK_OEM_MINUS
        ("equal", 0x18, 0xBB),        // VK_OEM_PLUS
        ("semicolon", 0x29, 0xBA),    // VK_OEM_1
        ("quote", 0x27, 0xDE),        // VK_OEM_7
        ("bracketleft", 0x21, 0xDB),  // VK_OEM_4
        ("bracketright", 0x1E, 0xDD), // VK_OEM_6
        ("backslash", 0x2A, 0xDC),    // VK_OEM_5
        ("grave", 0x32, 0xC0),        // VK_OEM_3
        ("tab", 0x30, 0x09),
        ("return", 0x24, 0x0D),
        ("escape", 0x35, 0x1B),
        ("backspace", 0x33, 0x08), // kVK_Delete is the backspace key
        ("delete", 0x75, 0x2E),    // kVK_ForwardDelete / VK_DELETE
        ("home", 0x73, 0x24),
        ("end", 0x77, 0x23),
        ("pageup", 0x74, 0x21),   // VK_PRIOR
        ("pagedown", 0x79, 0x22), // VK_NEXT
        ("left", 0x7B, 0x25),
        ("right", 0x7C, 0x27),
        ("up", 0x7E, 0x26),
        ("down", 0x7D, 0x28),
    ];
    for (name, mac, win) in SPECIALS {
        v.push(KeyDef {
            name: name.to_string(),
            mac,
            win,
        });
    }
    v
}

fn all_keys() -> &'static [KeyDef] {
    static KEYS: OnceLock<Vec<KeyDef>> = OnceLock::new();
    KEYS.get_or_init(build_table)
}

pub fn lookup_key(name: &str) -> Option<&'static KeyDef> {
    all_keys().iter().find(|k| k.name == name)
}

/// A parsed key combo. Modifier order in the input is free; `canonical()`
/// always prints ctrl → super → alt → shift → key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub super_: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: &'static KeyDef,
}

impl Combo {
    pub fn parse(s: &str) -> Result<Combo, String> {
        let tokens: Vec<&str> = s.split('+').collect();
        if s.is_empty() || tokens.iter().any(|t| t.is_empty()) {
            return Err(format!("empty token in combo `{s}` (stray `+`?)"));
        }
        let (key_name, mods) = tokens.split_last().expect("non-empty by check above");
        let (mut ctrl, mut super_, mut alt, mut shift) = (false, false, false, false);
        for m in mods {
            let slot = match *m {
                "ctrl" => &mut ctrl,
                "super" => &mut super_,
                "alt" => &mut alt,
                "shift" => &mut shift,
                other => {
                    return Err(format!(
                        "expected a modifier, got `{other}` in `{s}` (only the last token may be the main key)"
                    ))
                }
            };
            if *slot {
                return Err(format!("duplicate modifier `{m}` in `{s}`"));
            }
            *slot = true;
        }
        if key_name.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(format!(
                "uppercase key `{key_name}` in `{s}` — write it lowercase and add `shift` explicitly"
            ));
        }
        let key = lookup_key(key_name).ok_or_else(|| {
            let hint = if matches!(*key_name, "ctrl" | "super" | "alt" | "shift") {
                " (did you forget the main key?)"
            } else {
                ""
            };
            format!("unknown key `{key_name}` in `{s}`{hint}")
        })?;
        Ok(Combo {
            ctrl,
            super_,
            alt,
            shift,
            key,
        })
    }

    pub fn canonical(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.super_ {
            parts.push("super");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        parts.push(&self.key.name);
        parts.join("+")
    }
}

/// One line of a shortcuts file: a combo bound to exactly one app name.
#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub combo: Combo,
    pub app: String,
}

/// Parse a flat shortcuts TOML file: every top-level key is a combo, every
/// value is one app-name string. First error wins. Iteration order follows
/// `toml::Table` (BTreeMap, sorted by key) — registration order is
/// irrelevant to hotkey behavior.
pub fn parse_shortcuts(text: &str) -> Result<Vec<Shortcut>, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(table.len());
    for (raw_key, value) in &table {
        let combo = Combo::parse(raw_key)?;
        let canon = combo.canonical();
        if let Some(prev) = seen.get(&canon) {
            return Err(format!(
                "`{raw_key}` duplicates `{prev}` (both normalize to `{canon}`)"
            ));
        }
        seen.insert(canon, raw_key.clone());
        let app = match value {
            toml::Value::String(s) if !s.trim().is_empty() => s.clone(),
            toml::Value::String(_) => return Err(format!("empty app name for `{raw_key}`")),
            toml::Value::Array(_) => {
                return Err(format!(
                    "value for `{raw_key}` is an array — candidate lists are not supported, \
                     write exactly one app name"
                ))
            }
            other => {
                return Err(format!(
                    "value for `{raw_key}` must be a string (one app name), got {}",
                    other.type_str()
                ))
            }
        };
        out.push(Shortcut { combo, app });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{all_keys, lookup_key, parse_shortcuts, Combo};

    #[test]
    fn key_table_covers_spec_names() {
        for name in [
            "a",
            "z",
            "0",
            "9",
            "f1",
            "f20",
            "comma",
            "period",
            "slash",
            "space",
            "minus",
            "equal",
            "semicolon",
            "quote",
            "bracketleft",
            "bracketright",
            "backslash",
            "grave",
            "tab",
            "return",
            "escape",
            "backspace",
            "delete",
            "home",
            "end",
            "pageup",
            "pagedown",
            "up",
            "down",
            "left",
            "right",
        ] {
            assert!(lookup_key(name).is_some(), "missing key `{name}`");
        }
        assert!(
            lookup_key("f21").is_none(),
            "f21 has no macOS keycode — must not exist"
        );
        assert!(lookup_key("A").is_none(), "table is lowercase-only");
    }

    #[test]
    fn key_table_has_no_duplicates() {
        let keys = all_keys();
        let mut names: Vec<_> = keys.iter().map(|k| k.name.as_str()).collect();
        let mut macs: Vec<_> = keys.iter().map(|k| k.mac).collect();
        let mut wins: Vec<_> = keys.iter().map(|k| k.win).collect();
        names.sort();
        macs.sort();
        wins.sort();
        let (n0, m0, w0) = (names.len(), macs.len(), wins.len());
        names.dedup();
        macs.dedup();
        wins.dedup();
        assert_eq!(n0, names.len(), "duplicate key name");
        assert_eq!(m0, macs.len(), "duplicate mac keycode (copy-paste typo?)");
        assert_eq!(w0, wins.len(), "duplicate win VK (copy-paste typo?)");
    }

    #[test]
    fn spot_check_known_keycodes() {
        let t = lookup_key("t").unwrap();
        assert_eq!((t.mac, t.win), (0x11, 0x54));
        let comma = lookup_key("comma").unwrap();
        assert_eq!((comma.mac, comma.win), (0x2B, 0xBC));
        let f19 = lookup_key("f19").unwrap();
        assert_eq!((f19.mac, f19.win), (0x50, 0x82));
    }

    #[test]
    fn parse_combo_order_insensitive_and_canonical() {
        let a = Combo::parse("ctrl+super+alt+t").unwrap();
        let b = Combo::parse("alt+ctrl+super+t").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.canonical(), "ctrl+super+alt+t");
        assert_eq!(
            Combo::parse("shift+alt+super+ctrl+comma")
                .unwrap()
                .canonical(),
            "ctrl+super+alt+shift+comma"
        );
    }

    #[test]
    fn parse_combo_bare_key_is_allowed() {
        let c = Combo::parse("f13").unwrap();
        assert!(!c.ctrl && !c.super_ && !c.alt && !c.shift);
        assert_eq!(c.key.name, "f13");
    }

    #[test]
    fn parse_combo_rejects_duplicate_modifier() {
        let e = Combo::parse("ctrl+ctrl+a").unwrap_err();
        assert!(e.contains("duplicate modifier `ctrl`"), "{e}");
    }

    #[test]
    fn parse_combo_rejects_unknown_key() {
        let e = Combo::parse("ctrl+banana").unwrap_err();
        assert!(e.contains("unknown key `banana`"), "{e}");
    }

    #[test]
    fn parse_combo_rejects_modifier_as_main_key_with_hint() {
        let e = Combo::parse("ctrl+shift").unwrap_err();
        assert!(e.contains("unknown key `shift`"), "{e}");
        assert!(e.contains("did you forget the main key"), "{e}");
    }

    #[test]
    fn parse_combo_rejects_non_modifier_before_end() {
        let e = Combo::parse("ctrl+a+b").unwrap_err();
        assert!(e.contains("expected a modifier, got `a`"), "{e}");
    }

    #[test]
    fn parse_combo_rejects_uppercase() {
        let e = Combo::parse("ctrl+A").unwrap_err();
        assert!(e.contains("lowercase"), "{e}");
    }

    #[test]
    fn parse_combo_rejects_empty_tokens() {
        assert!(Combo::parse("ctrl++a").is_err());
        assert!(Combo::parse("").is_err());
        assert!(Combo::parse("ctrl+a+").is_err());
    }

    #[test]
    fn parse_shortcuts_happy_path_real_toml() {
        let text = r##"
# comments are fine — this is real TOML now
"ctrl+super+alt+t" = "kitty"
"ctrl+super+alt+shift+t" = 'Telegram Web'   # single quotes too
"##;
        let s = parse_shortcuts(text).unwrap();
        assert_eq!(s.len(), 2);
        let t = s
            .iter()
            .find(|x| x.combo.canonical() == "ctrl+super+alt+t")
            .unwrap();
        assert_eq!(t.app, "kitty");
        let tg = s
            .iter()
            .find(|x| x.combo.canonical() == "ctrl+super+alt+shift+t")
            .unwrap();
        assert_eq!(tg.app, "Telegram Web");
    }

    #[test]
    fn parse_shortcuts_rejects_duplicate_after_normalization() {
        let text = "\"ctrl+alt+a\" = \"X\"\n\"alt+ctrl+a\" = \"Y\"\n";
        let e = parse_shortcuts(text).unwrap_err();
        assert!(
            e.contains("`ctrl+alt+a`") && e.contains("`alt+ctrl+a`"),
            "{e}"
        );
    }

    #[test]
    fn parse_shortcuts_rejects_array_value() {
        let e = parse_shortcuts("\"ctrl+alt+a\" = [\"A\", \"B\"]\n").unwrap_err();
        assert!(e.contains("candidate lists are not supported"), "{e}");
    }

    #[test]
    fn parse_shortcuts_rejects_empty_app() {
        let e = parse_shortcuts("\"ctrl+alt+a\" = \"  \"\n").unwrap_err();
        assert!(e.contains("empty app name"), "{e}");
    }

    #[test]
    fn parse_shortcuts_rejects_non_string_value() {
        let e = parse_shortcuts("\"ctrl+alt+a\" = 7\n").unwrap_err();
        assert!(e.contains("must be a string"), "{e}");
    }

    #[test]
    fn parse_shortcuts_toml_syntax_error_carries_position() {
        let e = parse_shortcuts("\"ctrl+alt+a\" = \n").unwrap_err();
        assert!(
            e.contains("line 1"),
            "toml error should carry position: {e}"
        );
    }

    #[test]
    fn parse_shortcuts_propagates_combo_errors() {
        let e = parse_shortcuts("\"ctrl+banana\" = \"X\"\n").unwrap_err();
        assert!(e.contains("unknown key `banana`"), "{e}");
    }
}
