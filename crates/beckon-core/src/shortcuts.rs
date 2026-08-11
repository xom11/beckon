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
                // ASCII hyphens; see the array arm of `parse_config`.
                "uppercase key `{key_name}` in `{s}` -- write it lowercase and add `shift` explicitly"
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

/// The modifiers holding Caps Lock stands for. No main key, and no `shift`.
///
/// Shift is absent from the type rather than rejected by a rule. The hook
/// has to press and release whatever is here, and releasing Shift while the
/// user is physically holding it tells Windows their Shift is up -- so
/// everything they type next arrives lowercase, silently, until they let go
/// and press it again. Making it unrepresentable means no configuration,
/// hand-written or otherwise, can reach that state. `shift` on an individual
/// binding is unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub super_: bool,
    pub alt: bool,
}

impl Default for Chord {
    fn default() -> Self {
        Chord {
            ctrl: true,
            super_: true,
            alt: true,
        }
    }
}

impl Chord {
    pub fn parse(s: &str) -> Result<Chord, String> {
        let (mut ctrl, mut super_, mut alt) = (false, false, false);
        for tok in s.split('+') {
            let slot = match tok {
                "ctrl" => &mut ctrl,
                "super" => &mut super_,
                "alt" => &mut alt,
                "shift" => {
                    return Err(format!(
                        "`shift` is not allowed in `{}` -- beckon has to press and \
                         release what you put here, and releasing Shift while you are \
                         holding it makes everything you type next lowercase. Put \
                         `shift` on the individual shortcut instead",
                        KEYBOARD_CAPS_HOLD
                    ))
                }
                "" => {
                    return Err(format!(
                        "`{KEYBOARD_CAPS_HOLD}` needs at least one modifier \
                         (`ctrl`, `super` or `alt`)"
                    ))
                }
                other => {
                    return Err(format!(
                        "expected a modifier in `{KEYBOARD_CAPS_HOLD}`, got `{other}` \
                         -- only `ctrl`, `super` and `alt` are allowed, and there is no \
                         main key here"
                    ))
                }
            };
            if *slot {
                return Err(format!(
                    "duplicate modifier `{tok}` in `{KEYBOARD_CAPS_HOLD}`"
                ));
            }
            *slot = true;
        }
        if !(ctrl || super_ || alt) {
            return Err(format!(
                "`{KEYBOARD_CAPS_HOLD}` needs at least one modifier \
                 (`ctrl`, `super` or `alt`)"
            ));
        }
        Ok(Chord { ctrl, super_, alt })
    }

    /// Same order `Combo::canonical` prints: ctrl, super, alt.
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
        parts.join("+")
    }

    pub fn is_default(&self) -> bool {
        *self == Chord::default()
    }
}

/// The dotted key name, used in every error message about it.
pub const KEYBOARD_CAPS_HOLD: &str = "keyboard.caps_hold";

/// One line of a shortcuts file: a combo bound to exactly one app name.
#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub combo: Combo,
    pub app: String,
}

/// What a bare Caps Lock tap does when `keyboard.caps` is on. The hook must
/// swallow the physical Caps key to use it as a modifier, so the original
/// behavior only exists if beckon puts it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapsTap {
    /// Toggle Caps Lock, as if nothing had been remapped. The default,
    /// because someone ticking a box should not silently lose a key.
    #[default]
    CapsLock,
    Escape,
    None,
}

impl CapsTap {
    pub fn parse(s: &str) -> Result<CapsTap, String> {
        match s {
            "capslock" => Ok(CapsTap::CapsLock),
            "escape" => Ok(CapsTap::Escape),
            "none" => Ok(CapsTap::None),
            other => Err(format!(
                "unknown `keyboard.caps_tap` value `{other}` \
                 (expected \"capslock\", \"escape\" or \"none\")"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CapsTap::CapsLock => "capslock",
            CapsTap::Escape => "escape",
            CapsTap::None => "none",
        }
    }
}

/// The `keyboard` block. Read only by Windows `serve`, parsed everywhere:
/// one config file is meant to travel between machines, so a Windows-only
/// setting must not fail `beckon check` on macOS or Linux.
///
/// `#[derive(Default)]` calls each field type's own `Default::default()`,
/// so this produces the same `caps_hold` as `Chord::default()`
/// (ctrl+super+alt) regardless of whether `Chord`'s impl is derived or
/// hand-written -- no manual impl is needed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardConfig {
    pub caps: bool,
    pub caps_tap: CapsTap,
    /// What holding Caps Lock stands for. Meaningful only when `caps` is
    /// true; parsed everywhere so one config file travels between machines.
    pub caps_hold: Chord,
}

/// A whole shortcuts file.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub shortcuts: Vec<Shortcut>,
    pub keyboard: KeyboardConfig,
}

/// The one top-level key that is a settings block rather than a combo.
pub const KEYBOARD_KEY: &str = "keyboard";

