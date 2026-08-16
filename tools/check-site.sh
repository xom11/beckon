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

# The other half of spec item 5: the letter->app pairing. Read the rows out of
# README's own table and require the page to carry the same pairing, so
# re-binding a letter in examples/ cannot leave the page teaching the old one.
#
# THE PAGE NO LONGER PRINTS A LETTER TABLE, so this no longer looks for one. It
# used to grep for a <kbd> row in #config, which was a third listing of
# something the docks and the cheat sheet already showed twice, and it was
# deleted with the section's other redundancies. The GUARANTEE did not move
# with it: the pairing still lives in exactly two places, and both are checked
# below. Do not weaken this to one — the two can drift apart, and if they do the
# icon a reader presses and the sheet telling them to press it disagree.
key_fail=0
keys=0
while IFS='|' read -r _ letter app _; do
  letter=$(printf '%s' "$letter" | tr -d ' `')
  app=$(printf '%s' "$app" | sed 's/^ *//;s/ *$//')
  [ -z "$letter" ] && continue
  keys=$((keys + 1))
  # 1. The dock prints the letter ON the icon — the one place where the key and
  #    the thing it reaches are the same object. Both desks carry a full dock,
  #    so both have to agree: hence -c and the count, not a bare grep.
  n=$(grep -cF "data-app=\"$app\" data-key=\"$letter\"" "$H" || true)
  [ "$n" -eq 2 ] \
    || { bad "dock icon for $app does not print $letter on both desks (found $n of 2)"; key_fail=1; }
  # 2. DESK_APPS in desk.js, which is what fills the cheat sheet pinned to the
  #    hero and what deskAppOf() resolves a keypress through. A letter that is
  #    right in the markup and wrong here is a key that prints one app and
  #    beckons another.
  grep -qF "key: '$letter', name: '$app'" "$M" \
    || { bad "DESK_APPS in desk.js does not map $letter to $app"; key_fail=1; }
done < <(awk '/^\| Letter \| App \|/{f=1;next} f&&/^\|---/{next} f&&/^\|/{print} f&&!/^\|/{exit}' README.md)
if [ "$keys" -eq 0 ]; then
  bad "could not find README's letter->app table"
elif [ "$key_fail" -eq 0 ]; then
  ok "letter->app pairing matches README in the docks and desk.js ($keys rows)"
fi

# The five branch names. They replaced the step numbers (4, 5, 5a-5c) that the
# table used to print, and they now exist in two places that must agree: the
# table in #how, and DESK_STEP_NAMES in desk.js, which is what the readout under
# the desk prints when that branch fires. A reader who presses a key sees the
# readout name and looks for the row with the same word; if these drift, that
# stops working and nothing else would notice.
name_fail=0
for n in Launch Focus Cycle Back Hide; do
  grep -q "class=\"how-do\">$n<" "$H" \
    || { bad "the #how table does not name the '$n' branch"; name_fail=1; }
  grep -q "'$n'" "$M" \
    || { bad "desk.js has no '$n' in DESK_STEP_NAMES"; name_fail=1; }
done
# And the numbers must not come back as a visible column.
if grep -q 'class="how-step"' "$H"; then
  bad "the #how table is printing step numbers again"
  name_fail=1
fi
[ "$name_fail" -eq 0 ] && ok "the five branches are named, in the table and in desk.js"

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

# --- 7. all three machines are still reachable -----------------------------
# The hero draws ONE desk, the reader's, so the claim "the same key on every OS
# you use" now rests on two things: chrome the CSS can draw for each of the
# three, and a chord row per OS in the markup — the row set a JS-off reader
# keeps in full, since they have no OS strip to press. Lose either half and the
# headline is promising something the page no longer shows.
desk_fail=0
for os in macos windows linux; do
  grep -q "\.desk\[data-os=\"$os\"\]" "$C" \
    || { bad "beckon.css draws no chrome for $os"; desk_fail=1; }
  grep -q "class=\"os-row\" data-os=\"$os\"" "$H" \
    || { bad "the hero has no $os chord row"; desk_fail=1; }
done
grep -q 'class="mods hero-chords"' "$H" \
  || { bad "the hero chord rows are gone"; desk_fail=1; }
[ "$desk_fail" -eq 0 ] && ok "all three machines are drawable and named"

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
hero-os
how-readout
hud
CTLS
# `how-press` was on this list until the #how section stopped carrying a press
# row of its own. It is not hidden — it does not exist, and the element must not
# come back without this line coming back with it.
if grep -q 'id="how-press"' "$H"; then
  bad "#how-press is back in the markup — add it to the CTLS list above"
  ctl_fail=1
fi
[ "$ctl_fail" -eq 0 ] && ok "every JS-only control ships hidden"

