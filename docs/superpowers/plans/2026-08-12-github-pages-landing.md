# beckon Landing Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a single static landing page at `xom11.github.io/beckon` that sells beckon's one unanswerable claim — the same `beckon Claude` on macOS, Windows and Linux — deployed from `site/` by GitHub Actions.

**Architecture:** Three hand-written text files (`index.html`, `beckon.css`, `beckon.js`) plus two PNGs derived from `assets/beckon.ico`. No build step, no package manager, no framework, no external request. A shell script, `tools/check-site.sh`, is the test suite: it enforces the spec's five automated checks and is written **first**, so every later task has a failing gate to turn green.

**Tech Stack:** HTML5, CSS custom properties + `@keyframes`, ~60 lines of vanilla JS, `bash` + `grep`/`awk` for the checker, GitHub Actions (`configure-pages` / `upload-pages-artifact` / `deploy-pages`).

**Spec:** `docs/superpowers/specs/2026-08-12-github-pages-landing-design.md`. Read it before Task 1. Where this plan and the spec disagree, the spec wins and the plan is the bug.

## Global Constraints

Every task's requirements implicitly include all of these.

- **No external network requests.** No CDN, no webfont, no analytics, no third-party embed. Everything the page loads is one of its own five files.
- **No build step.** Nothing that requires `npm`, a bundler, a preprocessor, or a lockfile. The page must open correctly from `file://`.
- **Site lives in `site/`, never `docs/`.** `docs/` holds internal `superpowers/specs`, `plans` and `measurements`; Pages served from `/docs` would publish them as an indexable website.
- **Every claim traces to the repo.** Install commands byte-match `README.md`; version comes from `Cargo.toml` (`0.8.0`); the environment grid matches `examples/` plus the dispatch table in `CLAUDE.md`; the letter→app table matches `README.md`.
- **Accent `#2563EB`** (measured from `assets/beckon.ico`). Dark theme lifts it to `#5B8CFF` — `#2563EB` on `#0B0D12` is 3.8:1 and fails WCAG AA for body text; `#5B8CFF` is 6.1:1.
- **Full light palette defined on bare `:root`.** Only differing tokens are redefined under `@media (prefers-color-scheme: dark)` and again under `:root[data-theme="dark"]`. `body` always gets an explicit background token.
- **Progressive enhancement.** Every section readable and every link working with JS disabled. Panels ship visible; JS hides them.
- **No benchmark number on the hero.** 57 ms / ~95 ms may appear in the FAQ only, each stated with the machine it was measured on.
- **No testimonials, no pricing.** Not "not yet written" — deliberately absent. Do not invent either.
- **ASCII-safe copy in code blocks.** Install commands are copy-pasted by readers; no smart quotes, no en-dashes inside `<code>`.
- After each task, `tools/check-site.sh` must exit 0 (from Task 2 onward).

---

### Task 1: The checker script — the page's test suite

The whole plan is TDD'd against this one script. It is written before any
markup exists and is expected to fail loudly on an empty repo.

**Files:**
- Create: `tools/check-site.sh`
- Modify: `.github/workflows/ci.yml` (append one job)

**Interfaces:**
- Consumes: nothing.
- Produces: `tools/check-site.sh`, exit 0 = all checks pass, exit 1 = at least
  one failed. Every later task runs it as its test step.

- [ ] **Step 1: Write the checker**

Create `tools/check-site.sh`:

```bash
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
demos=$(grep -c 'class="[^"]*demo' "$H" || true)
steps=$(grep -c 'demo-steps' "$H" || true)
if [ "$demos" -gt 0 ] && [ "$steps" -gt 0 ]; then
  ok "demos carry a text transcript (.demo-steps)"
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

printf '\n'
if [ "$fail" -eq 0 ]; then printf 'all checks passed\n'; else printf 'checks FAILED\n'; fi
exit "$fail"
```

Then `chmod +x tools/check-site.sh`.

- [ ] **Step 2: Run it to verify it fails**

```bash
./tools/check-site.sh
```

Expected: `FAIL  missing site/index.html` (and the css/js lines), then
`site/ is not built yet`, exit 1.

Confirm the exit code, because a script that prints failures and still exits 0
is the failure mode that makes every later task's gate meaningless:

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected: `exit=1`.

- [ ] **Step 3: Wire it into CI**

Append to `.github/workflows/ci.yml`, at the same indentation as the existing
entries under `jobs:`:

```yaml
  site:
    name: Landing page checks
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: ./tools/check-site.sh
```

- [ ] **Step 4: Verify the YAML parses**

```bash
python3 -c "import yaml,sys; d=yaml.safe_load(open('.github/workflows/ci.yml')); print(sorted(d['jobs']))"
```

Expected: the existing job names plus `site`. If `yaml` is not installed, use
`python3 -c "import json,sys" ` and instead confirm by eye that the new block
sits under `jobs:` at the same indent as its siblings.

- [ ] **Step 5: Commit**

```bash
git add tools/check-site.sh .github/workflows/ci.yml
git commit -m "test: add the landing page's check suite, before the page"
```

---

### Task 2: Deploy pipeline and the page shell

Ends with a real page on a real URL. Everything after this is content.

**Files:**
- Create: `.github/workflows/pages.yml`, `site/index.html`, `site/beckon.css`,
  `site/beckon.js`, `site/favicon.png`, `site/icon-512.png`

**Interfaces:**
- Consumes: `tools/check-site.sh` from Task 1.
- Produces: the token set — `--bg`, `--bg-raised`, `--fg`, `--fg-dim`,
  `--rule`, `--accent`, `--accent-fg`, `--radius`, `--maxw`, `--sans`,
  `--mono`; every one of them defined on bare `:root` — the `.wrap` container, the
  `<header class="nav">`/`<footer>` shell, and `site/beckon.js`'s theme
  toggle. Every later task appends `<section>` elements between them.

- [ ] **Step 1: Derive the icons**

```bash
mkdir -p site
sips -s format png -Z 64  assets/beckon.ico --out site/favicon.png
sips -s format png -Z 512 assets/beckon.ico --out site/icon-512.png
sips -g pixelWidth site/favicon.png site/icon-512.png
```

Expected: 64 and 512. `assets/beckon.ico` stays the single source of the mark;
these two are regenerated by the same commands if it changes.

- [ ] **Step 2: Write `site/beckon.css` — tokens and shell only**

```css
/* beckon — landing page. No build step: this file is served as written. */

:root {
  --bg:         #FFFFFF;
  --bg-raised:  #F6F7F9;
  --fg:         #0B0D12;
  --fg-dim:     #5A6072;
  --rule:       #E3E6EC;
  --accent:     #2563EB;   /* measured from assets/beckon.ico */
  --accent-fg:  #FFFFFF;
  --radius:     10px;
  --maxw:       1080px;
  --sans: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
          "Helvetica Neue", Arial, sans-serif;
  --mono: ui-monospace, SFMono-Regular, "SF Mono", "Cascadia Mono", Menlo,
          Consolas, "Liberation Mono", monospace;
}

/* Only the tokens that differ are restated. Twice: once for the system
   preference, once for the explicit toggle, so the toggle wins both ways. */
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #0B0D12; --bg-raised: #141822; --fg: #E8EAF0;
    --fg-dim: #9AA1B4; --rule: #232936;
    --accent: #5B8CFF;  /* #2563EB is 3.8:1 here and fails AA */
    --accent-fg: #0B0D12;
  }
}
:root[data-theme="dark"] {
  --bg: #0B0D12; --bg-raised: #141822; --fg: #E8EAF0;
  --fg-dim: #9AA1B4; --rule: #232936;
  --accent: #5B8CFF; --accent-fg: #0B0D12;
}

* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body {
  margin: 0;
  background: var(--bg);       /* explicit, never transparent */
  color: var(--fg);
  font-family: var(--sans);
  font-size: 17px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
}
.wrap { max-width: var(--maxw); margin-inline: auto; padding-inline: 24px; }
a { color: var(--accent); }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }

.nav {
  position: sticky; top: 0; z-index: 10;
  background: color-mix(in srgb, var(--bg) 88%, transparent);
  backdrop-filter: blur(8px);
  border-bottom: 1px solid var(--rule);
}
.nav .wrap { display: flex; align-items: center; gap: 24px; height: 60px; }
.nav a { color: var(--fg-dim); text-decoration: none; font-size: 15px; }
.nav a:hover { color: var(--fg); }
.brand {
  display: flex; align-items: center; gap: 9px;
  font-weight: 650; color: var(--fg) !important; margin-inline-end: auto;
}
.brand img { width: 22px; height: 22px; border-radius: 5px; }

footer { border-top: 1px solid var(--rule); margin-top: 96px; padding-block: 40px; }
footer .wrap { display: flex; flex-wrap: wrap; gap: 16px; color: var(--fg-dim); font-size: 14px; }

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: .01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: .01ms !important;
    scroll-behavior: auto !important;
  }
}
```