/// Parse a shortcuts TOML file: every top-level key is a combo bound to one
/// app-name string, except `keyboard`, which is the settings block. First
/// error wins. Iteration order follows `toml::Table` (BTreeMap, sorted by
/// key) — registration order is irrelevant to hotkey behavior.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(table.len());
    let mut keyboard = KeyboardConfig::default();
    for (raw_key, value) in &table {
        if raw_key == KEYBOARD_KEY {
            keyboard = parse_keyboard(value)?;
            continue;
        }
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
                    // ASCII hyphens, not an em-dash: since the settings
                    // window opens read-only on a parse failure, every
                    // string `parse_config` produces is now something a
                    // STATIC in that window may have to draw -- and it
                    // carries a text face, not a symbol one, so a glyph it
                    // lacks draws as a box that reads like a beckon bug.
                    // Same rule `mark_glyph` and `title_base` already state.
                    "value for `{raw_key}` is an array -- candidate lists are not supported, \
                     write exactly one app name"
                ));
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
    Ok(Config {
        shortcuts: out,
        keyboard,
    })
}

fn parse_keyboard(value: &toml::Value) -> Result<KeyboardConfig, String> {
    let t = value.as_table().ok_or_else(|| {
        format!(
            "`keyboard` must be a table of settings, got {}",
            value.type_str()
        )
    })?;
    let mut kb = KeyboardConfig::default();
    for (k, v) in t {
        match k.as_str() {
            "caps" => {
                kb.caps = v.as_bool().ok_or_else(|| {
                    format!(
                        "`keyboard.caps` must be true or false, got {}",
                        v.type_str()
                    )
                })?
            }
            "caps_tap" => {
                let s = v.as_str().ok_or_else(|| {
                    format!("`keyboard.caps_tap` must be a string, got {}", v.type_str())
                })?;
                kb.caps_tap = CapsTap::parse(s)?;
            }
            "caps_hold" => {
                let s = v.as_str().ok_or_else(|| {
                    format!(
                        "`{KEYBOARD_CAPS_HOLD}` must be a string like \"ctrl+super+alt\", got {}",
                        v.type_str()
                    )
                })?;
                kb.caps_hold = Chord::parse(s)?;
            }
            other => {
                // TOML puts every bare key-value pair written after a
                // `[keyboard]` header INSIDE that table. A shortcut appended
                // to the bottom of such a file is silently nested here and
                // never registers, with no error anywhere. Say so.
                if Combo::parse(other).is_ok() {
                    return Err(format!(
                        "`{other}` is a shortcut but it is nested under `[keyboard]`. \
                         Move it above the `[keyboard]` header, or write the settings \
                         as `keyboard.caps = ...` instead of a `[keyboard]` section."
                    ));
                }
                return Err(format!(
                    "unknown setting `keyboard.{other}` \
                     (expected `caps`, `caps_tap` or `caps_hold`)"
                ));
            }
        }
    }
    Ok(kb)
}

/// Shortcuts only. Kept because `check` and `serve` want exactly this.
pub fn parse_shortcuts(text: &str) -> Result<Vec<Shortcut>, String> {
    parse_config(text).map(|c| c.shortcuts)
}

#[cfg(test)]
mod tests {
    use super::{
        all_keys, lookup_key, parse_config, parse_shortcuts, CapsTap, Chord, Combo, KeyboardConfig,
    };

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

    // ---------- keyboard settings ----------

