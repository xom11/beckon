#!/usr/bin/env bash
# The landing page's test suite. Every check here is one of the five
# "verifiable without a browser" items in
# docs/superpowers/specs/2026-08-12-github-pages-landing-design.md.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
ok()  { printf '  ok   %s\n' "$1"; }
bad() { printf ' FAIL  %s\n' "$1"; fail=1; }

H=site/index.html
C=site/beckon.css
J=site/beckon.js

for f in "$H" "$C" "$J"; do
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
if grep -nEi '(fetch|XMLHttpRequest|import)\s*\(\s*["'"'"']https?://' "$J"; then
  bad "network call in beckon.js (above)"
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
if grep -q 'prefers-reduced-motion: reduce' "$C" \
   && awk '/prefers-reduced-motion: reduce/,/^\}/' "$C" | grep -q 'animation-duration' \
   && awk '/prefers-reduced-motion: reduce/,/^\}/' "$C" | grep -q 'animation-iteration-count: 1'; then
  ok "reduced-motion block pins animations to their final frame"
else
  bad "no reduced-motion block, or it does not pin animation-duration + iteration-count"
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

printf '\n'
if [ "$fail" -eq 0 ]; then printf 'all checks passed\n'; else printf 'checks FAILED\n'; fi
exit "$fail"