Note what the reduced-motion block does: `animation-duration: .01ms` with
`iteration-count: 1` lands every demo on its **final keyframe** and holds it.
That is exactly the spec's requirement, and it is why every demo in Tasks 3–5
must be authored so its last keyframe is the state worth seeing.

- [ ] **Step 3: Write `site/index.html` — shell with nav and footer**

`<html lang="en">`, `<meta charset>`, `<meta name="viewport" content="width=device-width, initial-scale=1">`.

`<title>beckon — one key per app, on every OS</title>`

Meta: `description`, `og:title`, `og:description`, `og:type=website`,
`og:image` → `icon-512.png`, `twitter:card=summary`. Favicon → `favicon.png`.

Body order: `<header class="nav">` (brand + `Install` `How it works` `Docs`
`GitHub` + `<button id="theme">`), `<main>` (empty for now), `<footer>`
carrying `MIT OR Apache-2.0`, `v0.8.0`, and links to the repo and releases.

The version string must be the literal `0.8.0` so check 5 finds it.

Repo links use the `https://github.com/xom11/beckon/blob/main/<path>` form so
check 3 can verify the path exists on disk.

- [ ] **Step 4: Write `site/beckon.js` — theme toggle only**

```js
// Progressive enhancement only. Everything below is optional: with JS off the
// page still reads, every link works, and the theme follows the OS.
(() => {
  const root = document.documentElement;
  const btn = document.getElementById('theme');
  if (!btn) return;
  const saved = localStorage.getItem('beckon-theme');
  if (saved) root.dataset.theme = saved;
  const label = () =>
    (btn.setAttribute('aria-label',
      'Switch to ' + (root.dataset.theme === 'dark' ? 'light' : 'dark') + ' theme'));
  label();
  btn.addEventListener('click', () => {
    const dark = root.dataset.theme
      ? root.dataset.theme === 'dark'
      : matchMedia('(prefers-color-scheme: dark)').matches;
    root.dataset.theme = dark ? 'light' : 'dark';
    localStorage.setItem('beckon-theme', root.dataset.theme);
    label();
  });
})();
```

- [ ] **Step 5: Write `.github/workflows/pages.yml`**

```yaml
name: Pages

on:
  push:
    branches: [main]
    paths: ['site/**', '.github/workflows/pages.yml']
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  deploy:
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deploy.outputs.page_url }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/configure-pages@v5
      - uses: actions/upload-pages-artifact@v3
        with:
          path: site
      - id: deploy
        uses: actions/deploy-pages@v4
```

The `paths:` filter keeps a Rust-only commit from redeploying.
`workflow_dispatch` is there because this repo has already been bitten by a
workflow that never fired on its own — see *Distribution* in `CLAUDE.md`.

- [ ] **Step 6: Run the checker**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`. Checks 1–4 pass trivially on a shell with no animations —
except the `.demo-steps` half of check 4 and the install-command half of check
5, which **will fail** because no demo and no install block exists yet.

That is correct and expected at this task. To keep the gate meaningful without
weakening it, add the Install section's four command blocks now, in
`<main>`, as a plain `<section id="install">` with four `<pre><code>` blocks
carrying the five exact command lines from the checker's heredoc, plus one
demo placeholder `<div class="demo"><p class="demo-steps">…</p></div>`. Task 8
restyles that section; it does not re-author the commands.

Re-run: expected `exit=0`.

- [ ] **Step 7: Commit**

```bash
git add site .github/workflows/pages.yml
git commit -m "feat(site): deploy pipeline and page shell"
```

- [ ] **Step 8: Note the one manual step**

Repo Settings → Pages → Source must be **GitHub Actions**. Left at *Deploy from
a branch*, this workflow goes green and publishes nothing. This cannot be done
from the plan; it is listed again in Task 10 so it is not lost.

---

### Task 3: The keycap component, the hero, and the hero demo

**Files:**
- Modify: `site/index.html` (add `<section class="hero">` as the first child of
  `<main>`), `site/beckon.css` (append)

**Interfaces:**
- Consumes: tokens and `.wrap` from Task 2.
- Produces: `.key` — reused verbatim by Tasks 4, 6 and 7. Do not fork it.
  `.key[data-down]` is the pressed state. Also produces `.demo` and
  `.demo-steps`, which every later demo reuses.

- [ ] **Step 1: The keycap**

```css
.key {
  display: inline-flex; align-items: center; justify-content: center;
  min-width: 2.1em; padding: .3em .55em;
  font: 600 .82em/1 var(--mono);
  color: var(--fg);
  background: var(--bg-raised);
  border: 1px solid var(--rule);
  border-radius: 6px;
  box-shadow: 0 2px 0 var(--rule);
  transform: translateY(0);
}
.key[data-down] { box-shadow: 0 1px 0 var(--rule); transform: translateY(1px); }
.chord { display: inline-flex; align-items: center; gap: 6px; }
.chord .plus { color: var(--fg-dim); font-size: .8em; }
```

- [ ] **Step 2: The hero copy**

Exactly this, no rewriting:

- `<h1>` — **One key per app.<br>Every OS you use.**
- Lead — *`beckon Claude` is the same line on macOS, Windows and Linux. It
  resolves against the OS's own metadata, so there is no id to keep in sync
  between machines.*
- Buttons: `Install` → `#install` (filled, `--accent`), `GitHub` → the repo
  (outline).
