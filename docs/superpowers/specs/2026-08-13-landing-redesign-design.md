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
| 2 | Trigger is **Caps Lock and a letter**. |
| 3 | Visual identity is **authentic desktops**; the page itself is near-colourless. |
| 4 | Copy is **cut hard**; everything removed moves verbatim into the FAQ. Nothing is deleted. |
| 5 | `site/` is **rewritten**, keeping six named contracts (below). |
| 6 | The hero draws **one desk, the reader's**, at full size — not three side by side. |

Decisions 2 and 6 were taken after seeing the first build. Both reverse an
earlier choice, and the reversals are recorded rather than tidied away because
the reasoning that produced the first version is still the reasoning that
constrains this one.

### One desk, not three

Three desks made the cross-platform claim by drawing it, and each came out
340px wide — a thumbnail of a desktop rather than a desktop. The hero has one
job, which is to look like the machine the reader is sitting at, and 340px
does not do it.

**The claim moved to a control, not to the copy.** An OS strip sits over the
desk: pressing *Windows* redraws the same desk in Windows chrome, with the same
gesture, and the nav's switcher follows. So "the same key on every OS" is
something the reader does. Under the desk, all three real chords ship in the
markup and CSS keeps the reader's own — which means a JS-off reader, who has no
strip to press, keeps all three and loses nothing.

### Caps Lock and a letter, not a bare letter

A bare letter is frictionless and reads as nothing. `C` is not a shortcut; a
page that teaches beckon by asking for `C` has taught the wrong shape.

**A page cannot see a *held* Caps Lock** — there is no `capsKey` on a keyboard
event the way there is `shiftKey`. Measured 2026-08-13, there are exactly two
observable signals, and the gate accepts either:

1. **The lock is on** — `getModifierState('CapsLock')`. It is available on
   `MouseEvent` and `PointerEvent` as well as `KeyboardEvent`, so the state
   becomes known as soon as the reader moves the mouse. The earlier version of
   this page could only learn it from a keypress, which is why it shipped a
   readout reading `Caps Lock: unknown` on first sight.
2. **The Caps key was just touched** — a `keydown`/`keyup` whose `key` is
   `CapsLock`, within 1.5s. macOS fires only keydown on the way on and only
   keyup on the way off, so both arm it. This is also the only half a
   synthetic-event test can reach: Chrome does not flip its caps modifier for
   injected keys, measured with the same probe.

**A remapped Caps Lock satisfies neither**, and kanata / PowerToys / a Hyper
remap are disproportionately common among the people who would want this tool.
A demo that cannot be operated is worse than one that teaches its gesture
loosely — so there is a way out, and **it is a button, not a counter**.

Two refused presses used to open the gate by themselves. That was worse than
either alternative: a reader whose Caps works fine reached it by fumbling
twice, and from the outside the demo simply looked like it had quietly stopped
asking for Caps. The gate now never opens on its own. After two refusals the
hint offers *Use letters only*, and only a click opens it — for every demo at
once, since having answered the question in the hero should not mean answering
it again in `#how`.

Clicking a key never goes through the gate: requiring a lock key from a pointer
would be asking for a gesture the device may not have.

Why Caps and not each platform's real chord: the browser cannot see those
either. `Super` is grabbed by every Wayland compositor and most X11 WMs, and
`Win` opens the Start menu. Only macOS Hyper is observable. Caps is beckon's
own gesture on Windows, and here it stands in for whatever the reader's real
chord is — which the rows under the desk name.

## Architecture

### `site/desk.js` — the model, and the only file with the algorithm in it

Pure. No DOM, no globals beyond one export.

```js
Desk  = { os, wins: [{ id, app, min, slot }], focused }
press(desk, letter) -> { desk, step }   // step ∈ '4' | '5' | '5a' | '5b' | '5c'
```

**`slot` is where the window sits, and nothing ever changes it.** Focusing a
window raises it; it does not move it. The renderer derived position from MRU
order at first, so focusing Claude slid it into the place Brave had been
occupying while Brave slid out — and with two windows of similar size that does
not read as "Claude came forward", it reads as **the Brave window having been
renamed to Claude**, which is the exact opposite of what the demo exists to
show. Position comes from the window, stacking comes from the order, and three
tests pin it (`a focused window keeps the place it already had`, `no press of
any letter ever changes a slot`, `a launched window takes a new place, not
somebody else's`).

The same defect had a second home: the JS-off `@keyframes` swapped the two
windows' cascade offsets. They no longer mention `transform` at all, which is
what lets the base rule's `translate(var(--slot) …)` stand.

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

Three more, all of them consequences of the hero going to one big desk, and all
found by driving the page rather than by reading it:

- **The hero went deaf.** Key routing scored a demo by *how much of the demo was
  visible*, so a demo taller than the viewport could never reach the 0.5
  threshold: at 841px against 900px it scored 0.481 and the hero answered
  nothing. A reader landed, read the instruction, pressed, and got silence. The
  denominator is the viewport now, and the floor is a quarter of it.
- **A refused press is no longer swallowed.** `preventDefault` ran before the
  Caps gate, so a reader whose Caps is remapped lost Space as a scroll key and
  got nothing back for it. `press()` returns whether it consumed the key.
- **Window chrome scales with the desk.** The two desks on the page differ by
  nearly 2:1 in width, and a 22px title bar that reads correctly at 500px is a
  hairline at 860px. The chrome is sized in `cqw` against the desk as a
  container, each rule stating a plain px value first so a browser without
  container query units gets the middle size rather than none.

And one layout rule that is a correctness rule in disguise: the hero desk's
width is capped by the height the copy above it leaves, so the whole desk —
including the dock along its bottom edge — is on screen without scrolling. The
dock is not decoration. It is the only thing that distinguishes step 4 from
step 5, because launching an app and focusing one end with the same window in
the same place and differ only in a slot lighting up.

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
