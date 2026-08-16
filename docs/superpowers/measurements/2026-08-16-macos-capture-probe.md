# Chord capture on macOS — the four questions, measured

**Machine: `airm3`** (MacBook Air M3), macOS 26.5.2, arm64. Run in **kitty**,
which is the terminal holding TCC on this host — Terminal.app has no rows here
at all. `org.nixos.kanata` was **running throughout** and does not interfere:
capture records an arbitrary chord and never touches Caps, which is the one key
kanata claims.

Probe: `crates/beckon-macos/examples/capture_probe.rs`, run twice —
`capture_probe` (observe) then `capture_probe swallow`. The two runs are the
control pair; a tap that receives nothing and a tap that suppresses everything
look identical from outside.

---

## Q1 — ordinary keys carry a keycode `key_table()` knows

**Yes, every one.** The probe did the lookup per event:

| Carbon | name | Win32 vk |
|---|---|---|
| `0x00` | a | `0x41` |
| `0x78` | f2 | `0x71` |
| `0x31` | space | `0x20` |
| `0x30` | tab | `0x09` |
| `0x7E` | up | `0x26` |
| `0x35` | escape | `0x1B` |
| `0x0C` | q | `0x51` |
| `0x24` | return | `0x0D` |

**So `capture::step` is REUSABLE, not forked.** `KeyDef` already carries both
`mac: u16` and `win: u32`, so macOS needs a projection layer and nothing more.
This is the single biggest result: `caps::decide` had to be duplicated for this
platform (`KeyEvent` has no `Edge` for a lock key, and `time_ms` is a different
clock), and the pre-measurement assumption was that capture would have to fork
the same way. It does not.

## Q2 — `flagsChanged` reports which edge a modifier took

**Yes, readable directly from the flag bit.** Holding three and releasing them:

```
ctrl    DOWN   mods ctrl
option  DOWN   mods ctrl+opt
command DOWN   mods ctrl+cmd+opt
command UP     mods ctrl+opt
option  UP     mods ctrl
ctrl    UP     mods -
```

The flags carry state *after* the event, so the bit belonging to the key that
just changed gives the edge. **Unlike Caps** — `caps_tap` tracks parity because
suppression freezes the lock the flag reports, and these are not locks. `step`
needs an `Edge` and a live `Mods`; both come straight out of this.

## Q3 — the system chords are visible and suppressible

`Cmd+Space`, `Cmd+Tab`, `Ctrl+Up`: all seen, all swallowed, **none acted**
(confirmed by the person at the keyboard — Spotlight did not open, the switcher
did not appear, Mission Control did not activate).

Two pieces of evidence the machine produced on its own:

- **Terminal echo disappears.** In the observe run the terminal printed `a` and
  F2's escape sequence `^[OQ` between the probe's own lines. In the swallow run,
  same keys, **no echo at all**.
- **`Cmd+Tab`'s event order changes.** Observe: `tab down (cmd)` → `command UP`
  → `tab up (mods -)` — the modifier is released *between* the two, which is
  App Switcher holding the session. Swallow: `tab down` → `tab up` → `command
  UP`, the ordinary shape, because nothing intervened.

## Q4 — nothing seen-but-not-suppressible, which INVERTS the Windows rule

`Cmd+Q` and `Ctrl+Cmd+Q` were both swallowed. The proof is that the log
continues past them:

```
key down 0x0C  mods cmd        -> "q"     <- Cmd+Q
key down 0x0C  mods ctrl+cmd   -> "q"     <- Ctrl+Cmd+Q
… a, f2, ctrl/opt/cmd, Cmd+Space, Cmd+Tab, and the run's own summary
```

If `Ctrl+Cmd+Q` had gone through, the screen would have locked and those lines
would not exist. If `Cmd+Q` had gone through, kitty would have quit and there
would be no output to read.

**The control for this was an accident, and it is the cleanest one in the
series.** The probe's first run had no Input Monitoring row of its own, and
`Cmd+Q` closed kitty. Same chord, same machine, same binary path, one variable
different. Without the grant the key goes through; with it, it does not.

### What that means for `is_reserved`

Windows block-lists `Win+L` because the hook **cannot** stop it — writing a
binding that can never fire is a lie. macOS stops `Cmd+Q` and `Ctrl+Cmd+Q`
**too well**, so a list is still needed, for the opposite reason: someone who
binds `Cmd+Q` by accident loses the ability to quit any application while
beckon is recording.

Not yet measured, and they belong on the list of things to try before shipping:
`Cmd+Opt+Esc` (Force Quit), and whether anything below the window server —
Touch ID, the power button — is reachable at all. A tap sees keys; those are
not keys.

---

## The trap that cost the first run

**Input Monitoring is per-BINARY and is NOT inherited from the terminal.**
Accessibility *is*: `beckon doctor` inside kitty reports kitty's grant. TCC
keeps a `kTCCServiceListenEvent` row per binary path, and after the failed run
`/Users/kln/beckon-test/beckon` had one while `capture_probe` had none:

```
kTCCServiceListenEvent | /Users/kln/beckon-test/beckon | 2     <- present
                       | …/capture_probe                       <- no row
```

So `CGEventTapCreate` returned a live port and the tap received **nothing**,
which is indistinguishable from "the system let everything through". That run
looked exactly like a Q4 result and was worth nothing.

`IOHIDCheckAccess` only asks and **never prompts**, so a binary with no row
cannot acquire one through it. The probe now calls `IOHIDRequestAccess` and
refuses to measure without the grant.

**This matters for the feature, not just the probe: every fresh `cargo build`
is a new binary, so the Caps tap loses Input Monitoring on every rebuild** —
the same shape as Accessibility losing its code-signature identity, but a
different pane and a different mechanism.

## Two hazards for the implementation, seen in the logs

- **An orphan key-up arrives when the tap starts mid-keystroke.** The swallow
  run opened with `key up 0x35 ("escape")` and no matching down — the Escape
  that stopped the previous run. `capture::step` already handles it:
  `st.release(vk)` returns false when the key was not held.
- **The escape hatch must only match key-DOWN.** That same orphan up would
  otherwise have stopped the run before it began.