- Meta line: `MIT OR Apache-2.0 · v0.8.0 · Rust, no runtime`.

`h1 { font-size: clamp(2.75rem, 7vw, 5.5rem); letter-spacing: -0.03em; line-height: 1.02; }`

- [ ] **Step 3: The hero demo**

> **CORRECTED 2026-08-12, after this step shipped.** This step used to
> prescribe ONE `.chord` of `Ctrl` `Win` `Alt` `+` `C` above the three cards.
> That chord is the **Windows** default. `README.md:216` — *"Modifier defaults:
> `Super` on Linux, Hyper (`cmd+ctrl+alt`) on macOS, `Ctrl+Win+Alt` on
> Windows"* — so the page claimed a Windows chord was pressed on a Mac and on
> a Linux box. Two of the three cards were false. The agent that built it
> flagged the contradiction rather than shipping it silently; the prescription
> below is the fixed one. Do not collapse the three chords back into one.

Markup: three `.os-card` elements labelled `macOS`, `Windows`, `Linux`, each
containing its **own** `.chord` and then two stacked fake window bars —
`Brave` and `Claude`. Only the letter is shared, and that is the actual claim:

| Card | Chord |
|---|---|
| macOS | `Cmd` `Ctrl` `Alt` `+` `C` |
| Windows | `Ctrl` `Win` `Alt` `+` `C` |
| Linux | `Super` `+` `C` |

Spell the modifiers. Do **not** use the glyphs `(Cmd)(Ctrl)(Alt)`: a reader
who does not already know them learns nothing, and they read badly in the
transcript.

Animation, one shared 4.4s timeline, `infinite`:
1. `0–18%` — resting.
2. `18–26%` — every `.key` in all three chords takes the pressed look.
3. `26–46%` — in all three cards at once, the `Claude` bar raises above
   `Brave` (z-index + a 6px translate) and takes `--accent` for its title dot.
4. `46–100%` — hold.

Simultaneity across the three cards is the whole argument and it survives the
correction: declare **no** `animation-delay` anywhere in the demo, so every
cap and every card is phase-locked by construction. Different key, same
instant, same result.

`.key` and `.chord` are shared components — do not fork or restyle them.
Scope size and wrapping to the hero (`.os-chord`). The chord must be allowed
to WRAP: three cards at a third of a 375px viewport cannot hold four
spelled-out modifier caps on one line at any legible size. Pair that with
`margin-block-start: auto` on `.os-stack` so the window stacks stay level
across cards whatever each chord wrapped to.

**The final keyframe must be state 4** — the focused state — because
reduced-motion lands there and holds.

- [ ] **Step 4: The transcript**

> **CORRECTED 2026-08-12.** The old transcript read *"Ctrl+Win+Alt+C, pressed
> once on each machine: Claude comes to the front on all three."* It named one
> chord for three machines, which `README.md:216` contradicts (see Step 3).

```html
<p class="demo-steps">One line &mdash; beckon Claude &mdash; bound to each machine's own modifier. Pressed once on all three: Claude comes to the front on all three.</p>
```

`.demo-steps { color: var(--fg-dim); font-size: 14px; }` — visible to
everyone, not `sr-only`. It is what a reduced-motion reader reads.