# --- 8b. and none of them is announced as nothing at all -------------------
# The same six ids, asked a second question. `hidden` is about a control that
# would be inert; this is about one that WORKS and is unreachable: every id in
# CTLS is filled by beckon.js with real buttons, and a focusable control inside
# an `aria-hidden` subtree is a defect — the browser hands it to Tab and hands
# the screen reader nothing to say about it.
#
# This is a REGRESSION guard, not a hypothetical. `#hero-os` shipped under
# `<div class="desks" aria-hidden="true">`: the picture below it is what has
# nothing to announce, so the attribute belongs on the slot, and with it one
# level up all three OS buttons were reachable and nameless. Nothing else in
# this file could see it — the id was present, it shipped `hidden`, check 8 was
# green, and the page looked right.
#
# Why a parser and not a grep: the answer is a question about ANCESTRY, and
# `site/index.html` now carries two `.desks` grids — the hero's (line ~205,
# attribute moved down onto `.desk-slot`) and #how's (line ~422, still hidden
# whole, correctly, because it contains no control). A line-based check cannot
# tell those apart, and an indentation-based one is guessing. So: strip
# comments to newlines (line numbers survive, and the prose in them says
# "aria-hidden" often enough to poison any regex over the raw file), walk the
# tags keeping a stack, and report an id whose ancestors — or whose own tag —
# carry the attribute.
#
# Skipped rather than failed without node, like check 6, and for the same
# reason: this is a Rust repository, and CI has node.
if command -v node >/dev/null 2>&1; then
  if a11y=$(node - "$H" os-switch theme hero-press hero-os how-readout hud <<'NODE'
const fs = require('fs');
const [file, ...ids] = process.argv.slice(2);
// Blank the comments but keep their newlines, so reported lines match the file.
const src = fs.readFileSync(file, 'utf8')
  .replace(/<!--[\s\S]*?-->/g, c => c.replace(/[^\n]/g, ''));
const VOID = new Set(['area','base','br','col','embed','hr','img','input',
                      'link','meta','param','source','track','wbr']);
const tag = /<(\/?)([a-zA-Z][\w:.-]*)((?:"[^"]*"|'[^']*'|[^>"'])*?)(\/?)>/g;
const stack = [], bad = [];
let m;
while ((m = tag.exec(src)) !== null) {
  const [, slash, rawName, attrs, selfClose] = m;
  const name = rawName.toLowerCase();
  if (slash) {
    // Pop to the matching open tag, tolerating anything left unclosed inside.
    for (let i = stack.length - 1; i >= 0; i--) {
      if (stack[i].name === name) { stack.length = i; break; }
    }
    continue;
  }
  const hidden = /\baria-hidden\s*=\s*"true"/.test(attrs);
  const id = (/\bid\s*=\s*"([^"]*)"/.exec(attrs) || [])[1];
  if (id && ids.includes(id)) {
    const owner = stack.find(f => f.hidden) || (hidden ? { name, line: null } : null);
    if (owner) {
      const line = src.slice(0, m.index).split('\n').length;
      bad.push(`#${id} (line ${line}) is inside <${owner.name}` +
               (owner.line ? ` line ${owner.line}` : ' (itself)') + ' aria-hidden="true">');
    }
  }
  if (!selfClose && !VOID.has(name)) {
    stack.push({ name, hidden, line: src.slice(0, m.index).split('\n').length });
  }
}
if (bad.length) { console.log(bad.join('\n')); process.exit(1); }
NODE
  ); then
    ok "no JS-only control sits under an aria-hidden ancestor"
  else
    bad "a JS-only control is hidden from assistive tech:"
    printf '%s\n' "$a11y" | sed 's/^/       /'
  fi
else
  skip "node not installed, aria-hidden ancestry not checked"
fi

if grep -qE 'class="panel"[^>]*hidden' "$H"; then
  bad "an install panel ships hidden — JS-off readers lose it"
else
  ok "all four install panels ship visible"
fi

# The guard above is a regex over `class="panel"`, so it stops being able to
# match the moment a panel's class list grows — `class="panel is-active"` makes
# it vacuous and it reports green whatever the markup does. Counting the bare
# form is what keeps the check honest.
n=$(grep -c 'class="panel"' "$H" || true)
if [ "$n" -eq 4 ]; then
  ok "all four panels are still bare class=\"panel\" (check 8 can see them)"
else
  bad "expected 4 bare class=\"panel\" attributes, found $n — check 8's guard is now vacuous"
fi

# --- 9. the skins are actually three skins ---------------------------------
# The token audit in check 2 proves a NAME exists on bare :root. It cannot see a
# forgotten override — and a form token that is never restated for Windows and
# Linux silently serves them the macOS value, so the page's whole "it wears your
# machine" thesis is refuted by the page itself with every other check green.
#
# beckon.css marks the OS-varying tokens with @os-parity begin/end. Every one of
# them has to appear in all three :root[data-os="…"] blocks in §1c.
parity=$(awk '/@os-parity begin/,/@os-parity end/' "$C" | grep -oE '^\s+--[a-zA-Z0-9-]+' | tr -d ' ')
if [ -z "$parity" ]; then
  bad "no @os-parity block in beckon.css — the skin tokens are unguarded"
else
  parity_fail=0
  for os in macos windows linux; do
    block=$(awk -v pat=":root\\[data-os=\"$os\"\\], .door\\[data-os=\"$os\"\\] \\{" \
              'index($0, "@os-parity") { next }
               $0 ~ /^:root\[data-os=/ && index($0, "\"'"$os"'\"") { f=1 }
               f { print }
               f && /^\}/ { exit }' "$C")
    for t in $parity; do
      printf '%s\n' "$block" | grep -q -- "$t:" \
        || { bad "$t is not overridden for $os — that skin silently gets the macOS value"; parity_fail=1; }
    done
  done
  [ "$parity_fail" -eq 0 ] \
    && ok "every @os-parity token is overridden in all three skins ($(printf '%s\n' "$parity" | wc -l | tr -d ' ') tokens)"
fi

printf '\n'
if [ "$fail" -eq 0 ]; then printf 'all checks passed\n'; else printf 'checks FAILED\n'; fi
exit "$fail"
