//! Settings-window model. Everything the window draws is computed here, so
//! the drawing is a pure function of a snapshot — the same shape
//! `MenuModel`/`build_entries` already use for the tray menu, and for the
//! same reason: it can be tested without a window, a message loop or a
//! registry.

use crate::config_write::{render, RowWrite};
use crate::shortcuts::{parse_config, CapsTap, Combo, KeyboardConfig};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The raw key this row was loaded from; `None` for a row the user
    /// added. Passed straight through to `RowWrite` so an untouched row
    /// keeps its original spelling and position in the file.
    pub orig_key: Option<String>,
    pub combo: String,
    pub app: String,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub rows: Vec<Row>,
    pub keyboard: KeyboardConfig,
    pub selected: Option<usize>,
    original: String,
    dirty: bool,
}

/// One reason a row cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub row: usize,
    pub message: String,
}

/// What `serve` knows that the file does not.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    /// Canonical combo -> registration outcome, from the last pass. Keyed
    /// by canonical string rather than by index because `ServeState` holds
    /// shortcuts in `toml::Table` order while the window shows file order,
    /// and aligning two different orderings by position is a bug waiting
    /// to happen.
    pub registered: HashMap<String, Result<(), String>>,
    /// Installed app names. `None` until the catalog scan finishes — which
    /// is NOT the same as "no apps installed", and the UI must not conflate
    /// the two.
    pub catalog: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Ok,
    Bad,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub combo: String,
    pub app: String,
    pub mark: Mark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub mark: Mark,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub combo: String,
    pub app: String,
    pub notes: Vec<Note>,
}

/// Exactly what the window draws. The window never reads `Model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlState {
    pub items: Vec<ListItem>,
    pub detail: Option<Detail>,
    pub caps_checked: bool,
    pub caps_tap: CapsTap,
    pub apply_enabled: bool,
    pub remove_enabled: bool,
}

impl Model {
    pub fn from_text(text: &str) -> Result<Model, String> {
        let cfg = parse_config(text)?;
        // `parse_config` returns BTreeMap order; the window shows file
        // order, which is what the user sees in their editor.
        let order = top_level_keys(text);
        let mut rows: Vec<Row> = cfg
            .shortcuts
            .iter()
            .map(|s| {
                let canon = s.combo.canonical();
                let raw = order
                    .iter()
                    .find(|k| {
                        Combo::parse(k)
                            .map(|c| c.canonical() == canon)
                            .unwrap_or(false)
                    })
                    .cloned();
                Row {
                    orig_key: raw.clone(),
                    combo: raw.unwrap_or(canon),
                    app: s.app.clone(),
                }
            })
            .collect();
        rows.sort_by_key(|r| {
            r.orig_key
                .as_deref()
                .and_then(|k| order.iter().position(|o| o == k))
                .unwrap_or(usize::MAX)
        });
        Ok(Model {
            rows,
            keyboard: cfg.keyboard,
            selected: None,
            original: text.to_string(),
            dirty: false,
        })
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_combo(&mut self, i: usize, v: &str) {
        if self.rows[i].combo != v {
            self.rows[i].combo = v.to_string();
            self.dirty = true;
        }
    }

    pub fn set_app(&mut self, i: usize, v: &str) {
        if self.rows[i].app != v {
            self.rows[i].app = v.to_string();
            self.dirty = true;
        }
    }

    pub fn add_row(&mut self) {
        self.rows.push(Row {
            orig_key: None,
            combo: String::new(),
            app: String::new(),
        });
        self.selected = Some(self.rows.len() - 1);
        self.dirty = true;
    }

    pub fn remove_row(&mut self, i: usize) {
        self.rows.remove(i);
        self.selected = if self.rows.is_empty() {
            None
        } else {
            Some(i.min(self.rows.len() - 1))
        };
        self.dirty = true;
    }

    pub fn set_caps(&mut self, on: bool) {
        if self.keyboard.caps != on {
            self.keyboard.caps = on;
            self.dirty = true;
        }
    }

    pub fn set_caps_tap(&mut self, t: CapsTap) {
        if self.keyboard.caps_tap != t {
            self.keyboard.caps_tap = t;
            self.dirty = true;
        }
    }

    /// Every reason this model cannot be written, one entry per offending
    /// row. A row may appear more than once.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let mut canon: Vec<Option<String>> = Vec::with_capacity(self.rows.len());
        for (i, r) in self.rows.iter().enumerate() {
            match Combo::parse(&r.combo) {
                Ok(c) => canon.push(Some(c.canonical())),
                Err(e) => {
                    canon.push(None);
                    out.push(Problem { row: i, message: e });
                }
            }
            if r.app.trim().is_empty() {
                out.push(Problem {
                    row: i,
                    message: "app name is empty".to_string(),
                });
            }
        }
        // Duplicates: flag EVERY row in a colliding group, not just the
        // later ones -- the user needs to see both ends of the collision.
        let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, c) in canon.iter().enumerate() {
            if let Some(c) = c {
                groups.entry(c.as_str()).or_default().push(i);
            }
        }
        let mut dups: Vec<(&str, Vec<usize>)> =
            groups.into_iter().filter(|(_, v)| v.len() > 1).collect();
        dups.sort_by_key(|(c, _)| *c);
        for (c, rows) in dups {
            for i in rows {
                out.push(Problem {
                    row: i,
                    message: format!("duplicate shortcut: another row also means `{c}`"),
                });
            }
        }
        out.sort_by_key(|p| p.row);
        out
    }

    /// The file text this model would write. `Err` if the model is invalid
    /// or the writer refuses. Never touches the filesystem.
    pub fn render(&self) -> Result<String, String> {
        if let Some(p) = self.problems().first() {
            return Err(format!("row {}: {}", p.row + 1, p.message));
        }
        let writes: Vec<RowWrite> = self
            .rows
            .iter()
            .map(|r| RowWrite {
                orig_key: r.orig_key.clone(),
                combo: r.combo.clone(),
                app: r.app.trim().to_string(),
            })
            .collect();
        let text = render(&self.original, &writes, &self.keyboard)?;
        // Validate through the real parser rather than a second rule set:
        // this is what makes "what the UI writes is what beckon reads" true
        // by construction.
        parse_config(&text)?;
        Ok(text)
    }
}