- [ ] **Step 5: Test**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`.

- [ ] **Step 6: Commit**

```bash
git add site && git commit -m "feat(site): keycap component, hero and the three-OS demo"
```

---

### Task 4: "Focus is only the first press"

**Files:**
- Modify: `site/index.html` (append `<section id="how">`), `site/beckon.css`

**Interfaces:**
- Consumes: `.key`, `.demo`, `.demo-steps` from Task 3.
- Produces: nothing later tasks depend on.

> **CORRECTED 2026-08-12, after this task shipped.** Steps 1–3 used to
> prescribe ONE demo of five presses walking
> `Claude 1/3 → 2/3 → 3/3 → Brave → hidden`, and a table whose three
> "focused" rows implied that single linear walk. **That sequence cannot
> happen.** `CLAUDE.md`'s step 5 branches on a precondition, and its step-5a
> note is explicit: the cycle ring is ordered by address and *"rotating over
> them visits every window exactly once per lap. Verified live on sway: three
> `foot` windows, seven presses, `35 → 36 → 37 → 35 → …`"*. With three Claude
> windows open the ring never exits 5a, so press four returns to `1/3` and
> both `Brave` (5b) and `hidden` (5c) are unreachable. The agent that built it
> flagged the contradiction rather than quietly "improving" it. Steps 1–3
> below are the fixed prescription: **two** demos, each naming its own
> precondition.

- [ ] **Step 1: The copy**

Heading: **Focus is only the first press.**

Lead: *Most launchers stop at "bring it to the front". beckon keeps going, and
which thing happens is decided for you — there is no flag and nothing to
configure.*

The steps, from the focus algorithm in `CLAUDE.md`. Each row that depends on a
precondition must SAY the precondition — a row reading "No other window" leaves
the reader to guess which of 5a/5b/5c they are in:

| Press | What happens |
|---|---|
| App is not running | launch it |
| Running, not focused | focus it |
| Focused, app has another window | focus the next one, wrapping round |
| Focused, one window, another app open | switch back to the app you came from |
| Focused, one window, nothing else open | hide it |

- [ ] **Step 2: The demos — two of them**

Side by side from 900px up, stacked below it. Each is a `.demo` whose first
child is a caption naming its precondition; `.demo-steps` stays the last child.

**Demo A — "Three windows open".** Rows `Claude 1/3`, `Claude 2/3`,
`Claude 3/3` and no other app. The `C` key pulses **four** times over a 7.2s
`infinite` timeline (the key's own period is 1.8s, so 7.2s is exactly four
presses); the highlight walks `1/3 → 2/3 → 3/3 → 1/3`. Final keyframe = back
on `1/3`, which is the honest surprise: it laps.

**Demo B — "One window open".** Two rows, `Claude` (no count) and `Brave`. The
`C` key pulses **twice** over 3.6s; press one focuses Claude, press two goes
back to Brave. Final keyframe = Brave focused. 5c is carried by the transcript,
not by a frame — a demo cannot show a window that is not there.

Motion contract, unchanged and load-bearing: every animation carries
`animation-fill-mode: both` and every base rule equals its own 100% keyframe,
so the global reduced-motion block (`animation-duration: .01ms` +
`animation-iteration-count: 1`) lands on the final frame and holds. Prefer
**no `animation-delay` at all** — write one keyframe set per row. A positive
delay is only safe if that element's 0% and 100% frames are identical, because
reduced motion does not override the delay and a reader would sit in the
backwards fill for seconds.

- [ ] **Step 3: Transcripts**

```html
<p class="demo-steps">Four presses with three Claude windows open: it walks the ring and comes back round to the first.</p>
```

```html
<p class="demo-steps">With one Claude window and Brave also open: the first press focuses Claude, the second goes back to Brave. With nothing else open, that second press hides it instead.</p>
```

- [ ] **Step 4: Test**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`.

- [ ] **Step 5: Commit**

```bash
git add site && git commit -m "feat(site): the focus algorithm section"
```

---

### Task 5: "Type the name, not the id"

**Files:**
- Modify: `site/index.html` (append `<section id="names">`), `site/beckon.css`

**Interfaces:**
- Consumes: tokens; no new shared component.
- Produces: `.compare` (two-column good/bad card), reused by nothing else —
  keep it local and small.

- [ ] **Step 1: The copy**

Heading: **Type the name, not the id.**

Two stacked code lines:

```
beckon Claude
```
labelled *works on every machine you own*, and

