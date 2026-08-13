#!/usr/bin/env bash
# The landing page's test suite. Checks 1-5 are the five "verifiable without a
# browser" items in
# docs/superpowers/specs/2026-08-12-github-pages-landing-design.md; checks 6-8
# were added by
# docs/superpowers/specs/2026-08-13-landing-redesign-design.md.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
ok()  { printf '  ok   %s\n' "$1"; }
bad() { printf ' FAIL  %s\n' "$1"; fail=1; }
skip() { printf ' skip  %s\n' "$1"; }

H=site/index.html
C=site/beckon.css
J=site/beckon.js
M=site/desk.js
T=site/desk.test.mjs

for f in "$H" "$C" "$J" "$M" "$T"; do
  [ -f "$f" ] || { bad "missing $f"; }
done
[ "$fail" -eq 1 ] && { printf '\n%s\n' "site/ is not built yet"; exit 1; }

# --- 1. no external subresources -------------------------------------------
# <a href="https://..."> is fine and expected. Anything the browser FETCHES
# is not: it leaks the reader to a third party and breaks file:// and offline.
if grep -nEi '<(link|script|img|video|source|iframe|embed|object)[^>]+(src|href|data)="https?://' "$H"; then
  bad "external subresource in index.html (above)"
else
  ok "no external subresources in HTML"
fi
if grep -nEi "url\(\s*[\"']?https?://" "$C"; then
  bad "external url() in beckon.css (above)"
else
  ok "no external url() in CSS"
fi
if grep -nEi '(fetch|XMLHttpRequest|import)\s*\(\s*["'"'"']https?://' "$J" "$M"; then
  bad "network call in JS (above)"
else
  ok "no network calls in JS"
fi

# --- 2. token audit ---------------------------------------------------------
# A token defined ONLY inside @media or [data-theme] renders unstyled for one
# of the three theme states (light / dark / explicit-toggle).
used=$(grep -oE 'var\(--[a-zA-Z0-9-]+' "$C" | sed 's/var(//' | sort -u)
defined=$(awk '/^:root \{/,/^\}/' "$C" | grep -oE -- '--[a-zA-Z0-9-]+' | sort -u)
missing=$(comm -23 <(printf '%s\n' "$used") <(printf '%s\n' "$defined"))
if [ -n "$missing" ]; then
  bad "token(s) used but not defined on bare :root: $(echo "$missing" | tr '\n' ' ')"
else
  ok "every token defined on bare :root"
fi

# --- 3. link check ----------------------------------------------------------
anchor_fail=0
for a in $(grep -oE 'href="#[a-zA-Z0-9_-]+"' "$H" | sed 's/href="#//;s/"//' | sort -u); do
  grep -q "id=\"$a\"" "$H" || { bad "anchor #$a has no target"; anchor_fail=1; }
done
[ "$anchor_fail" -eq 0 ] && ok "every in-page anchor resolves"

path_fail=0
for p in $(grep -oE 'github\.com/xom11/beckon/(blob|tree)/main/[A-Za-z0-9_./-]+' "$H" \
           | sed 's|.*/main/||' | sed 's|/$||' | sort -u); do
  [ -e "$p" ] || { bad "links to a repo path that does not exist: $p"; path_fail=1; }
done
[ "$path_fail" -eq 0 ] && ok "every repo link points at a real path"

# --- 4. reduced motion ------------------------------------------------------
# The page's argument IS the animation, so turning motion off must land on the
# final frame, not on nothing.
#
# `animation-fill-mode: forwards` is checked because the other two are NOT
# sufficient and this check used to say they were. Measured under Chrome's
# reduced-motion emulation: a 1ms single-iteration animation runs and then
# returns the element to its base style, which is the OPENING frame — so the
# page froze on the one picture that contradicts its own caption while this
# check reported ok. All three, or the guarantee is not a guarantee.
if grep -q 'prefers-reduced-motion: reduce' "$C" \
   && awk '/prefers-reduced-motion: reduce/,/^\}/' "$C" | grep -q 'animation-duration' \
   && awk '/prefers-reduced-motion: reduce/,/^\}/' "$C" | grep -q 'animation-iteration-count: 1' \
   && awk '/prefers-reduced-motion: reduce/,/^\}/' "$C" | grep -q 'animation-fill-mode: forwards'; then
  ok "reduced-motion block pins animations to their final frame"
else
  bad "reduced-motion block missing, or it does not pin duration + iteration-count + fill-mode"
fi
# Per demo, not "at least one of each": the old form counted any line whose
# class attribute contained the substring "demo" (so .how-demos and .demo-cap
# both scored) and passed on `> 0`, which three demos and one transcript also
# does — while the failure message claimed to name a demo that had none.
# `class="demo"` / `class="demo …"` matches the token exactly.
demos=$(grep -cE 'class="demo( |")' "$H" || true)
steps=$(grep -c 'class="demo-steps"' "$H" || true)
if [ "$demos" -gt 0 ] && [ "$demos" -eq "$steps" ]; then
  ok "every demo carries a text transcript ($demos demos, $steps transcripts)"