/// Bare `key = value` lines at the root, in file order.
///
/// Deliberately a line scanner and not a parser: its only job is to make
/// the window show rows in the order the user wrote them. Anything it
/// misses simply falls back to canonical order, and `render` still
/// validates through `parse_config`, so this cannot become a second source
/// of truth about the file.
fn top_level_keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            break; // a table header: everything after belongs to it
        }
        if t.starts_with('#') {
            continue;
        }
        let Some(eq) = t.find('=') else { continue };
        let raw = t[..eq].trim();
        if raw.is_empty() {
            continue;
        }
        let key = raw.trim_matches(|c| c == '"' || c == '\'');
        // Skip dotted keys (`keyboard.caps`): they are settings, not combos.
        if key.contains('.') && !key.starts_with('"') {
            continue;
        }
        out.push(key.to_string());
    }
    out
}

pub fn control_state(m: &Model, rt: &RuntimeStatus) -> ControlState {
    let problems = m.problems();
    let bad_rows: std::collections::HashSet<usize> = problems.iter().map(|p| p.row).collect();

    let items = m
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| ListItem {
            combo: r.combo.clone(),
            app: r.app.clone(),
            mark: if bad_rows.contains(&i) {
                Mark::Bad
            } else {
                match Combo::parse(&r.combo)
                    .ok()
                    .and_then(|c| rt.registered.get(&c.canonical()))
                {
                    Some(Ok(())) => Mark::Ok,
                    Some(Err(_)) => Mark::Bad,
                    None => Mark::Unknown,
                }
            },
        })
        .collect();

    let detail = m.selected.and_then(|i| {
        m.rows.get(i).map(|r| {
            let mut notes = Vec::new();
            match Combo::parse(&r.combo) {
                Ok(c) => match rt.registered.get(&c.canonical()) {
                    Some(Ok(())) => notes.push(Note {
                        mark: Mark::Ok,
                        text: "registered".into(),
                    }),
                    Some(Err(e)) => notes.push(Note {
                        mark: Mark::Bad,
                        text: format!("not registered: {e}"),
                    }),
                    None => notes.push(Note {
                        mark: Mark::Unknown,
                        text: "not registered yet".into(),
                    }),
                },
                Err(e) => notes.push(Note {
                    mark: Mark::Bad,
                    text: e,
                }),
            }
            notes.push(match &rt.catalog {
                // A scan that has not run cannot prove absence.
                None => Note {
                    mark: Mark::Unknown,
                    text: "checking installed apps...".into(),
                },
                Some(names) => {
                    let want = r.app.trim().to_lowercase();
                    if !want.is_empty() && names.iter().any(|n| n.to_lowercase() == want) {
                        Note {
                            mark: Mark::Ok,
                            text: "found in installed apps".into(),
                        }
                    } else {
                        Note {
                            mark: Mark::Bad,
                            text: "no installed app has this name".into(),
                        }
                    }
                }
            });
            for p in problems.iter().filter(|p| p.row == i) {
                notes.push(Note {
                    mark: Mark::Bad,
                    text: p.message.clone(),
                });
            }
            Detail {
                combo: r.combo.clone(),
                app: r.app.clone(),
                notes,
            }
        })
    });

    ControlState {
        items,
        detail,
        caps_checked: m.keyboard.caps,
        caps_tap: m.keyboard.caps_tap,
        apply_enabled: m.dirty() && problems.is_empty(),
        remove_enabled: m.selected.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str =
        "# mine\n\"ctrl+super+alt+t\" = \"Terminal\"\n\"ctrl+super+alt+e\" = \"File Explorer\"\n";

    fn model() -> Model {
        Model::from_text(FILE).unwrap()
    }

    #[test]
    fn loading_keeps_file_order_and_original_spelling() {
        let m = Model::from_text("\"alt+ctrl+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(m.rows[0].orig_key.as_deref(), Some("alt+ctrl+t"));
        assert_eq!(m.rows[0].combo, "alt+ctrl+t");
        assert!(!m.dirty(), "just-loaded is not dirty");
    }

    /// `parse_config` returns BTreeMap order; the window must not.
    #[test]
    fn file_order_is_kept_even_when_it_is_not_alphabetical() {
        let m = Model::from_text(
            "\"ctrl+alt+z\" = \"Zed\"\n\"ctrl+alt+a\" = \"Apple\"\n\"ctrl+alt+m\" = \"Mail\"\n",
        )
        .unwrap();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(apps, vec!["Zed", "Apple", "Mail"]);
    }

    #[test]
    fn editing_marks_dirty_and_keeps_comments_on_render() {
        let mut m = model();
        assert!(!m.dirty());
        m.set_app(0, "Windows Terminal");
        assert!(m.dirty());
        let out = m.render().unwrap();
        assert!(out.contains("Windows Terminal"));
        assert!(out.contains("# mine"), "comment lost:\n{out}");
    }

    #[test]
    fn add_and_remove_rows() {
        let mut m = model();
        m.add_row();
        assert_eq!(m.rows.len(), 3);
        assert_eq!(m.selected, Some(2), "a new row selects itself");
        assert!(m.rows[2].orig_key.is_none());
        m.remove_row(2);
        assert_eq!(m.rows.len(), 2);
        assert!(m.dirty());
    }

    #[test]
    fn removing_the_last_row_clears_the_selection() {
        let mut m = Model::from_text("\"ctrl+alt+t\" = \"Terminal\"\n").unwrap();
        m.selected = Some(0);
        m.remove_row(0);
        assert_eq!(m.selected, None);
    }

    // ---------- validation ----------

    #[test]
    fn a_bad_combo_is_reported_verbatim_from_the_parser() {
        let mut m = model();
        m.set_combo(0, "ctrl+super+alt+T");
        let p = m.problems();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].row, 0);
        assert!(p[0].message.contains("uppercase"), "{}", p[0].message);
    }

    #[test]
    fn an_empty_app_is_a_problem() {
        let mut m = model();
        m.set_app(1, "   ");
        let p = m.problems();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].row, 1);
    }

    #[test]
    fn duplicates_flag_both_rows_and_name_the_canonical_form() {
        let mut m = model();
        m.set_combo(1, "alt+ctrl+super+t");
        let p = m.problems();
        assert_eq!(p.len(), 2, "both rows must be flagged, not just the second");
        assert!(p.iter().any(|x| x.row == 0));
        assert!(p.iter().any(|x| x.row == 1));
        assert!(
            p[0].message.contains("ctrl+super+alt+t"),
            "{}",
            p[0].message
        );
    }

    #[test]
    fn render_refuses_an_invalid_model() {
        let mut m = model();
        m.set_combo(0, "nope+t");
        assert!(m.render().is_err());
    }

    /// The load-bearing guarantee: what the UI writes, the parser accepts.
    #[test]
    fn every_valid_model_round_trips_through_the_real_parser() {
        let mut m = model();
        m.set_app(0, "Windows Terminal");
        m.add_row();
        m.set_combo(2, "ctrl+super+alt+c");
        m.set_app(2, "Claude");
        m.set_caps(true);
        m.set_caps_tap(CapsTap::Escape);
        assert!(m.problems().is_empty(), "{:?}", m.problems());

        let text = m.render().unwrap();
        let parsed = parse_config(&text).expect("the writer must emit what the reader accepts");
        assert_eq!(parsed.shortcuts.len(), 3);
        assert!(parsed.keyboard.caps);
        assert_eq!(parsed.keyboard.caps_tap, CapsTap::Escape);

        let reloaded = Model::from_text(&text).unwrap();
        assert_eq!(reloaded.rows.len(), 3);
        assert_eq!(reloaded.keyboard, m.keyboard);
        assert!(!reloaded.dirty());
    }

    // ---------- the drawing projection ----------

    fn status_all_ok() -> RuntimeStatus {
        let mut r = HashMap::new();
        r.insert("ctrl+super+alt+t".to_string(), Ok(()));
        r.insert("ctrl+super+alt+e".to_string(), Ok(()));
        RuntimeStatus {
            registered: r,
            catalog: Some(vec!["Terminal".into(), "File Explorer".into()]),
        }
    }

    #[test]
    fn a_healthy_row_is_marked_ok() {
        let cs = control_state(&model(), &status_all_ok());
        assert_eq!(cs.items.len(), 2);
        assert_eq!(cs.items[0].mark, Mark::Ok);
        assert!(!cs.apply_enabled, "nothing to apply on a clean model");
    }

    #[test]
    fn a_failed_registration_marks_the_right_row() {
        let mut st = status_all_ok();
        st.registered.insert(
            "ctrl+super+alt+e".into(),
            Err("hotkey already taken".into()),
        );
        let cs = control_state(&model(), &st);
        assert_eq!(cs.items[0].mark, Mark::Ok);
        assert_eq!(cs.items[1].mark, Mark::Bad);
    }

    /// A scan that did not run cannot prove an app is absent.
    #[test]
    fn an_unscanned_catalog_shows_unknown_not_missing() {
        let st = RuntimeStatus {
            registered: status_all_ok().registered,
            catalog: None,
        };
        let mut m = model();
        m.selected = Some(0);
        let cs = control_state(&m, &st);
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .expect("there must be an app-resolution note");
        assert_eq!(note.mark, Mark::Unknown);
    }

    #[test]
    fn an_app_missing_from_a_scanned_catalog_is_marked_bad() {
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .unwrap();
        assert_eq!(note.mark, Mark::Bad);
    }

    #[test]
    fn catalog_matching_is_case_insensitive_like_every_beckon_resolver() {
        let mut m = model();
        m.set_app(0, "terminal");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .unwrap();
        assert_eq!(note.mark, Mark::Ok);
    }

    #[test]
    fn apply_needs_both_dirty_and_valid() {
        let mut m = model();
        assert!(!control_state(&m, &status_all_ok()).apply_enabled);
        m.set_app(0, "Windows Terminal");
        assert!(control_state(&m, &status_all_ok()).apply_enabled);
        m.set_combo(0, "bad+++");
        assert!(
            !control_state(&m, &status_all_ok()).apply_enabled,
            "a broken model must not be writable"
        );
    }

    #[test]
    fn remove_is_disabled_with_no_selection() {
        let mut m = model();
        assert!(!control_state(&m, &status_all_ok()).remove_enabled);
        m.selected = Some(1);
        assert!(control_state(&m, &status_all_ok()).remove_enabled);
    }

    #[test]
    fn the_keyboard_group_reflects_the_model() {
        let mut m = model();
        m.set_caps(true);
        m.set_caps_tap(CapsTap::None);
        let cs = control_state(&m, &status_all_ok());
        assert!(cs.caps_checked);
        assert_eq!(cs.caps_tap, CapsTap::None);
    }

    /// The keyboard block is not a shortcut and must never show up as a row.
    #[test]
    fn dotted_keyboard_settings_do_not_become_rows() {
        let m = Model::from_text(
            "keyboard.caps = true\nkeyboard.caps_tap = \"escape\"\n\"ctrl+alt+t\" = \"Terminal\"\n",
        )
        .unwrap();
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.rows[0].app, "Terminal");
        assert!(m.keyboard.caps);
    }
}
