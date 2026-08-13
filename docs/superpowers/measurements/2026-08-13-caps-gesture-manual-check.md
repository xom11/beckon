# The Caps Lock gesture on the landing page — what a human still has to press

Date: 2026-08-13
Subject: `site/index.html`, `site/beckon.css`, `site/beckon.js` — the hero
demo and the `#how` playground, after the trigger changed from a bare
stand-in `C` to **Caps Lock on + a letter**.

**Nothing in this file has been run.** The session that wrote the feature had
no physical keyboard: every keyboard result it has came from
`document.dispatchEvent(new KeyboardEvent(..., { modifierCapsLock: true }))`,
which sets `getModifierState('CapsLock')` from the init dictionary and
therefore proves the page's *handler* is correct while proving nothing at all
about the *lock*. A synthetic event cannot turn a lock on, cannot be
suppressed by `preventDefault`, and cannot tell you what a real macOS keyup
reports. This is the list of presses that closes that gap.

Budget: about six minutes at a real keyboard.

---

## 0. Control first — prove the probe can see anything

Without this step a dead page and a clean result look identical, which is the
trap `examples/windows/serve/README.md` and the a14 landing-1 write-up both
record paying for.

1. Serve the page: `python3 -m http.server 8791` from `site/`, open
   `http://127.0.0.1:8791/index.html`.
2. Scroll to the hero demo. **Click** the `C  Claude` button with the mouse.
3. **Expect:** the readout above the transcript changes from `READY` to
   `FOCUS IT`, and Claude comes to the front in all three OS cards.

If that does nothing, stop — the page is broken and no keyboard result below
means anything. Everything from §1 on assumes this passed.

While you are here, note what the lock chip says. Before any key or click it
reads `Caps Lock: unknown`; a real mouse click carries the modifier state, so
after step 2 it should already read `on` or `off` correctly. **Write down
which**, because §1 and §2 differ depending on where you start.

---

## 1. The gesture, from Caps off

State: Caps Lock **off** (chip reads `Caps Lock: off`, and the `Caps` keycap
in the try row is drawn UP).

| # | Press | Expect |
|---|---|---|
| 1.1 | `C` alone | Nothing moves. The bold line above the buttons turns accent-coloured and reads *"Caps Lock is off — switch it on, then press C. Or click C."* It goes back to the normal instruction after about three seconds. |
| 1.2 | `Caps Lock` (tap it on) | Chip reads `Caps Lock: on`. The `Caps` keycap is drawn DOWN and accent-coloured. Your keyboard's own Caps Lock light comes on. Nothing else on the page moves. |
| 1.3 | `C` | Claude comes forward in all three cards. Readout: `FOCUS IT`. The instruction line is back to normal (not the nudge). |
| 1.4 | `B` | Brave comes forward. **The last cap of all three chords changes from `C` to `B`** — `Cmd+Ctrl+Alt+B`, `Ctrl+Win+Alt+B`, `Super+B`. Readout: `FOCUS IT`, naming Brave. |
| 1.5 | `B` again | Claude comes forward. Readout: `SWITCH BACK` — "one window, already focused, and Claude is open". |
| 1.6 | `E` | Nothing moves. The instruction line reads *"E is Cursor. These three cards only draw Claude and Brave — the playground under "Focus is only the first press." takes all five."* |

**1.2 is the load-bearing one.** If the lock does NOT come on — because a
remapper (kanata, Karabiner, PowerToys) owns Caps on this machine, or because
the screen reader has it — then everything after it is untestable on this
machine and the page's whole premise is wrong *for this reader*. Note the
machine and move to a second one. Do not "fix" it by making the page fall
back to a bare letter; the click path is the fallback.

---

## 2. The playground, all five letters

Scroll to **Focus is only the first press.** so the playground fills most of
the viewport. Caps Lock **on**. Scenario 1 (`Claude is not running`) selected.

| # | Press | Expect (readout step, then the drawing) |
|---|---|---|
| 2.1 | `C` | `STEP 4` — the dashed Claude outline becomes a solid focused window. |
| 2.2 | `C` | `STEP 5C` — it goes dashed again and reads `HIDDEN`. |
| 2.3 | `C` | `STEP 5` — solid and focused again. |
| 2.4 | `D` | `STEP 4` — a **new** Discord window appears below Claude and takes focus; Claude reads `BACKGROUND`. |
| 2.5 | `D` | `STEP 5B` — focus returns to Claude, "switches back to Claude, the app you came from". |
| 2.6 | `E` | `STEP 4` — a Cursor window appears and takes focus. |
| 2.7 | `Space` | `STEP 4` — a `terminal` window appears and takes focus, **and the page does not scroll**. |
| 2.8 | `B` | `STEP 4` — a Brave window appears. Five windows on the desk. |
| 2.9 | Click `Reset` | Back to one dashed Claude window, readout `RESET`. |