else
  bad "a demo has no .demo-steps text transcript ($demos demos, $steps transcripts)"
fi

# --- 5. claim check ---------------------------------------------------------
ver=$(grep -m1 '^version' Cargo.toml | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if grep -qF "$ver" "$H"; then
  ok "page states the current version ($ver)"
else
  bad "Cargo.toml says $ver, page does not mention it"
fi

# Bidirectional: if README's install line changes, this fails and forces the
# page to follow. That is the point — a stale install command is the one
# error a landing page cannot afford.
cmd_fail=0
while IFS= read -r cmd; do
  [ -z "$cmd" ] && continue
  grep -qF -- "$cmd" README.md || { bad "README no longer contains: $cmd"; cmd_fail=1; }
  grep -qF -- "$cmd" "$H"      || { bad "page is missing install command: $cmd"; cmd_fail=1; }
done <<'CMDS'
brew install xom11/tap/beckon
scoop bucket add xom11 https://github.com/xom11/scoop-bucket
scoop install xom11/beckon
cargo install --git https://github.com/xom11/beckon
nix run github:xom11/beckon -- list
CMDS
[ "$cmd_fail" -eq 0 ] && ok "install commands match README byte for byte"

# The other half of spec item 5, and it was missing: the letter->app table.
# Read the rows out of README's own table and require the page to carry the
# same pairing, so re-binding a letter in examples/ cannot leave the page
# teaching the old one.
key_fail=0
keys=0
while IFS='|' read -r _ letter app _; do
  letter=$(printf '%s' "$letter" | tr -d ' `')
  app=$(printf '%s' "$app" | sed 's/^ *//;s/ *$//')
  [ -z "$letter" ] && continue
  keys=$((keys + 1))
  grep -qF "<kbd class=\"key\">$letter</kbd></th><td>$app</td>" "$H" \
    || { bad "letter table drifted from README: $letter -> $app"; key_fail=1; }
done < <(awk '/^\| Letter \| App \|/{f=1;next} f&&/^\|---/{next} f&&/^\|/{print} f&&!/^\|/{exit}' README.md)
if [ "$keys" -eq 0 ]; then
  bad "could not find README's letter->app table"
elif [ "$key_fail" -eq 0 ]; then
  ok "letter->app table matches README ($keys rows)"
fi

# --- 6. the algorithm the page draws is the algorithm it describes ----------
# site/desk.js is the page's only copy of beckon's focus algorithm, and it is
# pure precisely so this can run. Before it existed, "press it again and it
# cycles" was a sentence beside an animation and nothing could check either.
#
# Skipped rather than failed when node is absent: this is a Rust repository and
# a contributor is not required to have a JS runtime. CI does — GitHub's
# ubuntu-latest ships node — so the check is enforced where it counts.
if command -v node >/dev/null 2>&1; then
  if node --test "$T" >/tmp/beckon-desk-test.$$ 2>&1; then
    # node --test prefixes its summary with a multibyte glyph, so this matches
    # on the word rather than on a column.
    ok "desk model passes $(grep -oE '^[^0-9]*pass [0-9]+' /tmp/beckon-desk-test.$$ | grep -oE '[0-9]+') tests"
  else
    bad "desk model tests failed:"
    sed 's/^/       /' /tmp/beckon-desk-test.$$
  fi
  rm -f /tmp/beckon-desk-test.$$
else
  skip "node not installed, desk model tests not run"
fi

# --- 7. the hero is three machines, not one -------------------------------
# The claim the hero exists to make is "the same letter on every OS you use".
# It is made by drawing three different window chromes at once, so losing one
# of them to a refactor would quietly delete the argument while leaving the
# headline that promises it.
desk_fail=0
for os in macos windows linux; do
  grep -qF "<div class=\"desk\" data-os=\"$os\"" "$H" \
    || { bad "the hero has no $os desk"; desk_fail=1; }
done
[ "$desk_fail" -eq 0 ] && ok "the hero draws all three desks"

# --- 8. no control on screen that silently does nothing --------------------
# Every one of these is inert without JS, so every one of them must ship with
# the `hidden` attribute for beckon.js to remove. The install PANELS are the
# mirror image and must NOT ship hidden — a reader with JS off needs all four.
ctl_fail=0
while IFS= read -r id; do
  [ -z "$id" ] && continue
  grep -qE "id=\"$id\"[^>]*hidden|hidden[^>]*id=\"$id\"" "$H" \
    || { bad "control #$id does not ship hidden"; ctl_fail=1; }
done <<'CTLS'
os-switch
theme
hero-press
how-press
how-readout
hud
CTLS
[ "$ctl_fail" -eq 0 ] && ok "every JS-only control ships hidden"

if grep -qE 'class="panel"[^>]*hidden' "$H"; then
  bad "an install panel ships hidden — JS-off readers lose it"
else
  ok "all four install panels ship visible"
fi

printf '\n'
if [ "$fail" -eq 0 ]; then printf 'all checks passed\n'; else printf 'checks FAILED\n'; fi
exit "$fail"
