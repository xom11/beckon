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
    //
    //    Only keys that ARE shortcuts are candidates. The filter used to be
    //    "not `keyboard` and not kept", which made every top-level key beckon
    //    did not recognise collateral damage: a `[defaults]` block, a
    //    `version = 2`, anything a later format or a hand edit adds would be
    //    silently deleted by the first Save from the settings window. A
    //    removed binding still parses as a combo, so removal is unaffected.
    let doomed: Vec<String> = doc
        .as_table()
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| {
            k != KEYBOARD_KEY
                && !keep.contains(k.as_str())
                && crate::shortcuts::Combo::parse(k).is_ok()
        })
        .collect();
    for k in doomed {
        doc.remove(&k);
    }

    // 2. Write every row. A kept key already exists; assigning a whole
    //    fresh `Item` over it would drop that line's decor, and the decor
    //    is where a trailing `# comment` lives. Swap only the value and put
    //    the decor back. Anything else is a plain insert at the end.
    //
    //    A value that is not a string is left exactly as it was.
    //    `as_value_mut()` succeeds on an array and an inline table too, and
    //    the assignment below would flatten either one to whatever single
    //    string the model holds — silently, on a row nobody touched.
    //    `RowWrite.app` is a `String`, so the model cannot carry a richer
    //    shape and therefore cannot faithfully write one back; declining is
    //    the only lossless answer it has.
    for r in rows {
        if let Some(existing) = doc.get_mut(r.combo.as_str()).and_then(|i| i.as_value_mut()) {
            if !existing.is_str() {
                continue;
            }
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
    //
    //    "Whatever shape the user gave it" includes `keyboard = { caps =
    //    true }`. `as_table_mut()` returns `None` for an `InlineTable`, so
    //    that one spelling — which `parse_config` accepts, `beckon check`
    //    calls valid and `serve` runs on — made every Save from the settings
    //    window fail with "`keyboard` is not a table", permanently and with
    //    nothing on screen naming the line at fault. `as_table_like_mut()`
    //    is the accessor that covers a header, a dotted key AND an inline
    //    table; the error is kept for the shapes that really are not a table
    //    (`keyboard = 5`).
    //
    //    The writes go through `entry(..).or_insert(Item::None)` rather than
    //    `TableLike::insert`, which is what `kb["caps"] = ..` was doing
    //    before through `Table`'s `IndexMut`. `Table::insert` calls
    //    `fmt()` on an existing key, which drops that key's decor: a
    //    hand-aligned `caps      = true` would come back as `caps = true` on
    //    an unrelated Save — the same gratuitous diff `RowWrite::orig_key`
    //    exists to avoid, documented at the top of this file.
    if doc.get(KEYBOARD_KEY).is_none() {
        let mut t = Table::new();
        t.set_dotted(true);
        doc.insert(KEYBOARD_KEY, Item::Table(t));
    }
    let kb = doc[KEYBOARD_KEY]
        .as_table_like_mut()
        .ok_or_else(|| "`keyboard` is not a table".to_string())?;
    *kb.entry("caps").or_insert(Item::None) = toml_edit::value(keyboard.caps);
    *kb.entry("caps_tap").or_insert(Item::None) = toml_edit::value(keyboard.caps_tap.as_str());
    // Written ONLY when it carries information. Unknown keys under
    // `keyboard` are a hard error by design, so a file that always carried
    // this key would be rejected outright by any beckon built before it
    // existed -- a real scenario when one machine updates through Scoop and
    // another has not yet.
    if keyboard.caps_hold.is_default() {
        kb.remove("caps_hold");
    } else {
        *kb.entry("caps_hold").or_insert(Item::None) =
            toml_edit::value(keyboard.caps_hold.canonical());
    }

    // 4. Put the file's header back if step 1 ate it.
    //
    // `toml_edit` carries a leading comment block as the PREFIX DECOR OF THE
    // FIRST KEY, not as a property of the document. So `doc.remove(k)` on the
    // first key deletes the user's header along with the binding -- silently,
    // and they only find out by diffing. Found on a14: a stray keypress
    // deleted the top row of a 21-row config, and the saved file had lost
    // `# hardware pass 2 ...` as well as the row.
    //
    // Re-adding is guarded by a `contains` rather than by "did we remove the
    // first key", because the second question has more ways to be answered
    // wrongly than the first.
    let out = doc.to_string();
    let header = leading_comment_block(original);
    if !header.trim().is_empty() && !out.contains(header.trim_end()) {
        return Ok(format!("{header}{out}"));
    }
    Ok(out)
}

/// The comment block at the very top of the file: every leading blank or
/// `#` line, up to the first line that is neither.
///
/// Deliberately taken from the ORIGINAL TEXT rather than from the parsed
/// document. Once the key that carried the comment has been removed, there is
/// nothing left in the document to read it back from.
fn leading_comment_block(original: &str) -> String {
    let mut out = String::new();
    for line in original.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            out.push_str(line);
            out.push('\n');
        } else {
            break;
        }
    }
    out
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
        assert!(
            out.contains("# the good one"),
            "trailing comment lost:\n{out}"
        );
        assert!(
            out.contains("\"alt+ctrl+t\""),
            "an untouched row was re-spelled:\n{out}"
        );
        assert!(out.contains("Files"), "the edit did not land:\n{out}");
    }

    /// A Save must not delete what beckon does not recognise.
    ///
    /// The `doomed` filter used to be "not `keyboard` and not kept", so every
    /// other top-level key in the file was collateral: a `[defaults]` block, a
    /// `version = 2`, anything a later format or a hand edit adds. The window
    /// would have eaten it on the first Save of an unrelated row, with no
    /// error and nothing on screen to notice.
    #[test]
    fn a_top_level_key_that_is_not_a_shortcut_survives_a_save() {
        let src = "version = 2\n\n\"ctrl+alt+t\" = \"kitty\"\n\n[defaults]\nmatch = \"exact\"\n";
        let out = render(
            src,
            &[row("ctrl+alt+t", "Alacritty")],
            &KeyboardConfig::default(),
        )
        .expect("renders");
        assert!(out.contains("version = 2"), "{out}");
        assert!(out.contains("[defaults]"), "{out}");
        assert!(out.contains("match = \"exact\""), "{out}");
        assert!(out.contains("Alacritty"), "the edit still lands: {out}");
    }

    /// And must not flatten a value it cannot represent.
    ///
    /// `as_value_mut()` succeeds on an array as readily as on a string, so the
    /// assignment used to rewrite `["A", "B"]` as whatever single string the
    /// model held — on a row the user never touched, because `Model::render`
    /// sends every row on every Save.
    #[test]
    fn a_value_the_model_cannot_hold_is_left_exactly_as_it_was() {
        let src = "\"ctrl+alt+b\" = [\"Brave Browser\", \"Brave\"]\n\"ctrl+alt+t\" = \"kitty\"\n";
        let out = render(
            src,
            &[
                row("ctrl+alt+b", "Brave Browser"),
                row("ctrl+alt+t", "foot"),
            ],
            &KeyboardConfig::default(),
        )
        .expect("renders");
        assert!(
            out.contains("[\"Brave Browser\", \"Brave\"]"),
            "the array must survive verbatim: {out}"
        );
        assert!(
            out.contains("\"foot\""),
            "the string row still edits: {out}"
        );
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
            caps_hold: crate::shortcuts::Chord::default(),
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
            caps_hold: crate::shortcuts::Chord::default(),
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

    /// The reader accepts three spellings of the settings block and the
    /// writer has to survive all three. This is the one it did not: `toml`
    /// hands an inline table back as a table, so `parse_config` is happy,
    /// `beckon check` says `ok` and `serve` registers every shortcut -- while
    /// `as_table_mut()` returned `None` and EVERY Save from the settings
    /// window failed with "`keyboard` is not a table", for the life of the
    /// file. Measured before the fix: `render` returned
    /// `Err("`keyboard` is not a table")` on exactly this input.
    #[test]
    fn an_inline_keyboard_table_is_edited_rather_than_refused() {
        let original =
            "keyboard = { caps = true, caps_tap = \"escape\" }\n\"ctrl+alt+t\" = \"Terminal\"\n";
        let kb = KeyboardConfig {
            caps: false,
            caps_tap: CapsTap::CapsLock,
            caps_hold: crate::shortcuts::Chord::default(),
        };
        let out = render(original, &[row("ctrl+alt+t", "Terminal")], &kb)
            .unwrap_or_else(|e| panic!("an inline table is a table: {e}"));
        let c = parse_config(&out).unwrap_or_else(|e| panic!("must round-trip: {e}\n{out}"));
        assert_eq!(c.keyboard, kb, "the edit did not land:\n{out}");
        assert_eq!(c.shortcuts.len(), 1, "{out}");
    }

    /// And the removal arm reaches inside one too: `caps_hold` is dropped
    /// when it goes back to the default, whatever shape holds it.
    #[test]
    fn resetting_caps_hold_to_default_removes_it_from_an_inline_table() {
        let original = "keyboard = { caps = true, caps_hold = \"ctrl+alt\" }\n";
        let out = render(original, &[], &KeyboardConfig::default()).unwrap();
        assert!(
            !out.contains("caps_hold"),
            "the stale line survived inside the inline table:\n{out}"
        );
        let c = parse_config(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
        assert_eq!(c.keyboard.caps_hold, crate::shortcuts::Chord::default());
        assert!(!c.keyboard.caps, "unticking did not persist:\n{out}");
    }

    /// A shape that is genuinely not a table still says so, rather than
    /// panicking through `Index` — `as_table_like_mut` widened what counts
    /// as a table, it did not delete the guard.
    #[test]
    fn a_keyboard_key_that_is_not_a_table_at_all_is_still_an_error() {
        let err = render("keyboard = 5\n", &[], &KeyboardConfig::default())
            .expect_err("`keyboard = 5` is not a settings block");
        assert!(err.contains("not a table"), "{err}");
    }

    /// The decor of a key that is only being re-valued must survive. This is
    /// why the writes go through `entry(..).or_insert(..)` and not
    /// `TableLike::insert`, whose `Table` arm calls `fmt()` on the existing
    /// key and would return `caps = true` for the aligned line below.
    #[test]
    fn a_hand_aligned_keyboard_block_is_not_re_aligned_by_a_save() {
        let original = "[keyboard]\ncaps      = false\ncaps_tap  = \"none\"\n";
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::Escape,
            caps_hold: crate::shortcuts::Chord::default(),
        };
        let out = render(original, &[], &kb).unwrap();
        assert!(
            out.contains("caps      ="),
            "the key's own spacing was reformatted:\n{out}"
        );
        assert!(
            out.contains("caps_tap  ="),
            "the key's own spacing was reformatted:\n{out}"
        );
        let c = parse_config(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
        assert_eq!(c.keyboard, kb, "the edit did not land:\n{out}");
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
            caps_hold: crate::shortcuts::Chord::default(),
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

    #[test]
    fn a_default_caps_hold_is_not_written_at_all() {
        let out = render("", &[], &KeyboardConfig::default()).unwrap();
        assert!(
            !out.contains("caps_hold"),
            "an untouched default must stay readable by older beckon binaries, \
             which reject unknown keys under `keyboard`:\n{out}"
        );
    }

    #[test]
    fn a_non_default_caps_hold_is_written() {
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::CapsLock,
            caps_hold: crate::shortcuts::Chord::parse("ctrl+alt").unwrap(),
        };
        let out = render("", &[], &kb).unwrap();
        assert!(out.contains("caps_hold"), "{out}");
        assert!(out.contains("ctrl+alt"), "{out}");
        parse_config(&out).expect("the writer must emit what the reader accepts");
    }

    /// Mirrors `caps_off_is_still_written_so_unticking_persists`, but for
    /// `caps_hold`: the file on disk already carries a non-default line, and
    /// resetting to the default in the model must remove it, not just skip
    /// writing a fresh one over it. Both new writer tests above start from
    /// an *empty* document, where `kb.remove("caps_hold")` is a no-op and
    /// so cannot catch a deleted removal call -- this one starts from a
    /// document that already has the key.
    #[test]
    fn resetting_caps_hold_to_default_removes_the_stale_line() {
        let original = "keyboard.caps_hold = \"ctrl+alt\"\n\"ctrl+alt+t\" = \"Terminal\"\n";
        let out = render(
            original,
            &[row("ctrl+alt+t", "Terminal")],
            &KeyboardConfig::default(),
        )
        .unwrap();
        assert!(
            !out.contains("caps_hold"),
            "resetting to the default left the stale line behind, which an \
             older beckon rejects as an unknown key under `keyboard`:\n{out}"
        );
        let c = parse_config(&out).unwrap();
        assert_eq!(c.keyboard.caps_hold, crate::shortcuts::Chord::default());
    }
}
