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

/// The reverse of `lookup_key`: the key a Windows virtual-key code belongs
/// to, or `None` for one no binding can name (numpad, media, IME,
/// `VK_PROCESSKEY`).
///
/// Linear, like `lookup_key`, over 81 entries. It runs once per captured
/// key-down, not per keystroke, so a map would buy nothing and cost a
/// second source of truth.
pub fn lookup_win_vk(vk: u32) -> Option<&'static KeyDef> {
    all_keys().iter().find(|k| k.win == vk)
}

/// The whole key list, in the order the settings window shows it.
///
/// Public so the window can fill its key list without a second copy of the
/// names. Index into it is what `ComboView::key` means, and the two must
/// stay the same slice — which is why this returns `all_keys()` rather
/// than building anything.
pub fn key_table() -> &'static [KeyDef] {
    all_keys()
}

/// A combo as the five controls that show it: four modifier check boxes and
/// one index into `key_table`.
///
/// `key` is `None` when the string does not parse — a row that has never
/// been given a shortcut, or one whose stored text is not a valid combo.
/// The window shows that as "nothing selected" rather than guessing, and
/// `Model::problems` is what tells the user why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComboView {
    pub ctrl: bool,
    pub super_: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Option<usize>,
}

impl ComboView {
    /// What these control values spell, or `None` when no key is chosen.
    ///
    /// **`None` is not an error and must not be turned into one.** A
    /// modifier set with no main key is not a combo: writing `ctrl+` into
    /// the model would make the row invalid on a keystroke the user has not
    /// finished, and flag it for a mistake it is halfway through not
    /// making. Callers send nothing instead, so the row keeps whatever it
    /// had until a key is chosen.
    ///
    /// Spelled through `Combo::canonical` rather than by joining strings,
    /// so the order a window writes and the order the parser prints cannot
    /// drift apart. `key` is looked up with `get` rather than indexed,
    /// because a control is not a proof.
    ///
    /// The exact inverse of `combo_view` for every string `combo_view`
    /// resolves a key from — pinned by
    /// `spell_round_trips_through_combo_view`.
    pub fn spell(&self) -> Option<String> {
        let key = key_table().get(self.key?)?;
        Some(
            Combo {
                ctrl: self.ctrl,
                super_: self.super_,
                alt: self.alt,
                shift: self.shift,
                key,
            }
            .canonical(),
        )
    }
}

/// Render a combo string as control values. Never fails: an unparseable
/// string is `ComboView::default()`, i.e. nothing ticked and no key chosen.
pub fn combo_view(s: &str) -> ComboView {
    let Ok(c) = Combo::parse(s) else {
        return ComboView::default();
    };
    ComboView {
        ctrl: c.ctrl,
        super_: c.super_,
        alt: c.alt,
        shift: c.shift,
        key: key_table().iter().position(|k| k.name == c.key.name),
    }
}

/// One key's label as it appears on the keyboard, for display only.
///
/// **ASCII, exhaustively.** The settings window's faces are Segoe UI
/// Variable Text and Small -- text fonts, not symbol fonts -- and a missing
/// glyph renders as a box that reads as a rendering fault rather than as a
/// key. So the punctuation keys take their own ASCII symbol (which any text
/// font has) and the arrow keys take words (because an arrow is not ASCII).
///
/// Never used for serialisation. `Combo::canonical` is that.
pub fn key_label(name: &str) -> String {
    match name {
        "space" => "Space".to_string(),
        "comma" => ",".to_string(),
        "period" => ".".to_string(),
        "slash" => "/".to_string(),
        "minus" => "-".to_string(),
        "equal" => "=".to_string(),
        "semicolon" => ";".to_string(),
        "quote" => "'".to_string(),
        "bracketleft" => "[".to_string(),
        "bracketright" => "]".to_string(),
        "backslash" => "\\".to_string(),
        "grave" => "`".to_string(),
        "tab" => "Tab".to_string(),
        "return" => "Enter".to_string(),
        "escape" => "Esc".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Del".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "pageup" => "PgUp".to_string(),
        "pagedown" => "PgDn".to_string(),
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        // Letters, digits and f1-f20: uppercase the whole thing. `t` -> `T`,
        // `f10` -> `F10`, `7` -> `7`.
        other => other.to_uppercase(),
    }
}