```
beckon brave-fmpnliohjhemenmnlpbfagaolkdacoja-Default
```
labelled *works on exactly one*.

Body: *Brave and Chrome mint that hash locally when you install a PWA, so the
canonical id differs on every machine — copying your dotfile to a second
laptop silently stops working. The display name does not drift. beckon
resolves it against `.desktop` files on Linux, LaunchServices on macOS and the
Start menu on Windows, on every invocation. There is no cache to rebuild and
no alias file to keep in sync.*

Then the discovery commands, as a `<pre>`:

```
beckon installed | grep -i claude
beckon resolve Claude
beckon doctor
```

- [ ] **Step 2: Style**

The bad line gets `--fg-dim` and `text-decoration: line-through` on its label
only — never on the code itself, which must stay copyable and legible.

- [ ] **Step 3: Test**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`.

- [ ] **Step 4: Commit**

```bash
git add site && git commit -m "feat(site): names-not-ids section"
```

---

### Task 6: "Works with your setup"

The compatibility grid. This is the task most likely to overstate what beckon
does; the GNOME note exists to stop that.

**Files:**
- Modify: `site/index.html` (append `<section id="setups">`), `site/beckon.css`

**Interfaces:**
- Consumes: `.key` from Task 3 for the letter table.
- Produces: `.tiles`, `.tile` — used only here.

- [ ] **Step 1: The eight tiles**

Exactly these, each a `.tile` with a name and one note. Sourced from the
dispatch table in `CLAUDE.md` and the directories in `examples/`.

| Tile | Note |
|---|---|
| macOS | needs Accessibility permission |
| Windows | Start menu, MSIX/AppX and PWAs |
| sway | native i3-IPC |
| i3 | same backend as sway |
| Hyprland | native socket IPC |
| GNOME (Wayland) | **needs the bundled shell extension** |
| KDE (Wayland) | nothing to install — rides KWin scripting |
| XFCE · openbox · awesome · fluxbox | any EWMH X11 desktop |

The GNOME note is not optional and must not be softened. Every other tile
works straight after a package install; that one needs an extension and a
re-login. A grid that hides it is selling something beckon does not do.
Give that tile a visible marker (`--accent` left border) rather than burying
the note in small text.

- [ ] **Step 2: The letter table**

From `README.md`, using `.key` for each letter:

| Key | App |
|---|---|
| `Space` | terminal |
| `C` | Claude |
| `B` | Brave |
| `E` | Cursor |
| `D` | Discord |

Caption: *The examples wire the same five keys everywhere, so you remember the
letter and not the modifier.* Then the defaults: `Super` on Linux, Hyper
(`cmd+ctrl+alt`) on macOS, `Ctrl+Win+Alt` on Windows.

- [ ] **Step 3: Link to the examples**

One link to `https://github.com/xom11/beckon/tree/main/examples` — check 3
verifies `examples` exists on disk.

- [ ] **Step 4: Test**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`. If check 3 reports a missing path, the link is wrong, not
the check.

- [ ] **Step 5: Commit**

```bash
git add site && git commit -m "feat(site): compatibility grid and the shared key table"
```

---

### Task 7: "Or let beckon hold the keys"

**Files:**
- Modify: `site/index.html` (append `<section id="serve">`), `site/beckon.css`

**Interfaces:**
- Consumes: `.key` from Task 3.
- Produces: nothing shared.

- [ ] **Step 1: The copy**

Heading: **Or let beckon hold the keys.**

Lead: *On macOS and Windows nothing binds hotkeys for you, so beckon can do it
itself. `beckon serve` reads a flat TOML and registers every line.*

```toml
"ctrl+super+alt+t" = "kitty"
"ctrl+super+alt+shift+t" = "Telegram Web"
```

Three short cards:

- **macOS** — `brew services start beckon` is the whole install; the formula
  ships the LaunchAgent.
- **Windows** — `beckon-serve.exe` is a tray app with no console window at any
  point. Reload, pause, open the log, or open Settings from the tray; tick
  *Start with Windows*.
- **Linux** — deliberately not served here. Your compositor already binds keys
  better than beckon could, so `bindsym $cap+c exec beckon Claude` is the
  integration.

The Linux card is the honest half and stays. Do not present `serve` as
cross-platform; it is not.

- [ ] **Step 2: The settings-window line**

One sentence: *The Windows settings window lists every binding with whether it
actually registered, and writes back the same `apps.toml` you would edit by
hand — comments and key order survive.* No screenshot; there is none.

- [ ] **Step 3: Test**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`.