    #[test]
    fn a_file_without_keyboard_settings_gets_the_defaults() {
        let c = parse_config(r#""ctrl+alt+t" = "Terminal""#).unwrap();
        assert_eq!(c.shortcuts.len(), 1);
        assert!(!c.keyboard.caps, "caps must be off unless asked for");
        assert_eq!(c.keyboard.caps_tap, CapsTap::CapsLock);
        assert_eq!(c.keyboard.caps_hold, Chord::default());
    }

    /// Pinned directly (not just through `parse_config`) so a future change
    /// to how `KeyboardConfig` derives/implements `Default` cannot silently
    /// change what "no `keyboard.caps_hold` in the file" means.
    #[test]
    fn keyboard_config_default_pins_caps_hold_to_the_default_chord() {
        assert_eq!(KeyboardConfig::default().caps_hold, Chord::default());
    }

    #[test]
    fn dotted_keys_set_the_keyboard_block() {
        let c = parse_config(
            "keyboard.caps = true\nkeyboard.caps_tap = \"escape\"\n\"ctrl+alt+t\" = \"Terminal\"\n",
        )
        .unwrap();
        assert!(c.keyboard.caps);
        assert_eq!(c.keyboard.caps_tap, CapsTap::Escape);
        assert_eq!(c.shortcuts.len(), 1, "the shortcut must survive alongside");
    }

    #[test]
    fn a_hand_written_keyboard_header_works_too() {
        let c = parse_config("\"ctrl+alt+t\" = \"Terminal\"\n\n[keyboard]\ncaps = true\n").unwrap();
        assert!(c.keyboard.caps);
        assert_eq!(c.shortcuts.len(), 1);
    }

    /// The footgun the dotted-key spelling exists to avoid: a shortcut
    /// appended below a `[keyboard]` header is silently nested inside it and
    /// never registers.
    #[test]
    fn a_shortcut_nested_under_keyboard_is_a_named_error() {
        let err =
            parse_config("[keyboard]\ncaps = true\n\"ctrl+alt+t\" = \"Terminal\"\n").unwrap_err();
        assert!(
            err.contains("ctrl+alt+t"),
            "must name the offending key: {err}"
        );
        // Not just "unknown setting `keyboard.ctrl+alt+t`" -- that message
        // contains the key and the word `keyboard` too, so asserting on
        // those alone would pass without the guard existing at all. The
        // point of the guard is telling the user what to DO.
        assert!(
            err.contains("Move it above"),
            "must say how to fix it, not just that it is unknown: {err}"
        );
    }

    #[test]
    fn an_unknown_keyboard_setting_is_rejected_not_ignored() {
        let err = parse_config("keyboard.caps_tab = \"escape\"\n").unwrap_err();
        assert!(
            err.contains("caps_tab"),
            "a typo must be named, not ignored: {err}"
        );
    }

    #[test]
    fn caps_tap_takes_exactly_three_values() {
        for v in ["capslock", "escape", "none"] {
            parse_config(&format!("keyboard.caps_tap = \"{v}\"")).unwrap();
        }
        let err = parse_config("keyboard.caps_tap = \"esc\"").unwrap_err();
        assert!(err.contains("esc"), "{err}");
    }

    #[test]
    fn caps_must_be_a_boolean() {
        let err = parse_config("keyboard.caps = \"yes\"").unwrap_err();
        assert!(err.contains("caps"), "{err}");
    }

    #[test]
    fn keyboard_must_be_a_table() {
        let err = parse_config("keyboard = \"on\"").unwrap_err();
        assert!(err.contains("keyboard"), "{err}");
    }

    /// `parse_shortcuts` is the pre-existing API; every current caller and
    /// every current test must keep working through it.
    #[test]
    fn parse_shortcuts_still_ignores_the_keyboard_block() {
        let s = parse_shortcuts("keyboard.caps = true\n\"ctrl+alt+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].app, "Terminal");
    }

    // ---------- Chord / keyboard.caps_hold ----------

    #[test]
    fn a_chord_is_modifiers_with_no_main_key() {
        let c = Chord::parse("ctrl+super+alt").unwrap();
        assert!(c.ctrl && c.super_ && c.alt);
        assert_eq!(c.canonical(), "ctrl+super+alt");
        assert!(c.is_default());
    }

    #[test]
    fn chord_modifier_order_is_free_and_canonical_output_is_not() {
        assert_eq!(Chord::parse("alt+ctrl").unwrap().canonical(), "ctrl+alt");
    }

    #[test]
    fn a_chord_needs_at_least_one_modifier() {
        let e = Chord::parse("").unwrap_err();
        assert!(e.contains("at least one modifier"), "{e}");
    }

    #[test]
    fn shift_is_not_a_chord_modifier() {
        let e = Chord::parse("ctrl+shift").unwrap_err();
        assert!(e.contains("shift"), "{e}");
        assert!(
            e.contains("hold"),
            "the message must say WHERE shift is allowed instead: {e}"
        );
    }

    #[test]
    fn a_main_key_in_a_chord_is_rejected() {
        let e = Chord::parse("ctrl+alt+t").unwrap_err();
        assert!(e.contains('t'), "{e}");
    }

    #[test]
    fn caps_hold_defaults_when_absent_and_parses_when_present() {
        let d = parse_config("\"ctrl+alt+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(d.keyboard.caps_hold, Chord::default());

        let c = parse_config(
            "keyboard.caps = true\nkeyboard.caps_hold = \"ctrl+alt\"\n\"ctrl+alt+t\" = \"Terminal\"\n",
        )
        .unwrap();
        assert_eq!(c.keyboard.caps_hold, Chord::parse("ctrl+alt").unwrap());
    }

    #[test]
    fn an_invalid_caps_hold_names_the_key_in_the_error() {
        let e = parse_config("keyboard.caps_hold = \"ctrl+shift\"\n").unwrap_err();
        assert!(e.contains("caps_hold"), "{e}");
    }

    /// A Windows-only setting must not fail `beckon check` on another OS: one
    /// config file is meant to travel between machines.
    #[test]
    fn caps_hold_parses_on_every_platform() {
        assert!(parse_config("keyboard.caps_hold = \"ctrl+super+alt\"\n").is_ok());
    }
}
