//! Write a shortcuts file back out without destroying what the user wrote.
//!
//! `toml::Table` loses every comment on re-serialization. This is a file
//! beckon invites people to edit by hand, so the settings window edits the
//! document in place through `toml_edit` instead of rendering a fresh one.

use crate::shortcuts::{KeyboardConfig, KEYBOARD_KEY};

/// One row on its way back to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowWrite {
    /// The raw key this row was loaded from; `None` for a row the user
    /// added. A row whose combo still matches its `orig_key` is updated in
    /// place and keeps its original spelling, position and trailing
    /// comment — the file may say `alt+ctrl+t` where the canonical form is
    /// `ctrl+alt+t`, and rewriting that on an unrelated save would be a
    /// gratuitous diff in someone's own file.
    pub orig_key: Option<String>,
    pub combo: String,
    pub app: String,
}

/// Apply `rows` and `keyboard` to `original`, returning the new file text.
pub fn render(
    original: &str,
    rows: &[RowWrite],
    keyboard: &KeyboardConfig,
) -> Result<String, String> {
    use toml_edit::{DocumentMut, Item, Table};

    let mut doc: DocumentMut = if original.trim().is_empty() {
        DocumentMut::new()
    } else {
        original
            .parse()
            .map_err(|e: toml_edit::TomlError| e.to_string())?
    };

    // 1. Drop every shortcut key that no longer has a row spelling it the
    //    same way. A retyped combo is a remove-plus-insert, not an edit.
    let keep: std::collections::HashSet<&str> = rows
        .iter()
        .filter(|r| r.orig_key.as_deref() == Some(r.combo.as_str()))
        .filter_map(|r| r.orig_key.as_deref())
        .collect();
    let doomed: Vec<String> = doc
        .as_table()
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| k != KEYBOARD_KEY && !keep.contains(k.as_str()))
        .collect();
    for k in doomed {
        doc.remove(&k);
    }

    // 2. Write every row. A kept key already exists; assigning a whole
    //    fresh `Item` over it would drop that line's decor, and the decor
    //    is where a trailing `# comment` lives. Swap only the value and put
    //    the decor back. Anything else is a plain insert at the end.
    for r in rows {
        if let Some(existing) = doc.get_mut(r.combo.as_str()).and_then(|i| i.as_value_mut()) {
            let decor = existing.decor().clone();
            *existing = toml_edit::Value::from(r.app.as_str());
            *existing.decor_mut() = decor;
        } else {
            doc[r.combo.as_str()] = toml_edit::value(r.app.as_str());
        }
    }

    // 3. Keyboard settings. An existing `keyboard` item is edited in place
    //    whatever shape the user gave it; a fresh one is created DOTTED,
    //    never as a `[keyboard]` header — a header captures every bare
    //    key-value pair written after it, which would silently swallow the
    //    next shortcut appended by hand.
    if doc.get(KEYBOARD_KEY).is_none() {
        let mut t = Table::new();
        t.set_dotted(true);
        doc.insert(KEYBOARD_KEY, Item::Table(t));
    }
    let kb = doc[KEYBOARD_KEY]
        .as_table_mut()
        .ok_or_else(|| "`keyboard` is not a table".to_string())?;
    kb["caps"] = toml_edit::value(keyboard.caps);
    kb["caps_tap"] = toml_edit::value(keyboard.caps_tap.as_str());

    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::{parse_config, CapsTap};

    fn row(key: &str, app: &str) -> RowWrite {
        RowWrite {
            orig_key: Some(key.to_string()),
            combo: key.to_string(),
            app: app.to_string(),
        }
    }

    #[test]
    fn comments_and_spelling_survive_an_unrelated_edit() {
        let original = "# my keys\n\n\"alt+ctrl+t\" = \"Terminal\"  # the good one\n\"ctrl+alt+e\" = \"Explorer\"\n";
        let rows = vec![row("alt+ctrl+t", "Terminal"), row("ctrl+alt+e", "Files")];
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        assert!(out.contains("# my keys"), "header comment lost:\n{out}");
        assert!(out.contains("# the good one"), "trailing comment lost:\n{out}");
        assert!(
            out.contains("\"alt+ctrl+t\""),
            "an untouched row was re-spelled:\n{out}"
        );
        assert!(out.contains("Files"), "the edit did not land:\n{out}");
    }

    #[test]
    fn a_removed_row_disappears_and_the_rest_stay() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n\"ctrl+alt+e\" = \"Explorer\"\n";
        let rows = vec![row("ctrl+alt+e", "Explorer")];
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap();
        assert_eq!(c.shortcuts.len(), 1);
        assert_eq!(c.shortcuts[0].app, "Explorer");
    }

    #[test]
    fn a_new_row_is_appended() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n";
        let rows = vec![
            row("ctrl+alt+t", "Terminal"),
            RowWrite {
                orig_key: None,
                combo: "ctrl+alt+c".into(),
                app: "Claude".into(),
            },
        ];
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap();
        assert_eq!(c.shortcuts.len(), 2);
        assert!(c.shortcuts.iter().any(|s| s.app == "Claude"));
    }

    #[test]
    fn a_retyped_combo_replaces_the_old_key() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n";
        let rows = vec![RowWrite {
            orig_key: Some("ctrl+alt+t".into()),
            combo: "ctrl+alt+y".into(),
            app: "Terminal".into(),
        }];
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap();
        assert_eq!(c.shortcuts.len(), 1, "the old key survived:\n{out}");
        assert_eq!(c.shortcuts[0].combo.canonical(), "ctrl+alt+y");
    }

    #[test]
    fn keyboard_settings_are_written_as_dotted_keys_when_created() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n";
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::Escape,
        };
        let out = render(original, &[row("ctrl+alt+t", "Terminal")], &kb).unwrap();
        assert!(
            !out.contains("[keyboard]"),
            "a table header would swallow anything appended later:\n{out}"
        );
        assert!(out.contains("keyboard.caps"), "{out}");
        let c = parse_config(&out).unwrap();
        assert!(c.keyboard.caps);
        assert_eq!(c.keyboard.caps_tap, CapsTap::Escape);
    }

    /// The file may already contain a hand-written `[keyboard]` header. Edit
    /// it in place rather than reformatting someone's file — and a newly
    /// added shortcut must NOT end up nested inside it.
    #[test]
    fn an_existing_keyboard_header_is_edited_in_place_and_never_captures_new_rows() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n\n[keyboard]\ncaps = false\n";
        let rows = vec![
            row("ctrl+alt+t", "Terminal"),
            RowWrite {
                orig_key: None,
                combo: "ctrl+alt+c".into(),
                app: "Claude".into(),
            },
        ];
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::CapsLock,
        };
        let out = render(original, &rows, &kb).unwrap();
        let c = parse_config(&out).unwrap_or_else(|e| panic!("must round-trip: {e}\n{out}"));
        assert!(c.keyboard.caps, "the header was not updated:\n{out}");
        assert_eq!(
            c.shortcuts.len(),
            2,
            "a new row was swallowed by [keyboard]:\n{out}"
        );
    }

    #[test]
    fn caps_off_is_still_written_so_unticking_persists() {
        let original = "keyboard.caps = true\n\"ctrl+alt+t\" = \"Terminal\"\n";
        let out = render(
            original,
            &[row("ctrl+alt+t", "Terminal")],
            &KeyboardConfig::default(),
        )
        .unwrap();
        let c = parse_config(&out).unwrap();
        assert!(!c.keyboard.caps, "unticking did not persist:\n{out}");
    }

    #[test]
    fn rendering_an_empty_file_produces_a_parseable_one() {
        let out = render("", &[], &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
        assert!(c.shortcuts.is_empty());
    }

    /// The load-bearing guarantee: whatever the writer emits, the reader
    /// accepts, and it means the same thing — and saving twice with no
    /// edits in between must not churn the file.
    #[test]
    fn round_trip_preserves_meaning_and_is_idempotent() {
        let original = "# keep me\n\"ctrl+alt+t\" = \"Terminal\"\n";
        let rows = vec![
            row("ctrl+alt+t", "Windows Terminal"),
            RowWrite {
                orig_key: None,
                combo: "ctrl+super+alt+c".into(),
                app: "Claude".into(),
            },
        ];
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::None,
        };
        let once = render(original, &rows, &kb).unwrap();
        let parsed = parse_config(&once).unwrap();
        assert_eq!(parsed.keyboard, kb);
        assert_eq!(parsed.shortcuts.len(), 2);
        assert!(once.contains("# keep me"), "{once}");

        // Re-render the already-rendered file with the same rows, now all
        // loaded from it.
        let rows2: Vec<RowWrite> = rows
            .iter()
            .map(|r| RowWrite {
                orig_key: Some(r.combo.clone()),
                combo: r.combo.clone(),
                app: r.app.clone(),
            })
            .collect();
        let twice = render(&once, &rows2, &kb).unwrap();
        assert_eq!(once, twice, "saving twice changed the file");
    }
}