- [ ] **Step 4: Commit**

```bash
git add site && git commit -m "feat(site): resident mode section"
```

---

### Task 8: Install — OS tabs and copy buttons

Task 2 already placed the exact command text. This task makes it a usable
section without re-authoring a single command.

**Files:**
- Modify: `site/index.html` (`<section id="install">`), `site/beckon.css`,
  `site/beckon.js`

**Interfaces:**
- Consumes: the four command blocks placed in Task 2.
- Produces: `.tabs`/`.panel` and the copy button; nothing later depends on it.

- [ ] **Step 1: Markup, JS-off first**

Four `<section class="panel">` blocks — Homebrew, Scoop, Cargo, Nix — each
with an `<h3>` and its `<pre><code>`. **All four ship visible in the HTML.**
The tab strip is `<button role="tab">` elements inside a `<div role="tablist">`
that carries `hidden` in the markup; JS removes that `hidden` and adds it to
three panels.

Written the other way round — `hidden` in the markup, removed by JS — a reader
with JS off sees three empty gaps and no way to install on their OS. This is a
correctness requirement, not a preference.

- [ ] **Step 2: The JS**

```js
// Install tabs. The HTML ships every panel visible and the tablist hidden;
// this only ever *reduces* what is on screen, so JS-off degrades to
// "all four platforms listed", which is a fine page.
(() => {
  const list = document.querySelector('#install [role="tablist"]');
  if (!list) return;
  const tabs = [...list.querySelectorAll('[role="tab"]')];
  const panels = tabs.map(t => document.getElementById(t.getAttribute('aria-controls')));
  const show = i => {
    tabs.forEach((t, n) => {
      t.setAttribute('aria-selected', String(n === i));
      t.tabIndex = n === i ? 0 : -1;
      panels[n].hidden = n !== i;
    });
  };
  list.hidden = false;
  tabs.forEach((t, i) => {
    t.addEventListener('click', () => show(i));
    t.addEventListener('keydown', e => {
      const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
      if (!d) return;
      e.preventDefault();
      const n = (i + d + tabs.length) % tabs.length;
      show(n); tabs[n].focus();
    });
  });
  const p = navigator.userAgent;
  show(/Mac/i.test(p) ? 0 : /Win/i.test(p) ? 1 : /Linux|X11/i.test(p) ? 3 : 0);
})();

// Copy buttons. Hidden entirely when the clipboard API is absent, rather than
// rendering a button that silently does nothing.
(() => {
  if (!navigator.clipboard) return;
  document.querySelectorAll('#install pre').forEach(pre => {
    const b = document.createElement('button');
    b.className = 'copy'; b.type = 'button'; b.textContent = 'Copy';
    b.addEventListener('click', async () => {
      await navigator.clipboard.writeText(pre.querySelector('code').textContent.trim());
      b.textContent = 'Copied'; setTimeout(() => (b.textContent = 'Copy'), 1400);
    });
    pre.appendChild(b);
  });
})();
```

Tab order is Homebrew, Scoop, Cargo, Nix — the `show()` call maps macOS→0,
Windows→1, Linux→3 (Nix), everything else→0.

- [ ] **Step 3: Add the resident-mode follow-ons**

Under Homebrew add `brew services start beckon`; under Scoop note that
**beckon serve** lands in the Start menu. Neither is one of the five checked
lines, so neither can break check 5 — but both must still match `README.md`.

- [ ] **Step 4: Test**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`. Then verify JS-off behaviour by eye: comment out the
`<script>` tag, reload from `file://`, confirm all four panels are visible and
no tab strip is shown. Restore the tag.

- [ ] **Step 5: Commit**

```bash
git add site && git commit -m "feat(site): install tabs with a JS-off-first fallback"
```

---

### Task 9: FAQ, and the accessibility sweep

**Files:**
- Modify: `site/index.html` (append `<section id="faq">`), `site/beckon.css`,
  `site/beckon.js`

**Interfaces:**
- Consumes: everything.
- Produces: the finished page.

- [ ] **Step 1: The seven entries**

Native `<details>`/`<summary>`, so it works with JS off.

1. **Do I need a config file?** — Not for `beckon <id>`; ids resolve against
   the OS every time. Only `serve` reads a file, and that file is a hotkey
   table, not an alias list.
