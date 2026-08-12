# Landing 2b — what it left owed

Date: 2026-08-12
Shipped as **v0.8.0**, installed and running on a14 from the scoop `current`
junction against the real config (`apps.windows.toml`, 18 shortcuts).

Landing 2b turned out to be **five** pieces, not the four first planned — the
typed shortcut controls (§C.4) had never been built, and capture depends on
them. All five are merged:

| | Piece | Verified on a14 |
|---|---|---|
| 2b-i | the pure capture state machine | n/a — pure, 3 CI jobs |
| 2b-ii | the Caps Lock row (§F.8) | layout, §55–56 |
| 2b-iii | four modifier boxes + the 81-key list (§C.4) | layout + driving them, §57–58 |
| 2b-iv | the availability probe (§F.6) | gives the hotkey back, §60 |
| 2b-v | the hook refcount and capture wiring | capture works end to end, §62–64 |

Measurements: `docs/superpowers/measurements/2026-08-11-landing-1-a14.md`
§47–§65. Refuted claims: `docs/superpowers/specs/2026-08-11-settings-window-redesign.md` §7.

---

## 1. The UI is functional, not designed

The user's verdict after using v0.8.0: *"dùng cũng tạm rồi nhưng UI chưa thiết
kế đẹp lắm."* Every piece landed as a correct control in a correct place; none
of it was composed as a whole. A polish pass is the next session's work, and
these are the specific things a fresh eye should look at first.

**The editor strip is now seven controls on one line** — `App [combo] Shortcut
[ ][ ][ ][ ] [key list]` — measured at 415 px for the App combo and 12 px
between everything else (§57). It fits, it does not overlap, and it reads as a
row of parts rather than one thing. `Record` and `Reset` joined it after that
measurement.

**Two layout floors are tight and were derived, not designed.** `KEY_COL` and
`BTN_SM` exist only because the App combo's width reached zero inside
`MIN_WIDTH`; the comment on `tok::KEY_COL` carries the arithmetic. At
`MIN_WIDTH` the App combo is about 59 px, which is cramped.

**Enablement is all-or-nothing and reads oddly.** With no row selected, the
whole strip greys; a disabled `CBS_DROPDOWNLIST` keeps a white field and dark
text (§56), so it looks live beside greyed labels. That is the theme, not a
bug — but it is why the row does not look coherent.

**The notes strip is the only feedback surface**, and it now carries the
probe's verdicts, the row's problems, and the capture hints. Nobody has looked
at what several of those at once reads like.

**Nothing has been seen at 96 DPI.** Every measurement in this project is at
150 %. The token constants are written in 96-DPI units, so the base case is
the untested one.

## 2. Known-wrong, and it needs a person at the keyboard

**Holding a modifier while clicking `Record` with the mouse records the wrong
chord.** Hold `Ctrl`, click `Record`, press `Alt+T` → it records `alt+t`. The
hook never saw the Ctrl-down, so the held set does not contain it. §F.4's
`GetAsyncKeyState` union at commit is the fix and was deliberately deferred:
`capture::step` is pure and cannot read live key state, so the union has to be
supplied by the caller.

No stuck modifier — `Outcome::PassThrough` covers the key-up — but a silently
wrong chord, which is worse than a refusal.

## 3. Not measured

- The refusal paths: a bare key, an unnameable key, a reserved chord. Only the
  happy path and the two endings (commit, close-while-armed) were injected.
- `MessageBeep`'s asynchrony, taken from MSDN. If it blocks on a machine with
  no audio device, it blocks the hook's thread.
- Esc-cancelling an open key list: comctl32 restores the prior selection and
  is not documented to notify, so the model could keep an arrow-visited key.
- `Ctrl+Alt+Del` and `Win+G` in §F.5 — a14 has no Game Bar installed
  (`Microsoft.XboxGamingOverlay` absent), and `Ctrl+Alt+Del` was judged too
  disruptive to a live session for the one measurement it adds.
- Tab order and Enter through the new strip, by real keypress.

## 4. Deferred defects — real, judged not to block

- **A chord beckon itself still holds can be reported as `Taken`.** A row
  deleted or edited away from its saved chord, and not yet saved, leaves
  `serve` holding the registration while nothing in the model names it. The
  saved-chord relaxation at `probe_plan` step 4b narrows this; it does not
  close it. Closing it needs the probe to read `ServeState::shortcuts`, which
  §F.6 has no verdict string for.
- **A bare `VK_CAPITAL` selects the wrong hint.** It reports
  `Refusal::NoModifier`, so the user is told to hold Ctrl, Win or Alt — advice
  that cannot work for a lock key.
- **A modifier-less chord reaches `RegisterHotKey`.** `Combo::parse("t")`
  succeeds; the at-least-one-modifier rule belongs to `caps_hold` alone. It
  briefly grabs a bare letter. `register_all` does the same, so it is
  consistent rather than new.
- **While a capture is armed, a Caps tap toggles the lock through a
  synthesized stroke** rather than the real one. Transient and inherent to one
  hook serving two features.

## 5. The lesson this landing paid for six times

A first measurement reported a defect that a second one dissolved, six times
in two days:

1. Gate B asked last week's question after a design change (§44).
2. Gate A's control was contaminated by the previous test's cleanup (§50).
3. A stray keypress during a guided run looked exactly like the `BN_SETFOCUS`
   defect the run existed to find (§52).
4. A disabled `CBS_DROPDOWNLIST` looked enabled beside greyed labels (§56).
5. `"Recor&d"` does not contain `"Record"` — the mnemonic ampersand (§64).
6. `window closed: False` was the unsaved-changes prompt working (§64).

None was a product defect. **When a hardware probe reports a failure, suspect
the probe until its own controls say otherwise** — and build the probe so its
controls can say so.

The mirror of that: **seven plan defects** were found by implementers and
reviewers rather than by me, including two contradictory tests in one brief, a
three-way mnemonic collision, a row dropped from the very table that guards
against collisions, and a commutativity claim produced by an experiment whose
fixture could not have falsified it. Every one was flagged rather than
silently resolved, which is the only reason they are written down.
