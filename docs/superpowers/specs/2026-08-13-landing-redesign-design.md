# Landing page redesign — the page becomes the product

Date: 2026-08-13
Branch: `worktree-landing-redesign`, cut from `worktree-landing-caps` @ `d682c2b`
Supersedes the presentation layer of
`docs/superpowers/specs/2026-08-12-github-pages-landing-design.md`. That spec's
five verifiable claims survive unchanged and are still enforced by
`tools/check-site.sh`.

## Why

The page shipped its argument as three grey boxes. Measured against
[homerow.app](https://homerow.app), which is the reference the reader asked
for, four things were wrong and none of them were about taste:

1. **The demo was a diagram of the product, not the product.** Homerow's hero
   is a real macOS desktop — wallpaper, icons, Trash — and its shortcut labels
   the page's own navigation. beckon's hero was three `.os-stack` divs holding
   two grey bars each. A reader cannot recognise their own machine in that.
2. **Three identical boxes read as repetition, not as "three OSes".** The claim
   the hero exists to make — *same letter, three different chords* — was
   carried by a text label above each box and by nothing visual at all.
3. **The demo asked for a precondition before it would run.** "Turn Caps Lock
   on, then press C" is two steps, it toggles the reader's real Caps Lock, and
   the readout admitted `Caps Lock: unknown` — a control that reports it does
   not know its own state. Homerow's hero asks for one chord and says
   "Try it now".
4. **Nothing taught the letters.** Homerow pins a two-row cheat sheet to the
   bottom-right corner for the whole scroll. beckon's letter table sat once,
   mid-page, in a section most readers never reach.

Underneath all four: 70 KB of CSS and 74 KB of JS for a static page, spent on
a card-and-shadow system that reads as generated rather than designed.

## Decisions taken (all five confirmed with the reader before any code)

| # | Decision |
|---|---|
| 1 | Hero is a **simulated desktop with real window chrome**, driven by real keypresses — not a diagram. |
| 2 | Trigger is the **bare letter** (`C` `B` `E` `D` `Space`). No modifier, no lock state. |
| 3 | Visual identity is **three authentic desktops**; the page itself is near-colourless. |
| 4 | Copy is **cut hard**; everything removed moves verbatim into the FAQ. Nothing is deleted. |
| 5 | `site/` is **rewritten**, keeping six named contracts (below). |

### Why the bare letter, and not beckon's real chord

A browser cannot observe beckon's actual gesture on two of three platforms.
`Super` is grabbed by every Wayland compositor and by most X11 WMs before the
browser sees it; `Win` opens the Start menu; `Caps Lock` exposes only its
*lock state* through `getModifierState`, never a press. Only macOS Hyper
(`Cmd+Ctrl+Alt+C`) is genuinely observable.

So a page that insists on the real chord either lies, or degrades to "press
this other thing instead" on two platforms out of three. The page states the
real chord in the caption under each desk and asks for the letter alone:

> On your machine this is `Cmd+Ctrl+Alt+C`. A web page never sees that
> chord, so here the letter is enough.

The page has no text input, so a bare letter listener cannot swallow anyone's
typing.

## Architecture

### `site/desk.js` — the model, and the only file with the algorithm in it

Pure. No DOM, no globals beyond one export.

```js
Desk  = { os, apps: [{ key, name, windows: [id] }], stack: [id], focused }
press(desk, letter) -> { desk, step }   // step ∈ '4' | '5' | '5a' | '5b' | '5c'
```

`press` is CLAUDE.md's *Focus algorithm* transcribed, in the order that
document tests its branches, and it is the same shape as `beckon-linux`'s
`algorithm::decide`. This is the point of splitting the file out: the page's
claim about what beckon does is now a function with a test suite, instead of a
sentence next to an animation nobody can check.

Classic script, not an ES module — the page must open from `file://`, where
module scripts are blocked by CORS. A `module.exports` tail makes it
importable by `node --test` without affecting the browser.

### `site/beckon.js` — DOM only

Renderer (`stack` index → `z-index`, `focused` → lit/dim), key handling,
install tabs, theme + OS switch, copy buttons, HUD. Knows nothing about the
algorithm.

### The two demos are one component

| | Hero | `#how` |
|---|---|---|
| desks | three, one per OS | one, the reader's OS |
| driven by | one keypress → all three | one keypress → one desk |
| answers | *"the same letter on every machine I own"* | *"what happens when I press it again"* |
| reset | none needed | the algorithm table's rows |

### The algorithm table is the scenario switcher

The old page carried a five-row table *and*, with JS on, a separate
four-scenario playground — two controls telling one story, stacked.

A single desk cannot walk all five branches from keypresses alone, and this is
structural rather than a limitation of the mock: with three Claude windows
open the ring never exits 5a, so 5b and 5c are unreachable until the app is
down to one window. (`site/index.html` recorded this before the rewrite,
verified live on sway.) Something has to set the precondition.

So the rows *are* that control. Clicking a row builds the desk that satisfies
its situation; pressing a letter runs the algorithm; the row whose step fired
lights up. The table is read in either direction and the two controls collapse
into one block.

Rows are `<button>`s inside the cell, not click handlers on `<tr>` — a table
row is not focusable and cannot carry a name.

### Per-OS chrome is the whole visual identity

No external subresource is permitted (check 1), so there are no wallpapers to
download and no app logos to embed — which is also the right answer for
trademarks. Everything is CSS.

| | macOS | Windows 11 | Linux / sway |
|---|---|---|---|
| radius | 10px | 8px | 0 |
| titlebar | 28px, title centred, three lights left | 32px, title left, `─ □ ✕` right | none |
| focused | full colour + `0 20px 60px -20px` | 1px accent border + shadow | 2px accent border |
| unfocused | 55% opacity, no shadow | grey titlebar | 1px muted border |
| arrangement | cascade, overlapping | cascade, overlapping | **tiled, 8px gap, never overlapping** |
| wallpaper | blue→violet linear-gradient | radial bloom | flat |

The sway column is the one that earns the section. **A tiling compositor does
not stack windows**, so "focus" there is a border moving, not a window rising.
Modelling that is what makes the page look written by someone who runs it.

It also corrects a claim: the hero transcript said *"Claude comes to the front
on all three."* There is no front on a tiling WM. It now reads **"Claude takes
focus on all three."**

### The HUD

Fixed bottom-right, `role="group"` with real `<button>`s — not `aria-hidden`,
because a focusable control inside `aria-hidden` is a defect, and because the
buttons are how a reader without a keyboard presses a key at all.

Ships with the `hidden` attribute; JS removes it. That inversion is legal here
under the page's existing rule — it is only allowed where everything the
control offers is already reachable without it, which holds: every demo also
carries its own press row, and `#setups` carries the canonical table.

Hidden below 900px, where it would cover content.

## Contracts carried over from the old page

These are requirements, not preferences. Each one was paid for by a defect.

1. **JS only ever reduces what is on screen.** Every desk ships rendered in a
   readable final state with its transcript below it; JS makes it pressable.
   Nothing ships `hidden` waiting for JS except controls that do nothing
   without JS (install tablist, OS switch, theme button, HUD).
2. **Theme and `data-os` resolve before first paint** in an inline `<script>`
   in `<head>`. Anything later is a full-page colour flash.
3. **`[hidden] { display: none !important }`** — without it the UA rule loses
   to `.tabs { display: flex }` on source order and the tablist ships visible
   and inert.
4. **Every token is defined on the bare `:root`**, with `@media` and
   `[data-theme]` only restating what differs.
5. **The five install commands byte-match README.md.**
6. **The letter→app table keeps its exact markup**,
   `<kbd class="key">C</kbd></th><td>Claude</td>`, because that string is what
   `check-site.sh` greps for.

## Testing

`tools/check-site.sh` keeps all five existing groups and gains:

- `node --test site/desk.test.mjs`, covering all five branches — skipped with a
  notice if `node` is absent, since the repo is otherwise Rust-only. GitHub's
  `ubuntu-latest` ships node.
- the hero carries a desk for each of `macos`, `windows`, `linux`
- the HUD ships with `hidden`
- `class="demo"` count still equals `.demo-steps` count (two of each now)

The reduced-motion block pins `animation-duration`,
`animation-iteration-count: 1` **and `animation-fill-mode: forwards`**, and
check 4 now requires all three. The third is not tidiness. Measured under
Chrome's reduced-motion emulation while building this: the first two run the
animation and then hand the element back to its *base* style, which here is the
markup's opening frame — Brave focused, Claude behind it. So a reader who asked
for no motion got the one picture on the page that contradicts the sentence
under it, while the check reported ok. Two of the three is not a guarantee.

A second finding from the same pass, fixed the same way: macOS traffic lights
were coloured by the `.is-focused` class, and a `@keyframes` rule cannot add a
class — so the JS-off loop raised a window that sat full-opacity and on top
with three grey dots. The lights are now coloured unconditionally and unfocus
is a `filter: grayscale(1)`, which a keyframe *can* reach.

## Copy budget

| Section | Before | After |
|---|---|---|
| `#hero` | h1, 2-sentence lead, demo, 3-sentence Caps note | h1, **one sentence**, three desks, press row |
| `#how` | h2, lead, table, two looping demos, playground | h2, **one sentence**, one merged table-and-desk |
| `#names` | h2, comparison, 90-word paragraph, 3 commands | h2, comparison, **one sentence**, 3 commands |
| `#setups` | h2, grid, table, 3 notes | h2, grid, table, **one note** |
| `#serve` | h2, lead, figure, 3 cards, 4 notes | h2, **one sentence**, figure, 3 one-line cards |
| `#faq` | 7 entries | receives every cut sentence, verbatim |

The Caps Lock paragraph in `#serve` duplicated the FAQ entry below it almost
word for word. The duplicate goes; the FAQ entry stays.

Target: 6789px → ~4200px at 1440 wide.

## Out of scope

- Scroll-driven or sticky desks. Considered and rejected: fragile on mobile,
  poor under `prefers-reduced-motion`, and impossible to keep honest with JS
  off.
- Real app icons or logos. Trademarks, and check 1 forbids fetching them.
- Any change to `README.md`'s install commands or letter table. The page
  follows those files; it does not lead them.