2. **Why names instead of bundle ids?** — PWA hashes are minted per install
   and differ per machine. Names do not drift.
3. **Does beckon register the hotkey on Linux?** — No. Your compositor does,
   and that is a choice: every Linux desktop already ships a place to bind a
   key to a command, and there is no single API that would cover all of them.
4. **Why does macOS ask for Accessibility?** — Focusing another app's window
   needs it. The grant is bound to the signed binary, so a rebuild can reset
   it.
5. **What does GNOME Wayland need?** — The bundled shell extension, plus a
   re-login. Mutter gives external processes no way to focus a window, so an
   in-process collaborator is the only route.
6. **Is it fast?** — `beckon <id>` measured ~57 ms on an ARM64 Windows 11
   laptop and ~95–105 ms on an M3 MacBook Air, dominated in both cases by the
   OS's own launch/activation call. Each figure is one machine.
7. **Can I use Caps Lock as the modifier?** — On Windows, opt-in. Three
   caveats, all real: it does nothing while an elevated window has focus, any
   other remapper claiming Caps wins, and a low-level keyboard hook is a
   signature some EDR products flag.

Every figure in entry 6 carries its machine. Do not shorten it to "57 ms".

- [ ] **Step 2: Accordion JS**

```js
// Native <details> already works. This only closes siblings.
document.querySelectorAll('#faq details').forEach(d =>
  d.addEventListener('toggle', () => {
    if (!d.open) return;
    d.parentElement.querySelectorAll('details[open]').forEach(o => { if (o !== d) o.open = false; });
  }));
```

- [ ] **Step 3: Accessibility sweep**

- One `<h1>`; headings descend without skipping.
- The theme button has a live `aria-label` (Task 2 already sets it).
- Every decorative demo element is `aria-hidden="true"`; the `.demo-steps`
  transcript is not.
- Focus ring visible on nav links, both hero buttons, every tab, every
  `<summary>`, both copy buttons, footer links.
- `<html lang="en">`.

- [ ] **Step 4: Full test**

```bash
./tools/check-site.sh; echo "exit=$?"
```

Expected `exit=0`.

Then, with a browser (`python3 -m http.server -d site 8080`):

- renders in light, dark and OS-default;
- no horizontal scroll on `body` at 375, 768 and 1440 px;
- tab order reaches nav → hero buttons → tabs → summaries → footer;
- both demos loop cleanly;
- with `prefers-reduced-motion: reduce` forced, both demos sit on their final
  frame and the transcripts read.

- [ ] **Step 5: Commit**

```bash
git add site && git commit -m "feat(site): FAQ and the accessibility sweep"
```

---

### Task 10: Documentation and the manual switch

**Files:**
- Modify: `README.md`, `CLAUDE.md`

- [ ] **Step 1: README**

Add the site link immediately under the project heading, above Quickstart.

- [ ] **Step 2: `CLAUDE.md`**

Append to *Distribution*:

> **Landing page**: `site/`, deployed by `.github/workflows/pages.yml` (Pages
> source = **GitHub Actions**, set by hand in repo settings — left at *Deploy
> from a branch* the workflow goes green and publishes nothing). Not `docs/`:
> that directory holds internal specs, plans and measurements, and serving
> Pages from `/docs` would publish them. `tools/check-site.sh` is the page's
> test suite and runs in CI; it asserts the install commands still byte-match
> `README.md` and that the version matches `Cargo.toml`, so a release bump
> that forgets the page fails CI rather than shipping a stale command.

- [ ] **Step 3: Set the repo homepage**

```bash
gh repo edit xom11/beckon --homepage "https://xom11.github.io/beckon/"
```

- [ ] **Step 4: Confirm Pages source**

Repo Settings → Pages → Source = **GitHub Actions**. Then:

```bash
gh workflow run "Pages"
gh run list --workflow=Pages --limit 1
```

Then open the deployment URL and confirm it is not a 404.

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs: link the landing page and record how it deploys"
```

---

## Deferred (not in this plan)

- **A designed 1200×630 OG card.** v1 points `og:image` at `icon-512.png`,
  which previews as a square mark. A real card needs a rasterizer in CI or a
  checked-in PNG; neither is worth blocking on.
- **Real screenshots and video.** The demos are illustrations and must never
  be captioned as recordings. When `airm3` and `a14` can produce clips, the
  `.demo` markup is meant to be swapped for `<video>` at the same dimensions
  without touching layout.
- **Testimonials**, when there are real ones.