/// The chord as the user's keyboard spells it, one label per key.
///
/// `ctrl+super+alt+t` -> `["Ctrl", "Win", "Alt", "T"]`. **`super` is a valid
/// TOML token and a word on no keyboard**, which is the whole reason this
/// function exists.
///
/// Empty when the string does not parse -- the caller shows the raw text
/// instead, the same "show it rather than guess" rule `ComboView::key = None`
/// follows.
///
/// **Display only.** See `display_never_reaches_the_serialiser`.
pub fn combo_caps(s: &str) -> Vec<String> {
    let Ok(c) = Combo::parse(s) else {
        return Vec::new();
    };
    // Fixed order, independent of the order the string listed them in:
    // `Combo::parse` accepts free modifier order, and a display that varied
    // with it would make two identical chords look different.
    let mut v = Vec::with_capacity(5);
    if c.ctrl {
        v.push("Ctrl".to_string());
    }
    if c.super_ {
        v.push("Win".to_string());
    }
    if c.alt {
        v.push("Alt".to_string());
    }
    if c.shift {
        v.push("Shift".to_string());
    }
    v.push(key_label(&c.key.name));
    v
}

/// `combo_caps` joined for a screen reader, for a list cell, and for the
/// ellipsis fallback when the caps do not fit their column (spec §B.3).
///
/// Empty string when the chord does not parse.
pub fn combo_display(s: &str) -> String {
    combo_caps(s).join(" + ")
}

/// The label the Caps key wears when a binding is shown collapsed.
///
/// Not `key_label`'s to produce: `caps` is a real key name there (the LOCK
/// key, which `caps_tap` can select), and this is the shorthand for a chord
/// the Caps key STANDS IN for. Two different facts that would print the same
/// word, so they are kept apart -- see `caps_shorthand_is_not_the_lock_key`.
pub const CAPS_CAP: &str = "Caps";

/// `combo_caps`, with the caps chord folded into a single `Caps` cap.
///
/// Design §3.2's `Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]`. It
/// is a **view preference**, so nothing here touches what is written: the file
/// still says `ctrl+super+alt+b`, `Combo::canonical` is untouched, and the
/// editor still shows all four real modifiers. Only the list cell changes.
///
/// `hold` is `None` when the preference is off, or when Caps is not acting as
/// a shortcut key at all -- see `settings::caps_view_effective`, which is
/// where that AND lives so both halves are tested in one place.
///
/// **The fold requires an EXACT match and no Shift**, which is the rule the
/// mock-up draws rather than one invented here: its `Telegram Web` row is
/// `Ctrl Win Alt Shift T` and sits in `.chips.always`, uncollapsed. That is
/// the whole value of the preference -- once the common chord is one cap
/// wide, a binding on any other chord is the one that still looks long, and
/// spotting it costs no reading. Folding a superset would destroy exactly
/// that, and would also be a lie: `Caps+Shift+T` is not what the hook sends.
pub fn combo_caps_folded(s: &str, hold: Option<Chord>) -> Vec<String> {
    let Ok(c) = Combo::parse(s) else {
        return Vec::new();
    };
    match hold {
        Some(h) if !c.shift && c.ctrl == h.ctrl && c.super_ == h.super_ && c.alt == h.alt => {
            vec![CAPS_CAP.to_string(), key_label(&c.key.name)]
        }
        _ => combo_caps(s),
    }
}

