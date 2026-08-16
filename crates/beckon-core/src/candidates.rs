//! One binding, several ids: `"Google Keep || https://keep.google.com/"`.
//!
//! A binding names an app, but the *name* of an app is not the same on every
//! machine, and the user does not always get to choose it. Measured on the
//! author's laptop 2026-08-16: three Brave PWAs carry a URL where their `Name`
//! should be, because Chromium's policy force-install could not fetch the app
//! manifest for an auth-gated site and wrote a placeholder install instead.
//! Re-installing by hand fixes the name — and the policy path recreates the
//! placeholder, so the same binding is correct on one machine and dead on the
//! next.
//!
//! A chain lets one binding carry both spellings and take whichever this
//! machine can actually act on.
//!
//! # This is not an alias table
//!
//! CLAUDE.md puts *"config for the hot path / app aliases"* out of scope, and
//! that entry stands — a chain is not what it forbids. The one-line test: an
//! alias cannot be typed at a shell prompt without its table, and a chain can.
//! `beckon "Google Keep || https://keep.google.com/"` works with no file in
//! sight. An alias table has three properties and a chain has none of them:
//! indirection (the binding names a key that is not itself an id), a second
//! namespace consulted on the hot path, and growth per app into a machine-wide
//! dictionary. Every candidate here **is** a literal id, run through the same
//! unmodified resolver against the same OS metadata, with no lookup of any
//! kind. A chain is a disjunction of ids, not a name *for* an id — and it
//! rides on the binding, so it costs nothing where it is not used and can
//! never become a dictionary.
//!
//! # Why the separator is `||`
//!
//! Chosen by measuring collisions, not by taste. `||` appears in **0** of the
//! 108 `.app` display names on the author's Mac, **0** across all three of
//! their shortcut TOMLs, and cannot appear unencoded in a URL — RFC 3986
//! requires `|` percent-encoded, and URLs are exactly what the placeholder
//! names are. Two characters rather than one so that an app name carrying a
//! single pipe stays reachable.
//!
//! Rejected, each by a real installed app rather than by argument:
//!   - `::` — `https:::www.notion.so:` is an actual `.app` bundle name on that
//!     Mac (macOS swaps `/` for `:` in the placeholder's own name).
//!   - `,` — `ChatGPT: Chat, Work, Create & Code with AI` is installed.
//!
//! The cost is real and permanent: an app whose literal name contains `||` is
//! unreachable from a shortcuts TOML, the same way the eight `RESERVED` words
//! are unreachable through the bare positional. The CLI keeps an escape —
//! `beckon -- "Foo || Bar"` — because `--` already means *"this is a literal
//! id"*; a TOML has none.
//!
//! # Why splitting lives here and not in the backends
//!
//! [`split`] is called ABOVE the `Backend` trait, so `Backend::beckon(id)`
//! still receives exactly one plain candidate. That removes the sharpest
//! hazard by construction instead of by seven identical edits: the literal-id
//! fallbacks — `desktop::target_classes(None, raw_id)` on Linux, the
//! by-literal-id window scan on Windows — can never be handed a string
//! containing `||` and go looking for a window whose class is
//! `"Google Keep || https://keep.google.com/"`.

/// Everything a chain names, in the order to try them.
///
/// A string with no separator yields exactly one candidate and is the
/// overwhelmingly common case: it must behave identically to what beckon did
/// before chains existed, which is why the single-candidate path allocates
/// nothing and takes no decision.
///
/// # Errors
///
/// An empty segment (`"A || "`, `"|| B"`, `"A |||| B"`) is refused rather than
/// skipped. Skipping would make a trailing separator invisible, and the empty
/// id is already rejected at the CLI boundary for a reason CLAUDE.md records:
/// it is a substring of every `Name`, so a dotfile doing `beckon "$APP"` with
/// `$APP` unset used to launch whatever sorted first.
pub fn split(id: &str) -> Result<Vec<&str>, String> {
    // `split("||")` on a string with no separator yields one element, so the
    // common case falls out of the general one and needs no special arm.
    let parts: Vec<&str> = id.split("||").map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(format!(
            "empty candidate in `{id}` -- candidates are separated by `||` and none may be blank"
        ));
    }
    Ok(parts)
}

/// Does this id name more than one candidate?
///
/// Used to keep single-id output byte-identical: `beckon -v <one-id>` must
/// print exactly what it printed before, because `testing/linux_live_test.py`
/// greps that output for `action:` in eight live focus tests.
pub fn is_chain(id: &str) -> bool {
    id.contains("||")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_id_is_one_candidate_and_is_untouched() {
        assert_eq!(split("Claude").unwrap(), vec!["Claude"]);
        assert_eq!(split("Google Keep").unwrap(), vec!["Google Keep"]);
        assert!(!is_chain("Claude"));
    }

    #[test]
    fn a_chain_splits_and_trims() {
        assert_eq!(
            split("Google Keep || https://keep.google.com/").unwrap(),
            vec!["Google Keep", "https://keep.google.com/"]
        );
        assert!(is_chain("Google Keep || https://keep.google.com/"));
    }

    #[test]
    fn three_candidates_keep_their_order() {
        assert_eq!(
            split("Brave || brave-browser || com.brave.Browser").unwrap(),
            vec!["Brave", "brave-browser", "com.brave.Browser"]
        );
    }

    /// A single pipe is not a separator, so an app whose name carries one
    /// stays reachable. This is the whole reason the token is doubled.
    #[test]
    fn a_single_pipe_never_splits() {
        assert_eq!(split("Foo | Bar").unwrap(), vec!["Foo | Bar"]);
        assert!(!is_chain("Foo | Bar"));
    }

    #[test]
    fn an_empty_segment_is_refused_rather_than_skipped() {
        for bad in ["Claude || ", " || Claude", "A |||| B", "||"] {
            assert!(split(bad).is_err(), "should refuse {bad:?}");
        }
    }

    /// The placeholder names this feature exists for are URLs, and a URL
    /// cannot carry an unencoded `|` (RFC 3986). Pinned so nobody "simplifies"
    /// the separator to something a URL can contain.
    #[test]
    fn a_url_candidate_survives_intact() {
        assert_eq!(
            split("Gmail || https://mail.google.com/").unwrap(),
            vec!["Gmail", "https://mail.google.com/"]
        );
        // Query strings and fragments too -- neither introduces a pipe.
        assert_eq!(
            split("X || https://a.example/b?c=d&e=f#g").unwrap(),
            vec!["X", "https://a.example/b?c=d&e=f#g"]
        );
    }

    /// macOS spells the same placeholder differently -- it swaps `/` for `:`
    /// in the bundle name -- and `::` was a separator candidate until this
    /// real installed app ruled it out.
    #[test]
    fn the_macos_placeholder_spelling_is_not_split() {
        assert_eq!(
            split("https:::www.notion.so:").unwrap(),
            vec!["https:::www.notion.so:"]
        );
    }

    /// A comma was the other separator candidate, and this app is installed.
    #[test]
    fn a_comma_in_a_real_app_name_is_not_a_separator() {
        let name = "ChatGPT: Chat, Work, Create & Code with AI";
        assert_eq!(split(name).unwrap(), vec![name]);
    }
}