Then the ring, which is the one branch a wrong model gets wrong:

| # | Press | Expect |
|---|---|---|
| 2.10 | Click `Three windows open` | Three Claude windows plus Brave; Brave focused. |
| 2.11 | `C` ×5 | `STEP 5` (1/3) → `5A` (2/3) → `5A` (3/3) → `5A` (**1/3 again**, "that is the lap closing") → `5A` (2/3). Brave stays `BACKGROUND` the whole time and never takes focus. |

### 2.12 — Space must not steal a focused button

Still in the playground, Caps Lock on:

1. Click `Reset` (so `Reset` has keyboard focus — Chromium keeps it after a
   click).
2. Press `Space`.
3. **Expect:** the readout says `RESET` again — the button was activated, the
   scenario was reset. It must **not** launch a terminal window.
4. Now press `C` with `Reset` still focused. **Expect:** the demo advances.
   A letter is never withheld from a focused button; only `Space` is.

Step 4 is the regression guard for the blocker that shipped in an earlier
pass, where the letter went dead after clicking anything.

---

## 3. The honest part — did the page really leave the lock on?

1. With Caps Lock **on** from §1 or §2, look at the playground's second
   caveat paragraph. It must open **"Your Caps Lock is on, and pressing it
   here is what did that."**
2. Switch Caps Lock off. The same paragraph must flip to **"Press Caps here
   and it really will turn your Caps Lock on."**, and the chip and keycap must
   follow, **without pressing any other key**.

Step 2 is the macOS-specific one and it is the reason there is a `keyup`
listener at all: on macOS the browser sees keydown on the **on**-transition
and keyup on the **off**-transition, so a keydown-only page would latch "on"
forever. **Run step 2 on macOS specifically.** If the chip only corrects
itself after you press some other key, the keyup path is not firing and the
indicator is a one-way latch.

### 3.3 — the claim the page refuses to make

The page does **not** call `preventDefault()` on the Caps Lock key and does
not claim it could stop the lock. If someone later adds that, this is the
test: press Caps Lock on the page, and check the OS's own Caps Lock light. If
the light comes on anyway, the copy must not say the page swallowed anything.
Nothing measured so far says it can.

---

## 4. Cross-axis, and the two states the copy has

Quick, but each one is a different sentence rendered by a different branch.

| # | Do | Expect |
|---|---|---|
| 4.1 | Nav OS → `Windows` | Playground caveat: *"On Windows this is a real setting rather than an illustration: tick the Caps Lock box in Settings…"* plus the elevated-window line. Chord reads `Ctrl+Win+Alt+<letter>`. |
| 4.2 | Nav OS → `macOS` | *"beckon's Caps Lock mode is Windows-only, so on macOS this is the demo's trigger and not something you can switch on."* Chord reads `Cmd+Ctrl+Alt+<letter>`. |
| 4.3 | Nav OS → `Linux` | Same as 4.2 with "on Linux". Chord reads `Super+<letter>`. |
| 4.4 | Theme button, both ways | The locked `Caps` keycap and the `Caps Lock: on` chip stay accent-coloured and legible in both. |
| 4.5 | Reload with Caps already on | Chip reads `unknown` until the first key or click, then jumps straight to `on` — and the first letter you press **fires immediately**. Arriving with the lock on is a legitimate state, not something to be told to fix. |

4.5 is worth doing deliberately: it is the state a habitual Caps-Lock user is
in when they land, and the failure mode (being told to switch on a lock that
is already on) is invisible from a machine where the lock starts off.

---

## 5. What this file does not cover, and why

- **A screen reader.** NVDA and JAWS both bind Caps Lock as their own
  modifier, so the keyboard path is unavailable to those readers by
  construction and the pointer path is the answer. Worth a pass with VoiceOver
  to confirm the button names read as *"Run beckon Claude — Caps Lock plus
  C"* and that the readout announces once per press, not twice.
- **A phone.** No Caps Lock key exists, so §1 and §2 are pointer-only there;
  what matters is that all five buttons are reachable and full-size at 375 px,
  which was checked in a browser (no horizontal scroll, buttons wrap to three
  rows) rather than on hardware.
- **A non-QWERTY layout.** The handler compares `e.key`, not `e.code`, so it
  follows the letter printed on the reader's keycap. Untested on AZERTY.