/// `combo_caps_folded` joined the way `combo_display` joins.
///
/// The window's list cell holds this string and the painter splits it back on
/// the same separator, so the two must stay one function apart rather than
/// two spellings of a join.
pub fn combo_display_folded(s: &str, hold: Option<Chord>) -> String {
    combo_caps_folded(s, hold).join(" + ")
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
                    // lacks draws as a box that reads like a rendering bug.
                    // This is the same reason `title_base` enforces ASCII.
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
        all_keys, combo_caps, combo_caps_folded, combo_display, combo_display_folded, combo_view,
        key_label, key_table, lookup_key, lookup_win_vk, parse_config, parse_shortcuts, CapsTap,
        Chord, Combo, ComboView, KeyboardConfig, CAPS_CAP,
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

    // ---------- lookup_win_vk ----------

    #[test]
    fn a_vk_maps_back_to_the_key_that_owns_it() {
        // 0x41 is VK_A; 0x1B is VK_ESCAPE.
        assert_eq!(lookup_win_vk(0x41).map(|k| k.name.as_str()), Some("a"));
        assert_eq!(lookup_win_vk(0x1B).map(|k| k.name.as_str()), Some("escape"));
    }

    #[test]
    fn a_vk_no_key_claims_maps_to_nothing() {
        // 0xFC is VK_NONAME, which `caps` uses precisely because nothing
        // reaches it. 0x00 is not a virtual key at all.
        assert_eq!(lookup_win_vk(0xFC).map(|k| k.name.as_str()), None);
        assert_eq!(lookup_win_vk(0x00).map(|k| k.name.as_str()), None);
    }

    /// The reverse lookup is only well defined if the forward table is
    /// injective on `win`. If two keys ever share a VK, `lookup_win_vk`
    /// silently starts returning whichever the iteration order reaches
    /// first -- so pin it here rather than discovering it through a
    /// mis-captured chord.
    #[test]
    fn no_two_keys_share_a_windows_vk() {
        let mut seen: std::collections::HashMap<u32, &str> = std::collections::HashMap::new();
        for k in all_keys() {
            if let Some(prev) = seen.insert(k.win, k.name.as_str()) {
                panic!("`{prev}` and `{}` both claim VK {:#04x}", k.name, k.win);
            }
        }
    }

    /// Every key the user can type must survive the round trip, or capture
    /// can record a chord that `Combo::parse` then rejects.
    #[test]
    fn every_key_round_trips_through_its_vk() {
        for k in all_keys() {
            let back = lookup_win_vk(k.win).unwrap_or_else(|| panic!("{} lost", k.name));
            assert_eq!(back.name, k.name);
        }
    }

    // ---------- key_table / combo_view ----------

    #[test]
    fn the_key_table_is_the_whole_key_list() {
        assert_eq!(key_table().len(), all_keys().len());
        assert!(key_table().iter().any(|k| k.name == "t"));
        assert!(key_table().iter().any(|k| k.name == "escape"));
    }

    #[test]
    fn a_combo_becomes_five_control_values() {
        let v = combo_view("ctrl+super+alt+t");
        assert!(v.ctrl && v.super_ && v.alt);
        assert!(!v.shift);
        assert_eq!(key_table()[v.key.expect("a key")].name, "t");
    }

    #[test]
    fn shift_is_a_modifier_here_unlike_in_a_hold_chord() {
        let v = combo_view("ctrl+shift+a");
        assert!(v.ctrl && v.shift);
        assert!(!v.super_ && !v.alt);
        assert_eq!(key_table()[v.key.unwrap()].name, "a");
    }

    /// A row that has never been given a shortcut, and a row whose stored
    /// text does not parse, must both render as "nothing chosen" rather
    /// than panicking or inventing a key.
    #[test]
    fn an_unparseable_combo_selects_nothing() {
        for s in ["", "ctrl+", "ctrl+nosuchkey", "banana"] {
            let v = combo_view(s);
            assert_eq!(v.key, None, "{s:?} must select no key");
            assert!(!(v.ctrl || v.super_ || v.alt || v.shift), "{s:?}");
        }
    }

    /// The round trip the window depends on: whatever the controls show,
    /// rebuilding the canonical string from them must mean the same thing.
    #[test]
    fn a_view_rebuilds_the_same_canonical_combo() {
        for s in [
            "ctrl+t",
            "ctrl+super+alt+shift+f1",
            "alt+escape",
            "super+space",
        ] {
            let v = combo_view(s);
            let mut parts: Vec<&str> = Vec::new();
            if v.ctrl {
                parts.push("ctrl");
            }
            if v.super_ {
                parts.push("super");
            }
            if v.alt {
                parts.push("alt");
            }
            if v.shift {
                parts.push("shift");
            }
            let key = &key_table()[v.key.unwrap()].name;
            parts.push(key);
            assert_eq!(
                parts.join("+"),
                Combo::parse(s).unwrap().canonical(),
                "{s:?}"
            );
        }
    }

    #[test]
    fn caps_spell_the_chord_the_way_a_keyboard_does() {
        assert_eq!(
            combo_caps("ctrl+super+alt+t"),
            vec!["Ctrl", "Win", "Alt", "T"]
        );
        assert_eq!(
            combo_caps("ctrl+super+alt+shift+bracketright"),
            vec!["Ctrl", "Win", "Alt", "Shift", "]"]
        );
        // Modifier order is fixed by this function, not by the input string:
        // `Combo::parse` accepts free order, the display must not vary with it.
        assert_eq!(combo_caps("alt+ctrl+f10"), vec!["Ctrl", "Alt", "F10"]);
    }

    /// The preference OFF is the default, and off must be a no-op rather than
    /// a subtly different rendering.
    #[test]
    fn folding_with_no_chord_is_exactly_combo_caps() {
        for s in [
            "ctrl+super+alt+t",
            "ctrl+super+alt+shift+t",
            "alt+f4",
            "not a combo",
        ] {
            assert_eq!(combo_caps_folded(s, None), combo_caps(s), "{s}");
            assert_eq!(combo_display_folded(s, None), combo_display(s), "{s}");
        }
    }

    #[test]
    fn the_caps_chord_folds_to_one_cap() {
        let hold = Chord::default(); // ctrl + super + alt
        assert_eq!(
            combo_caps_folded("ctrl+super+alt+b", Some(hold)),
            vec!["Caps", "B"]
        );
        // Free modifier order in the FILE still folds -- the rule is about the
        // chord, not about how it was spelled.
        assert_eq!(
            combo_caps_folded("alt+ctrl+super+b", Some(hold)),
            vec!["Caps", "B"]
        );
        assert_eq!(
            combo_display_folded("ctrl+super+alt+b", Some(hold)),
            "Caps + B"
        );
    }

    /// **The row the mock-up draws uncollapsed**, and the reason the whole
    /// preference is worth having: once the common chord is one cap wide, a
    /// binding on any OTHER chord is the one that still looks long.
    #[test]
    fn a_binding_on_another_chord_never_folds() {
        let hold = Chord::default();
        // The mock-up's `Telegram Web`.
        assert_eq!(
            combo_caps_folded("ctrl+super+alt+shift+t", Some(hold)),
            vec!["Ctrl", "Win", "Alt", "Shift", "T"]
        );
        // A subset is not the chord either -- `Caps+F4` is not what the hook
        // sends for `alt+f4`.
        assert_eq!(combo_caps_folded("alt+f4", Some(hold)), vec!["Alt", "F4"]);
        assert_eq!(
            combo_caps_folded("ctrl+super+t", Some(hold)),
            vec!["Ctrl", "Win", "T"]
        );
    }

    /// The fold follows `keyboard.caps_hold`, not a hard-coded ctrl+win+alt.
    /// A user who set `caps_hold = "ctrl+alt"` must see THEIR chord fold and
    /// the default chord stay long.
    #[test]
    fn the_fold_follows_the_configured_hold_chord() {
        let hold = Chord {
            ctrl: true,
            super_: false,
            alt: true,
        };
        assert_eq!(
            combo_caps_folded("ctrl+alt+b", Some(hold)),
            vec!["Caps", "B"]
        );
        assert_eq!(
            combo_caps_folded("ctrl+super+alt+b", Some(hold)),
            vec!["Ctrl", "Win", "Alt", "B"]
        );
    }

    /// An unparsable combo has no chord to compare, so folding cannot invent
    /// one -- the caller shows the raw text, as it does without the preference.
    #[test]
    fn an_unparsable_combo_folds_to_nothing() {
        assert!(combo_caps_folded("not a combo", Some(Chord::default())).is_empty());
        assert!(combo_display_folded("", Some(Chord::default())).is_empty());
    }

    /// **`Caps` cannot be mistaken for a key**, which is what makes the folded
    /// cap unambiguous rather than merely short.
    ///
    /// The first draft of this test asserted that the shorthand differs from
    /// the LOCK key's own label, on the assumption that `capslock` is a
    /// bindable key. It is not: `capslock` exists only as a `CapsTap` value,
    /// and `Combo::parse` rejects it outright -- so the collision the test was
    /// guarding against cannot be constructed at all. The stronger fact is
    /// below, and it is checked against the whole table rather than one key.
    #[test]
    fn no_key_in_the_table_prints_the_folded_cap() {
        assert_eq!(CAPS_CAP, "Caps");
        for k in key_table() {
            assert_ne!(
                key_label(&k.name),
                CAPS_CAP,
                "key `{}` prints the same cap the fold uses",
                k.name
            );
        }
        // And the lock key really is unbindable, which is why the ambiguity
        // has no other route in.
        assert!(Combo::parse("ctrl+super+alt+capslock").is_err());
    }

    #[test]
    fn an_unparseable_chord_yields_no_caps_rather_than_a_guess() {
        assert!(combo_caps("").is_empty());
        assert!(combo_caps("ctrl+").is_empty());
        assert!(combo_caps("ctrl+nosuchkey").is_empty());
        assert_eq!(combo_display("ctrl+nosuchkey"), "");
    }

    #[test]
    fn display_joins_the_caps_the_way_the_window_reads_them_aloud() {
        assert_eq!(combo_display("ctrl+super+alt+t"), "Ctrl + Win + Alt + T");
        assert_eq!(combo_display("f1"), "F1");
    }

    /// Exhaustive over the 81-key table: every key must produce a non-empty,
    /// ASCII label. ASCII on purpose: the window's faces are text fonts, not
    /// symbol fonts, and a missing glyph reads as a rendering bug rather than
    /// as a key. That is why the arrow keys are words and not arrows.
    #[test]
    fn every_key_in_the_table_has_an_ascii_label() {
        for k in key_table() {
            let l = key_label(&k.name);
            assert!(!l.is_empty(), "no label for `{}`", k.name);
            assert!(l.is_ascii(), "label for `{}` is not ASCII: {l}", k.name);
        }
    }

    /// Two keys must never wear the same cap.
    ///
    /// `every_key_in_the_table_has_an_ascii_label` proves each label is
    /// non-empty and ASCII; neither of those notices a COLLISION. Adding
    /// `enter` beside `return`, or `esc` beside `escape`, would hand two rows
    /// of the settings window an identical Shortcut cell -- two different
    /// chords that read the same on screen, with nothing in the window able
    /// to tell the user which is which. `key_table_has_no_duplicates` guards
    /// the names, the mac keycodes and the win VKs; this guards the fourth
    /// column, the one the user actually reads.
    #[test]
    fn no_two_keys_share_a_label() {
        let mut seen: Vec<(String, &str)> = key_table()
            .iter()
            .map(|k| (key_label(&k.name), k.name.as_str()))
            .collect();
        seen.sort();
        for w in seen.windows(2) {
            assert_ne!(
                w[0].0, w[1].0,
                "keys `{}` and `{}` both display as `{}`",
                w[0].1, w[1].1, w[0].0
            );
        }
    }

    /// **The display path must never reach the file.** `Combo::canonical` is the
    /// serialiser; if these two ever merge, beckon writes `Win` into a TOML it
    /// then cannot parse -- a config the user did not break and cannot obviously
    /// fix. Spec §B.4.
    #[test]
    fn display_never_reaches_the_serialiser() {
        let c = Combo::parse("ctrl+super+alt+t").expect("valid combo");
        assert_eq!(c.canonical(), "ctrl+super+alt+t");
        assert!(c.canonical().contains("super"));
        assert!(!c.canonical().contains("Win"));
    }

    /// `spell` is the inverse of `combo_view`, and both settings windows
    /// depend on that being exact: `commit_fields` compares the live
    /// controls against the stored string as `ComboView`s precisely so a
    /// file written `"super+ctrl+alt+t"` does not read back as an edit.
    /// If the round trip drifted, every window open would mark rows dirty
    /// that nobody touched.
    #[test]
    fn spell_round_trips_through_combo_view() {
        for key in key_table() {
            for mods in 0u8..16 {
                let v = ComboView {
                    ctrl: mods & 1 != 0,
                    super_: mods & 2 != 0,
                    alt: mods & 4 != 0,
                    shift: mods & 8 != 0,
                    key: key_table().iter().position(|k| k.name == key.name),
                };
                let spelled = v.spell().expect("a key is selected");
                assert_eq!(
                    combo_view(&spelled),
                    v,
                    "{spelled:?} did not read back as the controls that wrote it"
                );
            }
        }
    }

    /// Modifier order in the input is free but `spell` always prints
    /// canonically, which is the property that makes the `ComboView`
    /// comparison in `commit_fields` see reordering as "no change".
    #[test]
    fn spell_is_canonical_regardless_of_how_the_file_wrote_it() {
        assert_eq!(
            combo_view("super+ctrl+alt+t").spell().unwrap(),
            combo_view("ctrl+super+alt+t").spell().unwrap()
        );
    }

    /// A modifier set with no key is not a half-combo to be repaired -- it
    /// is nothing to send. Turning this into `"ctrl+"` would flag a row for
    /// a mistake the user is halfway through not making.
    #[test]
    fn spell_declines_when_no_key_is_chosen() {
        let v = ComboView {
            ctrl: true,
            super_: true,
            alt: true,
            shift: false,
            key: None,
        };
        assert_eq!(v.spell(), None);
    }

    /// `key` is an index into `key_table()`, and a control is not a proof --
    /// an out-of-range index must decline rather than panic.
    #[test]
    fn spell_declines_on_an_index_no_key_table_entry_has() {
        let v = ComboView {
            key: Some(usize::MAX),
            ..ComboView::default()
        };
        assert_eq!(v.spell(), None);
    }
}
