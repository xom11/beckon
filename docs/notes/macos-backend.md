# macOS backend — measurements and maintenance notes

Extracted from `CLAUDE.md` 2026-08-17. Read this before changing anything in
`crates/beckon-macos/`. Every "measured on airm3 / macmini" claim here is
scoped to **that machine's OS** — re-run the probe rather than the reasoning
before carrying any of it to another platform.

## `serve` runs `[NSApp run]`, not Carbon's `RunApplicationEventLoop`

Changed 2026-08-16, and it is why the macOS settings window had never worked.
The macOS tray design's §5 told the implementer to settle this before writing
any view code. It was not settled; `tray.rs` and `settings_window.rs` were
written anyway, and the file's own module doc said "nothing in this file has
been seen on screen" for four days without anyone asking whether it *could* be.

Measured with `crates/beckon-macos/examples/loop_probe.rs` — one view
hierarchy, two loops chosen by argv, so the difference in the output IS the
result and cannot be a difference in the thing under test:

```text
nsapp  : isRunning=true   the button's action ran
carbon : isRunning=false  it never ran
```

`NSApplication` is *instantiated* under the Carbon loop, because `NSStatusBar`
requires it to exist — and that is exactly what made this invisible: the tray
drew, so the window was assumed to be fine. But nothing ever calls
`[NSApp run]`, so nothing calls `[NSApp sendEvent:]`, so nothing drains the
queue that routes a mouse event to a window and thence to a view. **Every
control in the window was decoration.**

**NARROWED 2026-08-17: that pair was measured with the probe's DEFAULT press
method, and the default cannot produce the strong claim.** `PRESS=post` is
`postEvent:atStart:`, which enqueues onto `NSApplication`'s **own** queue —
drained only by `[NSApp nextEventMatchingMask:]` from inside `[NSApp run]`. So
the `carbon` leg's `it never ran` follows from the `isRunning=false` printed
beside it *by construction*; it is a restatement, not an observation about a
click. The probe was honest about this in its module doc and silent about it in
its OUTPUT, which is the half a reader quotes. It now prints
`PRESS: <mode> -- measures …` and words every `VERDICT` line for the mode that
produced it.

What each half rests on, kept apart because they are not the same strength:

- *Controls work under `[NSApp run]`* — **measured with real events.** The
  `nsapp` leg is a genuine positive control (the same enqueue, delivered), and
  `testing/macos_settings_drive.lua` later drove the shipped window with
  `hs.eventtap`, which is the window server rather than an in-process queue.
- *They were decoration under the Carbon loop* — **inferred from the
  mechanism**, plus the tautological `post` leg. A real click through that loop
  has not been run. `PRESS=hid` or `PRESS=external` is the run that would carry
  it, and both need an Accessibility grant for whatever launches the probe.

The inference is a strong one and nothing here disputes the conclusion —
`sendEvent:` is the only thing that routes a mouse event to a view, and nothing
was calling it. But this repo's rule is that reasoning and measurement are
labelled differently, and this entry had them labelled the same.

The Carbon loop survives as `HotkeyManager::run_carbon_event_loop_for_probe`
and nothing in beckon calls it. Deleting it would make the finding above
unfalsifiable — and would also delete the only way to ever run the `hid` leg
that upgrades it.

**The hotkey half is measured too, indirectly**, by
`examples/carbon_queue_probe.rs` with the Carbon loop as the baseline in the
same run: `carbon : DISPATCHED` / `nsapp : DISPATCHED`. It installs a handler
on `GetApplicationEventTarget()` — the target `RegisterEventHotKey` installs on
— posts an event of its own class to the main event queue, and asks whether the
handler ran. Both loops deliver, so `[NSApp run]` pumps the Carbon application
event queue.

**And directly.** Once Terminal.app was granted Accessibility,
`examples/hotkey_loop_probe.rs` driven by `examples/hid_key.rs` posted a real
chord through the window server, baseline first: `carbon : HOTKEY FIRED` /
`nsapp : HOTKEY FIRED`. The queue probe now stands as the *explanation* rather
than as the evidence.

## A synthetic chord needs the modifiers posted as REAL KEY EVENTS

`CGEventCreateKeyboardEvent(code)` + `CGEventSetFlags(ctrl|opt|shift)` is the
obvious spelling and it does **not** fire a `RegisterEventHotKey` chord:
measured 2026-08-16, it posted successfully — `AXIsProcessTrusted: true`, no
error anywhere — and nothing happened under EITHER loop. The flags field
*describes* an event; it does not hold a key down, and the system tracks
modifier state from `kVK_Control` &co. key events. The working sequence is the
one a hand makes: each modifier down carrying the flags accumulated so far,
then the key down and up, then the modifiers up in reverse
(`examples/hid_key.rs`).

**The baseline is what caught it.** A flags-only injection failing under
`nsapp` alone would have read as "the loop change broke hotkeys" and got
`run_forever` reverted for no reason; it failed under the Carbon loop too,
which is the one that demonstrably delivers hotkeys in production, and that is
what said the injector was wrong rather than the thing under test.

## Three defects a person found by looking

2026-08-16, none reachable by any assertion in the tree. `settings_drive`
reported 10/10 the whole time and was right; these are what it structurally
cannot ask.

### A menu bar image must be a TEMPLATE

The status item drew the word `beckon` while every neighbour was a glyph. It is
an embedded template with `setTemplate(true)`, without which it does not invert
for a light menu bar or survive increased contrast. Clear the title explicitly
— an `NSStatusBarButton` given both draws the icon AND the word.

**CORRECTED 2026-08-17: it was the SF Symbol `b.square.fill`, and that symbol
draws a capital `B`.** This entry used to end *"`assets/beckon.ico` is not
usable here: a Windows container, and a coloured bitmap cannot be a template"*,
and `tray.rs` carried the matching claim that the symbol "is the same shape as
the About door's mark". Both halves are wrong in the way this repo keeps
recording: plausible, written down, never run. Rendered through
`NSImage(systemSymbolName:)` — the same call `tray.rs` made — and looked at:

| | menu bar | every other `b` in beckon |
|---|---|---|
| letter | **`B`** upper case | **`b`** lower case |
| ratio | 184x166 = **1.11:1** | 1:1 |

The control that rules out a renderer artifact: `a.square.fill` draws `A` at
the same 1.113 ratio, and `b.square.fill.lowercase` is ABSENT — the SF Symbols
letter family is upper case by design, with no lower-case member. The other
four surfaces are `assets/beckon.ico`, `site/favicon.png` + `icon-512.png`,
`cap::MARK` (Windows About) and `heading("b")` (macOS About), all lower case.

**What was salvageable is the ARTWORK, not the file.** `.ico` really is a
container nothing here reads, but `tools/make-menubar-mark.py` derives
`assets/beckon-menubar.png` from its 256px frame, so the menu bar and the
Windows tray cannot show two different letterforms. Two things that script must
keep doing:

- **Knock the letter out by WHITENESS, not by colour equality.** The tile is a
  gradient (`#3B82F6` to `#2563EB`), so there is no one blue to compare
  against; luminance separates the families and turns anti-aliased letter edges
  into partial alpha instead of a stair-step.
- **Round the tile, at `8.0/34.0` — the About door's own `cornerRadius`.**
  `beckon.ico` is FULL-BLEED: measured, its corner pixels are opaque `#3B82F6`,
  not transparent. That is correct for a Windows icon, where the shell applies
  its own shape, and wrong here, where nothing does — a solid 14x14 black
  square in the menu bar is a blob.

**Sizing is 17x17, and matching the old symbol was the wrong target.** The
first cut was 14x14, on the reasoning that `b.square.fill` occupied 15x14
(measured, with no symbol configuration — which is how `tray.rs` called it) so
the item would not change width. A user reported the result as small beside
their other menu bar tools, and measuring the neighbours says why. Apple's own
extras, at default size on a 22 pt bar:

```text
wifi 17x13   battery.100 22x11   speaker.wave.2.fill 19x14
airplayaudio 15x15   bolt.fill 13x17   moon.fill 15x15   display 19x15
```

**The complaint is about WIDTH, not height.** At 14 the mark was the
*narrowest* object in the bar while neighbours run 13–22 pt wide, and a menu
bar is a horizontal strip, so extent is what reads as size. Height at 14 was
already mid-range — which is exactly why "it looks small" and "it is shorter
than average" are different claims, and only the first one was true. 17 is the
tallest neighbour (`bolt.fill`) and `wifi`'s width.

**18 was rendered against those neighbours and rejected** as visibly the
largest object in the bar. What lets the tile hold 17 where an outline glyph
could not is ink: 78% coverage of its box against `wifi`'s 26% and
`battery.100`'s 45%, measured — and that is the same fact that caps it there.
The source is 34x34, exactly @2x, pixel-perfect on a Retina display rather than
resampled; `tools/make-menubar-mark.py`'s `PT` and `tray.rs`'s `setSize` are
the pair that must move together.

**The size-comparison probe had a bug worth not repeating**: it tinted the
template with `r.fill(using: .sourceAtop)` *in place* on the bar. Source-atop
respects the DESTINATION's alpha, the bar had just been filled opaque, so every
icon came out a solid black rectangle — neighbours and mark alike, at every
size, which reads as a plausible sheet rather than as a failure. Tint on a
transparent canvas first, then composite.

**`setTemplate(true)` became load-bearing in the same commit.** An SF Symbol
already answers `isTemplate == true` before anything sets it (measured), so
that line was a no-op the whole time it was there; a PNG answers `false`, and
without the call the mark would not invert.

**What no assertion in this tree can catch is the letter.** A probe can say the
asset decodes, is 28x28 square, and carries tile + knockout + anti-aliasing in
its alpha — all checked — and every one of those passed for the capital `B`
too. The instrument is a person looking.

### `makeKeyAndOrderFront` is not enough for an Accessory app

`hotkey::install` puts `serve` in the Accessory activation policy, where an
application is never frontmost on its own, so the settings window came up
BEHIND whatever the user was in, with grey traffic lights and fields that took
no keys — indistinguishable from "clicking Settings did nothing".
`NSApplication::activate` asks for the app; `makeKeyAndOrderFront` asks for the
window. Both, in that order. `settings_window::raise` is the one place that
knows it.

### The window's catalog must be what the RESOLVER can find

`row_condition` prints `missing` against the catalog, which was
`installed_apps()` alone — `/Applications`, `/System/Applications`,
`~/Applications`. `Finder` lives in `/System/Library/CoreServices`, so a
working `ctrl+super+alt+f = "Finder"` sat flagged while `beckon resolve Finder`
answered *resolved — running app localizedName (exact)*. **The window was
contradicting the resolver about one name.** `installed_app_names` now unions
the RUNNING apps, which is the tier `resolve` matched on and is resolvable by
definition, so the catalog cannot start over-claiming. Widening the scan roots
was rejected: CoreServices is mostly unlaunchable helpers, and it would change
what `beckon installed` prints — a different surface with a different job, and
now the control (`examples/catalog_probe.rs`).

**CORRECTED 2026-08-17: that control was itself over-claiming, in the same
direction as the defect it watches for.** It answered with its own
`eq_ignore_ascii_case` over `installed_app_names()` while `row_condition` had
grown a **substring tier** — the `Certainty::Guess` tier `check --resolve`
passes deliberately. Measured on airm3 over the same 109-name catalog in one
session: the copied rule prints `Settings MISSING` where the window prints it
present with *"Matches \"System Settings\" by substring"*. A control whose rule
is a stale copy of the rule under test reports a defect that is not there and,
worse, would keep reporting it after the real one was fixed. It now builds a
one-row `Model` and reads `ControlState::items[0].flag` — the literal word the
window draws.

The tray MENU was never broken. Clicking the icon opens it; `Settings…` works.
There is no double-click-to-open path on macOS, which is why the window seemed
unreachable.

## The settings window used to SEGFAULT

The minimal reproducer is two steps, not three. Measured on airm3 2026-08-17
through `testing/macos_settings_drive.lua`, control first — `0.9.6 (45298e9)`,
the commit immediately before the lifecycle fix, verified to contain zero
occurrences of `setReleasedWhenClosed` / `NSWindowDelegate`:

```text
control 45298e9   PASS close #1 by the X   FAIL reopen from the tray (0 windows)   process GONE
fixed   51070f7   PASS close #1   PASS reopen   PASS close #2   PASS reopen again   alive
```

```text
EXC_BAD_ACCESS / SIGSEGV, KERN_INVALID_ADDRESS
  libobjc.A.dylib   objc_retain
  beckon …
  -[NSApplication(NSResponder) sendAction:to:from:]
  -[NSMenuItem _corePerformAction:]
```

**The commit message for the fix got the mechanism wrong and this is the
correction.** It said *"It must be closed TWICE in one session … the second
release lands when the `Retained` drops"*, i.e. blame the second CLOSE. The
stack says the first close deallocates the window (`releasedWhenClosed`
defaults to `YES` for `initWithContentRect:`) and the crash comes on the
**reopen**, retaining freed memory from the tray's own `Settings…` action. So
the reproducer is **close by X, then click Settings…** — and the distinction is
not pedantry: the two stories imply *different regression tests*, and one
written from "close twice" passes while the bug is live.

**A probe trap this cost a run to find**, the same shape as the
`AXTitle`/`AXDescription` one. The harness finds the process with
`a:name() == "beckon"`. The control was deployed as `/tmp/beckon-ctl`, so it
reported `FAIL serve not running` while `ps` plainly showed the process. **On a
control run that reads as "the control does not reproduce the bug"** — turning
a real segfault into a clean bill of health. A control binary must keep the
basename `beckon`.

## Caps: the `CGEventTap` arm

`beckon_macos::caps_tap` is the `CGEventTap` twin of
`beckon_windows::caps_hook`, against the same design: Caps is an **alias for
the configured chord**, so the tap swallows `Caps+T` and injects
`ctrl+cmd+opt+T`, which `RegisterEventHotKey` already listens for.
`examples/caps_live.rs`, with the tap uninstalled as the control:

```text
off : hotkey fired = false     <- nothing else on this machine maps Caps
on  : HOTKEY FIRED             <- beckon's alias did it
```

The control is not ceremony here: with kanata running the `off` run FIRES, and
the probe would be measuring kanata.

**NARROWED 2026-08-17: `off : hotkey fired = false` had a SECOND reading, and
it is the likelier one.** *Nothing else maps Caps* and *this process cannot
type at all* print the identical line: `CGEventPost` returns `void` and does
nothing whatever from a process that is not Accessibility-trusted, which is the
state every freshly `cargo build`-ed binary is in, because the grant is bound
to the code signature. `hid_key.rs` already refuses on that reading;
`caps_live` did not, so its control could not tell silence from blindness. Two
things close it, and both run in the same session as the result they carry:

- `AXIsProcessTrusted()` is printed and the probe REFUSES when false — in
  **both** modes, because an `on` run that cannot inject reports *the alias did
  NOT work*.
- Every run, after the Caps sequence, presses `ctrl+cmd+opt+T` **directly** with
  the modifiers as real key events. That must fire. `DIRECT CONTROL: hotkey
  fired = false` means the injector or the registration is the broken part and
  nothing in the run is about Caps.

The `off : false` above therefore stands only if it was taken from a run whose
direct control fired. Re-run it before quoting it.

**The edge is parity, not a flag.** Nothing in a Caps event distinguishes a
press from a release: **observed**, both arrive as `flagsChanged` carrying
identical flags, so there is no bit to read an edge off. That observation is
all the parity scheme needs, and it is deliberately stated without the
mechanism.

**CORRECTED 2026-08-17: this paragraph used to explain the observation, and the
explanation asserted the row the table below marks UNMEASURED.** It read *"both
arrive with identical flags because suppression freezes the lock the flag
reports, and `CGEventSourceKeyState` reports that same frozen lock for a lock
key"* — which is the "suppression stops the lock" claim wearing a causal
sentence, plus a second claim about what `CGEventSourceKeyState` reports that
`caps_tap.rs:24-26` contradicts. Neither was measured. **A cause quietly
restates the effect**, so a reader checking the UNMEASURED row and then reading
this paragraph would find the claim apparently settled two screens earlier.

The failure mode is unchanged and is the reason `resync()` exists: a dropped
event inverts the phase. `caps_tap::resync()` is called from every path that
can drop one — the tap being disabled by timeout or by user input, and any
configuration change, which is also a moment nobody is holding a key.

**`caps::decide` is NOT shared, and this is why**, beyond the edge: its
`KeyEvent` is `{ vk: u32, edge }` with a down and an up, and macOS has neither;
its `time_ms` is documented as `KBDLLHOOKSTRUCT.time` in milliseconds since
boot while `CGEventTimestamp` is nanoseconds of mach absolute time. What IS
shared is `caps::bound_keys_mac`, the Carbon-keycode sibling of `bound_keys`,
pinned to select the same bindings by
`the_two_projections_select_the_same_bindings`.

**Input Monitoring, not Accessibility.** It is a separate grant in a separate
pane, and `caps_tap::install` refuses with that sentence rather than installing
a tap that receives nothing. The Keyboard door says the same thing under its
first group, because it is the one thing a reader cannot discover by trying.

### The four facts the port rested on

`examples/caps_probe.rs`, 2026-08-16. The Windows Caps feature is an ALIAS: the
hook swallows `Caps+T` and injects `ctrl+win+alt+T`, because `RegisterHotKey`
cannot bind Caps. Four facts had to hold for that shape to port.

| | |
|---|---|
| a `CGEventTap` sees Caps | yes — as **`kCGEventFlagsChanged`**, never `keyDown`/`keyUp` |
| returning NULL suppresses it | yes |
| **suppression also stops the LOCK** | **UNMEASURED — see below. This row used to read "yes".** |
| the tap survived | no timeout in that run |

The third would say: beckon can take the key without the lock engaging, so
`caps_tap` can offer `capslock` / `escape` / `none` the way Windows does rather
than being stuck with whatever the OS did. It is still what the feature is
built on, and `caps_tap`'s `inject_plain(K_CAPSLOCK)` on the tap gesture exists
precisely because *"the lock did not move while it was swallowed"*.

**WITHDRAWN 2026-08-17. The verdict could not have come out any other way, for
two independent reasons:**

1. **The lock was read with the wrong instrument.**
   `CGEventSourceKeyState(_, kVK_CapsLock)` answers *is that KEY down*, and
   Caps is momentary — down for the instant of the press, up long before the
   probe sampled it a tick later. It read `false` before and `false` after
   whether suppression worked or not, so `after == caps_before` was a
   tautology. The trace columns in the same output said so from the other side:
   neither `keyState` column ever flipped, recorded at the time as *no
   discriminator* when it was *wrong instrument*. The lock is the `alphaShift`
   bit of `CGEventSourceFlagsState`, which is level rather than momentary.
2. **The driver presses Caps TWICE**, so a before/after pair is equal even for
   a lock that toggled on both. Only the sequence can tell "never moved" from
   "moved and came back", and the probe now samples once per tick.

**And there was no control**: a run where Caps is NOT swallowed, which must
show the lock moving. Without it a working reader and a blind one print the
same words. `caps_probe` takes an arm now:

```sh
cargo run -p beckon-macos --example caps_probe -- session pass      # control: lock MUST move
cargo run -p beckon-macos --example caps_probe -- session swallow   # the test
```

Re-run both, in that order, on a machine with kanata stopped, and put the
result back in the table. Do not restore the "yes" from the old output.

### `caps_tap = "capslock"` did nothing on macOS

A SECOND defect fell out of trying to measure the first, 2026-08-17. The arm
was `inject_plain(K_CAPSLOCK)`, i.e. `CGEventPost` of `kVK_CapsLock` — and that
does not move the lock on this platform. Measured with `hs.hid.capslock.get()`
as an INDEPENDENT reader (IOKit, sharing nothing with the event path or with
`CGEventSourceKeyState`):

```text
control: set(false)->false, set(true)->true      the lock moves, the reader sees it
post to kCGSessionEventTap  (what beckon did)    before=false  after=false
post to kCGHIDEventTap      (the only other)     before=false  after=false
IOHIDSetModifierLockState                        0 -> 1, KERN_SUCCESS
```

`AXIsProcessTrusted` was 1 for both posts, so this is not the silent no-op an
untrusted `CGEventPost` gives, and the posts came from a standalone C probe
byte-identical to `inject_plain` so nothing else of beckon's could explain it.
**The Windows entry — *"an injected `VK_CAPITAL` flips the toggle, so
`caps_tap = "capslock"` is implementable"* — is true, and true only there.**
The macOS port inherited the shape without re-measuring it, so a user ticking
the box lost the key and got nothing back.

`toggle_caps_lock()` uses `IOHIDSetModifierLockState` now, verified through the
real function by `examples/capslock_probe.rs`: `false -> true -> false`, with
the second toggle asserted because a stuck reader also returns two equal reads.
**Still unverified: the gesture end to end.** The tap's Caps arm fires on
`kCGEventFlagsChanged` and an injected `kVK_CapsLock` is an ordinary `keyDown`,
so no synthetic event reaches it — a physical Caps tap with `serve` running is
the only way, and it needs a person.

### `CGEventTapCreate` returning non-NULL is not evidence

The tap will not necessarily receive anything, and the probe now prints
`IOHIDCheckAccess` because of it. Input Monitoring is a *separate* grant from
Accessibility, with its own System Settings pane; without it the create call
still succeeds and then delivers nothing, silently.

### Do not use `f19` in a probe on this platform

**A synthetic keycode no physical key carries does not survive the trip.** The
probe's control was `F19` (`kVK_F19`, 0x50), chosen because nothing binds it.
It produced **zero events of any type** while `ctrl+opt+shift+f` through the
same injector in the same session produced eight — so the tap was live the
whole time and the CONTROL was the broken part. That false negative cost a
wrong suspicion of kanata, which was stopped for nothing. Use a key the
keyboard has, and chord it so it types nothing.

**It recurred on 2026-08-17, on macmini, and cost most of a session.**
`examples/chord_timing_probe.rs` registered `ctrl+super+alt+f19` — picked for
this entry's own stated reason, *nothing binds it* — and reported that a
synthetic chord fires `RegisterEventHotKey` at **no** delay and through **no**
`CGEventPost` tap. Every cell was an artifact of the key. Bisected one variable
at a time, cross-process `hid_key` in the same run:

```text
ctrl+opt+shift+f     BAN, both taps, gaps 0 / 3 / 20 ms
ctrl+opt+shift+f19   im lang, every cell
ctrl+cmd+opt+f       BAN everywhere
ctrl+cmd+opt+q       BAN everywhere      <- the user's own binding
```

So the modifier set, the tap layer and the clock were never variables. Not as
the subject and not as the control. `hotkey_conflict_probe` may keep it,
because *registration* is a different question from *delivery* and that probe
never presses anything.

Two rules that would have caught it in the first run:

- **A probe that registers a hotkey must prove the hotkey can fire at all**,
  with a positive control it did not produce itself — `hid_key` from a separate
  process. The reachability control that was there (`CGEventSourceKeyState`,
  "does my injection reach the window server") passed the whole time and is
  about a different layer.
- Reach for the control **before** reading the result, not after it looks
  interesting. The first table was quoted as a finding for three exchanges.

### The Caps alias is measured working end to end on macmini

2026-08-17, `examples/caps_synth_probe.rs`, against the real `beckon serve`.
This closes the "an injected chord does fire our own hotkey" row that Windows
has had since 2026-08-11 and macOS did not.

Injecting Caps the way `caps_live` does cannot reach the tap — `CGEventPost` of
`kVK_CapsLock` is an ordinary `keyDown` and the Caps arm fires on
`kCGEventFlagsChanged` — so the probe builds the right event: same keycode,
`CGEventSetType(.., kCGEventFlagsChanged)`, carrying `alphaShift`. **Its
detector is `caps_tap = "capslock"` itself**: a bare synthetic Caps must move
the LOCK, and the lock is the `alphaShift` bit of `CGEventSourceFlagsState` —
*not* `CGEventSourceKeyState`. Lock did not move ⇒ the tap never saw it ⇒
step 2 says nothing, and the probe exits rather than reporting a verdict.

```text
lock false -> true                       the tap sees a synthetic Caps
Caps+Q, hold 40 / 250 / 600 / 1200 ms    Finder, every time
Caps lock LIT at injection time          Finder
chord injected from a separate process   Finder
```

The last row is the load-bearing one: **the chord beckon injects from INSIDE
its own tap callback does fire `RegisterEventHotKey` here.**

### TWO `serve` processes silently kill each other's Caps feature

The per-config lock file cannot stop it. Measured on macmini 2026-08-17,
controls on both sides:

```text
one tap                 8/8  [++++++++]
TWO taps                0/8  [........]
one tap again           8/8  [++++++++]
```

`lockfile::acquire` hashes the CONFIG PATH into the lock's name — deliberate,
so an old and a new binary cannot both serve one file. But a `CGEventTap` on
Caps is a **machine-global** resource, so `serve A.toml` and `serve B.toml` are
two lock holders and two taps. **Both log `caps event tap active`**, which is
why this is invisible from either side.

The mechanism was measured rather than reasoned, by reversing the order:

```text
caps.toml installed FIRST   0/6   its tap is underneath
caps.toml installed LAST    6/6   its tap is on top
```

Every tap goes in at the head of `kCGSessionEventTap`, so **the tap installed
LAST is upstream and swallows every Caps `flagsChanged`** (`hook_proc` returns
null on that arm unconditionally — "the lock must not move under a hold"). The
other beckon therefore never sees Caps go down, `CAPS_DOWN` stays false, and
`Caps+<key>` falls straight through to the focused app. That is exactly the
reported symptom: kitty receiving a byte-correct chord while nothing focused —
**the chord in kitty came from the WINNING tap, and the loser's hotkey was
never involved.**

Two consequences worth keeping apart:

- **A second beckon is easy to have by accident.** Three of one session's own
  `serve` processes were found running at once, because
  `pkill -f 'bin/beckon serve'` does not match `~/beckon-test/beckon` — and a
  peer Claude session had a fourth from the same shared directory. Any probe
  that starts a `serve` must **assert the count**, not assume its own kill
  worked.
- **The Windows arm has the same shape and is not measured.** `caps_hook`
  refcounts its two REASONS inside one process (`capture::HookOwners`); nothing
  there is about a second *process*. `WH_KEYBOARD_LL` hooks chain rather than
  shadow, so the failure will look different — the capture entry predicts the
  second hook records the alias instead of the key — but two
  `beckon-serve.exe` on two configs is reachable the same way. Re-run this
  experiment there before assuming either outcome.

**Fixed in `f91daeb`, and the fix is the LOG LINE — the lock is only how it is
detected.** `lockfile::acquire_caps` takes a **fixed**-name flock (never the
per-config one, which must keep its own single job), `sync_caps_hook` takes it
immediately before `caps_tap::install_for`, and a beckon that cannot get it
installs nothing and prints *another beckon owns Caps on this machine; Caps
shortcuts are off here (the typed chord still works)*. **A beckon that declined
quietly would be the same defect one level down**, so a test that only counts
taps passes against it — which is why `testing/macos_caps_one_owner.sh` asserts
the refusal line, and why each of its arms declares the tap count *and* the
refusal count it expects before it measures anything. Measured after the fix,
controls both sides:

```text
one serve    (1 tap, 0 refusal)  8/8      <- was 8/8
TWO serves   (1 tap, 1 refusal)  8/8      <- was 0/8
one serve    (1 tap, 0 refusal)  8/8
```

The flock is released by dropping the `File`, so pause, a reload that turns
Caps off, and exit all hand Caps back; `sync_caps_hook` re-takes it on every
call, which is why the loser recovers with **no timer** — every one of those
paths already calls it.

**Windows deliberately does NOT take this lock**, and the comment on that arm
says so with the recipe to close it: two `beckon.exe serve` on two configs, one
Caps binding each, press both. `WH_KEYBOARD_LL` chains rather than shadows, so
the failure *looks* likely and looking is not measuring — which is precisely
how `caps_tap = "capslock"` shipped dead on macOS.

**Both scripts stay.** `macos_two_caps_taps.sh` measures the DEFECT (and so
keeps it falsifiable — it must still print `0/8` against a beckon built before
`f91daeb`); `macos_caps_one_owner.sh` checks the FIX. Neither is the other's
replacement, and the middle arm means the opposite thing in each.

Two counting traps, both now written into the scripts:

- `pkill -f 'bin/beckon serve'` does not match `~/beckon-test/beckon`.
- `pgrep -f` matches the two `sudo` wrappers as well, so one serve counts as
  **three** and every premise check fails at exactly three times the truth.
  Count by `comm`.

This closes the "what is still unexplained" note that stood here: a live run
where kitty received `^[[113;15u` — `q` with ctrl+alt+super, byte-correct —
while Finder never came up. It reproduces on demand with two taps and never
with one (10/10 clean, with an external parity read that never showed an
inversion, so **parity was not the cause** and `toggle_caps_lock`'s missing
`INJECTING` guard is a separate, still-unmeasured question).

### kanata already implements beckon's Caps feature

`~/.nix/configs/kanata/main.kbd:52` is `caps (tap-hold 200 200 esc @cap_alias)`
and `kanata_macos.kbd:19` is `cap_alias (multi lmet lctl lalt)` — Caps tapped
sends Escape, Caps held sends Cmd+Ctrl+Option, which is beckon's own hyper
chord and its `caps_tap = "escape"` option. Anyone testing beckon's Caps
support on this machine must stop `org.nixos.kanata` first or they are
measuring kanata.

## `RegisterEventHotKey` accepts a chord the SYSTEM holds

So macOS has no availability probe worth building on what has been measured —
and the case the headline is about was not one of them. This entry used to open
*"`RegisterEventHotKey` does NOT report a chord another application holds"*;
**NARROWED 2026-08-17**, because the run below contains no other application.
Measured 2026-08-16 with `examples/hotkey_conflict_probe.rs`, in an Aqua
session, control first:

```text
Ctrl+Cmd+Opt+F19            ACCEPTED   <- control: registration works here
Ctrl+Cmd+Opt+F19 (again)    REFUSED    <- OSStatus -9878, same process
Cmd+Space   (Spotlight)     ACCEPTED
Ctrl+Up     (Mission Ctrl)  ACCEPTED
```

Carbon refuses a duplicate **within one process** (`eventHotKeyExistsErr`) and
happily accepts a chord the system owns. So the sixth step of `probe_plan` —
*ask the OS* — has nothing to ask on this platform, and `serve.rs`'s `AskTheOs`
arm returning without a verdict is correct: a successful registration would be
a guess dressed as a measurement. The five steps before it all still run, and
they are the ones that catch real mistakes. The same-process refusal is not a
fallback signal either — "another row in this file already uses it" is step
four, which core answers before that arm is reached.

**What is still unmeasured, and it is the case the old headline named: a chord
held by another ordinary application.** The four lines above are a duplicate
inside ONE process and two chords the SYSTEM owns. Neither stands in for the
third case, and the middle two are weaker than they look — Spotlight and
Mission Control are not registered through `RegisterEventHotKey` at all, so
Carbon accepting them says nothing about what it does between two clients OF
it. A user's conflict is *skhd / Raycast / Hammerspoon already has
`ctrl+cmd+opt+T`*, which is exactly the untested shape.

`hotkey_conflict_probe` grew **case 4** for it: it re-executes itself as a
`holder`, which registers `Ctrl+Cmd+Opt+F19` and prints `HOLDER READY` before
parking on the event loop; the parent then unregisters everything of its own —
otherwise the refusal is the same-process case again wearing a different label
— attempts the same chord, kills the holder and attempts once more, which
**must** be accepted. Not yet run: an agent shell is in the `Background`
bootstrap namespace and the probe refuses there. Run it from Terminal.app and
record the `CROSS-PROCESS:` line.

**The conclusion is unchanged either way and that is not luck.** If case 4
comes back ACCEPTED, `AskTheOs` staying silent is confirmed. If it comes back
REFUSED, macOS gains an availability probe that works for the conflicts users
actually hit — a strictly better outcome, and one nobody would have gone
looking for while this entry read as settled.

## The Accessibility grant is pinned to the BUILD, not to the path — REFUTES a claim this session made and shipped into two repos

**Measured 2026-08-17 by reading TCC directly on both Macs.** Do not re-derive the
refuted version; it is believable, it was written down twice, and it is wrong.

### The claim that was wrong

While moving the author's Macs from a nix-store beckon to Homebrew, this session
told the peer session — and the peer wrote it into `xom11/nix` — that Homebrew ends
the re-granting problem because `/opt/homebrew/opt/beckon/bin/beckon` is a stable
path and the grant hangs off it. **The reasoning never touched TCC.** It was
inferred from the correct half (nix store paths change every bump) plus an
assumption about what TCC keys on.

Ironically the ORIGINAL comment in that repo was already right — *"Homebrew khong
sua duoc dieu do (Cellar cung mang so phien ban)"* — and this session talked past
it. The peer reverted their own additions in `6d36ad41` and left the correct
sentence alone.

### What TCC actually stores

Accessibility lives in the **system** database, `/Library/Application
Support/com.apple.TCC/TCC.db`, not `~/Library/...`. A first attempt read the user
DB, found no Accessibility rows at all, and could have been reported as "no grant
exists".

```text
sudo sqlite3 "file:/Library/Application Support/com.apple.TCC/TCC.db?mode=ro" \
  "select auth_value, datetime(last_modified,'unixepoch','localtime'), client
   from access where service='kTCCServiceAccessibility' and client like '%beckon%'"

  2  17:22:30  /opt/homebrew/Cellar/beckon/0.9.14/bin/beckon
  0  23:17:32  /opt/homebrew/Cellar/beckon/0.9.15/bin/beckon
```

Two things kill the claim, and either one alone is enough:

1. **The client is the versioned CELLAR path**, not the `opt` symlink the launch
   agent invokes. TCC resolves the link and records the real file, so a stable
   `opt` path has nothing to do with where the grant is stored.
2. **The designated requirement is a content hash.** `codesign -d -r-` on the
   shipped binary returns `designated => cdhash H"897ba27c…"`, with
   `Signature=adhoc` and `TeamIdentifier=not set`. Every release is a new cdhash,
   so even a fixed path could not carry a grant across a bump.

So Homebrew and the nix store are **equivalent** on this axis. What ends it is any
signature that is not ad-hoc — **including a free self-signed certificate**, which
was measured rather than assumed; see the next section.

### `auth_value = 0` is DENIED, and it burns the ability to ask

The 0.9.15 row was written `0` at the exact second a `brew services restart`
produced the new process. `0` is denied, not "not yet asked" — and CLAUDE.md
already records that **macOS raises the panel only when no answer is recorded**.
So `request_accessibility()` will never prompt again for that cdhash: the ask
consumed its own future.

That is worth stating as a rule, because the API reads as harmless: **calling the
prompting variant from a launchd agent, on a machine with nobody in front of it,
can record a denial.** The grant then has to be added by hand in System Settings.

### The log CANNOT be used to diagnose this, in either direction

`Accessibility is not granted` is printed at startup and the grant-watch tick may
flip it to `granted; restarting` seconds later, so a snapshot of the tail says
nothing durable. This session got it wrong **twice in opposite directions** in one
hour: first reporting a persistent failure from a transient line, then accepting
the peer's "it is always transient" generalisation — measured on airm3 — and
applying it to macmini without re-measuring, where the last word really was
`not granted`. The second mistake is this repository's oldest theme: a measurement
on one machine is data about that machine.

**Read TCC.db. The log is a startup snapshot and beckon does not re-log.**
`beckon doctor` cannot answer either — it reports the CALLER's TCC, never the
launchd agent's.

### The grant follows the BUNDLE IDENTIFIER, and that is the half signing could not buy

**Signing was necessary and not sufficient, and v0.9.17 is the proof.** It was
signed identically to v0.9.16, with the same certificate and the same designated
requirement, and it lost the grant anyway:

```text
2 | 00:07:39 | /opt/homebrew/Cellar/beckon/0.9.16/bin/beckon   <- granted
0 | 00:22:40 | /opt/homebrew/Cellar/beckon/0.9.17/bin/beckon   <- new row, denied
```

Nobody touched System Settings. **Homebrew's Cellar path carries the version**, so
a path-keyed grant cannot survive an upgrade no matter who signed the binary. A
Developer ID would have failed the same way; the 99 USD buys notarisation, not
this.

The mechanism is one column in the TCC database, and reading it settles the whole
question:

```text
select auth_value, client_type, client from access
where service='kTCCServiceAccessibility'

  client_type = 0  (bundle identifier)   33 rows — every ordinary app
      com.raycast.macos, com.knollsoft.Rectangle, org.hammerspoon.Hammerspoon,
      net.kovidgoyal.kitty, com.microsoft.VSCode, org.pqrs.Karabiner-*, …
  client_type = 1  (absolute path)        6 rows — every bare CLI binary
      /opt/homebrew/Cellar/beckon/0.9.15/bin/beckon
      /opt/homebrew/Cellar/kanata/1.12.0/bin/kanata
      /usr/bin/env, /usr/libexec/sshd-keygen-wrapper, AEServer
```

Ordinary apps are keyed by identifier, which is why granting Raycast once is
enough. beckon was a bare binary, so it was keyed by path. `kanata` is in the same
trap, with `1.12.0` in its path.

**A self-signed bundle is enough**, measured on macmini 2026-08-18 from a clean
slate (no beckon rows existed):

| step | result |
|---|---|
| run a signed `.app`, ad-hoc-free but NOT Developer ID | TCC writes `0 \| 0 \| com.xom11.beckon.bundletest` — **`client_type = 0`** |
| owner grants it | `auth = 2` |
| replace the binary INSIDE (0.9.15 → 0.9.17, cdhash `e557cb76…` → `50d1c3b8…`) | — |
| **and move the whole bundle to another directory** | — |
| read again | **`granted`** |

So the bundle identity defeats both failure modes at once: content change and path
change. Free.

**Bundle identity does not need `open`.** The launchd job pointed straight at
`beckon.app/Contents/MacOS/beckon` and TCC still recorded the identifier — which
is what lets one file serve as both the CLI on `PATH` (a symlink into the bundle)
and the agent, under one identity and one grant.

### PROVEN end to end, through `brew upgrade` rather than a path of my own making

The bundle test above swapped a binary under a path this session controlled, which
is exactly the shape that made the 0.9.17 prediction wrong: it proved something
true of that setup and not of the one beckon ships through. So it was re-run on
the real mechanism.

macmini, 2026-08-18. 0.9.18 is the first release installed as a `.app`; the owner
granted it once at 09:19:37. Then 0.9.19 -- version bump only, no code change --
was released and installed with `brew upgrade`, with nobody opening System
Settings:

```text
BEFORE  2 | 0 | 09:19:37 | com.xom11.beckon
        running .../Cellar/beckon/0.9.18/beckon.app/Contents/MacOS/beckon
AFTER   2 | 0 | 09:19:37 | com.xom11.beckon      <- identical, timestamp included
        running .../Cellar/beckon/0.9.19/beckon.app/Contents/MacOS/beckon
        lsappinfo: bundleID="com.xom11.beckon" type="UIElement" Version="0.9.19"
        log: "Accessibility granted"
```

**macOS did not even touch the row.** The Cellar path changed and the binary
changed; the grant did not care, because it is not keyed on either. That is the
thing 0.9.17 could not do with an identical signature.

Scope, stated so it is not over-read later: this is about the SERVICE, the process
launchd starts. `beckon <id>` typed into a terminal is still attributed to the
TERMINAL as the responsible process, so it uses the terminal's grant -- unchanged
by any of this, and true before the bundle as well.

### `LSUIElement` and `TransformProcessType` collide, and the warning was the casualty

Declaring `LSUIElement` puts the process in the accessory state at launch, so
`serve`'s `TransformProcessType` is then asked for a state it already holds and
answers paramErr (-50). Measured: the call failed while `lsappinfo` reported
`type="UIElement"` for the same pid — the goal was met and the failure was
cosmetic.

Cosmetic but not harmless. The line it printed —

```text
hotkey: TransformProcessType failed: OSStatus -50 (hotkeys may not fire under launchd)
```

— is the exact sentence somebody greps for when hotkeys really do fail, and it
would have printed on every start of every bundled build. `hotkey.rs` now checks
the END STATE instead of the return code: it skips the transform when already an
accessory, and warns only if the process is still not one afterwards. Verified by
running the same bundle before and after: the line appears, then does not, with
`type="UIElement"` and the shortcut registered in both.

### A free self-signed certificate is enough — measured, with both controls

**Measured on airm3 2026-08-17.** Worth settling because the alternative costs
99 USD a year, and it is not needed for the grant to survive.

| # | configuration | result |
|---|---|---|
| 1 | ad-hoc, two releases | DR **is** the cdhash — different per build |
| 2 | one self-signed cert, 0.9.14 and 0.9.15 | DR **identical**: `identifier "com.xom11.beckon" and certificate leaf = H"e6b273ea…"` |
| 3 | fresh cert, never granted | `NOT granted` — **the probe can report negative** |
| 4 | owner grants 0.9.14 (`cdhash ff7a4705…`) | TCC writes `auth=2` |
| 5 | swap in 0.9.15 (`cdhash c29ae4bb…`), same path, same cert | **`granted`** |
| 6 | same path, same identifier, DIFFERENT cert | `NOT granted` |

Row 5 is the finding. Rows 3 and 6 are what make it mean anything: without 3 the
probe could have been blind, and without 6 the result could have been "TCC keys on
the path". **The grant follows the CERTIFICATE.**

A self-signed cert does NOT buy notarisation, so a browser-downloaded release is
still quarantined — the separate defect already recorded. Homebrew fetches with
curl and is unaffected, which is how every Mac here installs.

Three mechanical traps, all found by dry-running the signing sequence before
shipping it, all now commented in `release.yml`:

- **`security import` needs a legacy-PBE p12.** OpenSSL 3's default encryption
  reports "1 identity imported" and then yields no usable identity.
  `-certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1` fixes it.
- **`list-keychains` prints `login.keychain-db` and `-s` APPENDS `-db`.** Feeding
  the printed value back produces `login.keychain-db-db`, a path that does not
  exist — a broken search list for the whole account. It happened on the author's
  own machine mid-dry-run and had to be repaired by hand. Strip the trailing `-db`
  before restoring; the round trip is then stable.
- **`codesign --keychain <path>` alone is not sufficient** — still "no identity
  found". The keychain has to join the user search list. Measured twice.

And the reason the guard matches `certificate` rather than `certificate leaf`:
**the DR says `leaf` or `root` depending on the signing cert's SUBJECT.** An `/O=`
field flips it, because codesign then reads the self-signed cert as a chain root.

```text
/CN=beckon signtest2                              -> certificate leaf
/CN=beckon release signing (self-signed)/O=xom11  -> certificate root
/CN=beckon release signing (self-signed)          -> certificate leaf
```

Both are cert-anchored and either would carry a grant, but the shipping cert is
generated **without `/O=`** so its requirement has the exact shape row 5 was
proven with. A guard demanding `leaf` rejected a correctly signed binary on the
first dry run.

### ~~The restart-on-grant tick has no cap and no memory~~ — WITHDRAWN same day

**This entry was wrong. It is kept so the reasoning is not repeated.** It read
that `serve`'s grant-watch tick loops without bound, on the evidence of five
cycles in macmini's log:

```text
GRANTED->restart / registered / NOT-GRANTED / registered / GRANTED->restart / …
```

and concluded `is_accessibility_trusted()` was answering true in processes TCC
records as denied.

**The owner then said: those were their own clicks.** Five cycles is five
RELEASES installed that day (0.9.11 → 0.9.15) — each a new cdhash, each prompting,
each granted by a human sitting at that machine. The tick did exactly what it is
for, once per upgrade. There was no loop and there is nothing to cap.

The same correction dissolves two neighbouring claims made in the same hour:

- *"`is_accessibility_trusted()` flips within one second, so TCC is
  non-deterministic"* — no: `serve` raised the dialog, a human clicked Allow, and
  the next poll saw it. Designed behaviour, misread as an anomaly.
- *"the probe is blind, it reports granted for a cert nobody granted"* — no: the
  grant was real, made at 23:40:40 by the same human. A later run with a fresh
  cert reported `NOT granted`, which is the negative control that should have been
  taken FIRST.

**The shape of the error is worth more than the entry was.** Three wrong
conclusions in one hour, all from one omission: reading a positive result with no
negative control, on a machine where a human can change the answer between two
samples. Ask who else is touching the machine before calling a coincidence a
defect.

## Two capabilities live in different processes on this machine

Measured 2026-08-16; this is why the macOS UI probes are awkward and it is not
a thing any single process can discover:

| | agent's shell | Terminal.app |
|---|---|---|
| `launchctl managername` is `Aqua`, i.e. AppKit can draw | **no** (`Background`) | yes |
| `AXIsProcessTrusted()`, i.e. `CGEventPost` is not a no-op | **yes** | no |

So a probe launched through Terminal draws but cannot inject, and an injector
run from the agent's shell is trusted but has no session to inject into.
`examples/hid_click.rs` exists to be the second half of that split and **prints
`AXIsProcessTrusted` before posting**, because an untrusted `CGEventPost`
returns `void` and does nothing — silently, which is indistinguishable from a
click that missed.

**CORRECTED 2026-08-16: the table is closed WITHOUT any new grant, and the loop
exists — `testing/macos_settings_drive.lua`.** Use one process for each half
instead of looking for one process with both:

- **`sudo launchctl asuser $(id -u) <cmd>` runs in `Aqua`** — measured,
  `launchctl managername` prints it — so `beckon serve` draws its tray and
  window from a session that cannot draw anything of its own.
- **Hammerspoon drives it.** It is an ordinary GUI app that already holds
  Accessibility, so `hs.eventtap` posts real events and **`hs.axuielement`
  reads another process's entire control tree** — buttons, check boxes, popups,
  the table's rows, and the tray's own menu — by title and value, and presses
  them.

Measured on airm3 2026-08-16 by exactly that pair: every phase-D check passed
unattended (Record arms, an injected `ctrl+cmd+opt+B` fills the four boxes and
the key list, bare Escape cancels, a page switch stops the recording, `Cmd+Q`
is swallowed and beckon survives).

**The `hs -c` call TIMES OUT and that is expected**, because `hs.timer.usleep`
blocks Hammerspoon's run loop: the script writes its results to a file and the
caller reads that. A session that treats `receive timeout` as failure will
rewrite a working harness.

**The Accessibility *inspection* route is a dead end THROUGH SYSTEM EVENTS, and
only there.** System Events reported `count of windows` = 0 for the probe —
and, asked as a control, 0 for Terminal and 0 for Finder, on a machine where
System Events' own `UI elements enabled` is true. That is a property of the
AppleScript bridge, not of AX: Hammerspoon's `hs.axuielement` answered
`Finder AX ok, windows=3` on the same machine minutes later. This entry used to
stop at the first sentence, which reads as *AX cannot inspect* and sends the
next session to write instructions for a human instead of a harness.

What DOES work from an agent shell with no grant at all:
`beckon_macos::window_server_windows()` (`CGWindowListCopyWindowInfo`), which
is how "the settings window is on screen, 640x532, layer 0" was confirmed
without a screenshot.

**`WINDOW: up` is not a precondition; the first heartbeat is.** A probe that
prints a line after `makeKeyAndOrderFront` has not yet proved anything: an
AppKit window is not an accessibility citizen, and does not answer to anything,
until its process is pumping events. Wait for a line only a turning run loop
can emit.

## Hot-path cost (airm3, ~95–105 ms total)

Unlike Windows there is no structural win left here — most of the time is
Apple's, not ours:

- `open -b <bundle>` is **55–75 ms** and is 92% of the focus path. Of that,
  only ~13 ms is spawning `/usr/bin/open` (bare spawn floor is 2.8 ms; `open`
  with no args, i.e. spawn + dyld of Cocoa/AppKit/CoreServices, is 12.9 ms).
  The rest is the LaunchServices + reopen-Apple-Event round-trip, which no API
  avoids: a native `NSWorkspace.openApplication(at:configuration:)` probe
  measured 50–60 ms to its completion handler. Swapping to it would buy ~13 ms
  in exchange for block/runloop plumbing in the one area that has already
  produced two focus bugs (`82c210a`, `61bf656`) — not currently worth it.
- `AXIsProcessTrusted()` is **~20 ms**, which is why the step-4.5 guard tests
  the window count first (the order is load-bearing). A/B on the cycle/toggle
  path: 53.8 ms → 44.7 ms median.
- AX cost is **per-process setup, not per-call**: the first
  `collect_app_windows` for a pid is ~38 ms, the second ~0.25 ms. So
  de-duplicating the `visible_standard_window_count` / `cycle_to_next_window`
  pair buys nothing — measured, don't bother.
- Everything else is noise: `running_apps()` 8–9 ms, process start ~5 ms, MRU
  write ~0.4 ms.

## Implementation details

- **Accessibility permission** is bound to the binary's code signature. Each
  fresh `cargo build` produces a new unsigned binary with a different identity
  → permission resets. For development, sign the binary or use a stable
  wrapper. Production users via Nix get a stable
  `/etc/profiles/per-user/<user>/bin/beckon` path that survives rebuilds (the
  Nix-store hash changes but the wrapper symlink does not, and macOS appears to
  accept that).
- **`activate()` vs `activateWithOptions:`** — objc2-app-kit 0.3 only exposes
  `activateWithOptions:`. We pass empty options (no `ActivateAllWindows`) so
  step 5a's window-cycle decision survives the activation.
- **Launch path** — we shell out to `/usr/bin/open -b <bundle_id>` instead of
  `NSWorkspace.openApplicationAtURL:configuration:completionHandler:` because
  the latter is async-only on modern macOS and would force us to spin a
  runloop. `open` returns in ~10–20 ms.
- **Cycle algorithm** — `AXUIElementCopyAttributeValue(app, "AXWindows")` gives
  a `CFArray<AXUIElement>`. We find the element with `AXMain == true` and
  `AXRaise` the next one (wrap-around). Returns `false` (falls through to step
  5b) if there are <2 windows OR if the process is not AX-trusted — we can't
  distinguish those reliably.
- **z-order other-app pick (5b)** —
  `CGWindowListCopyWindowInfo(.onScreenOnly | .excludeDesktopElements,
  kCGNullWindowID)` returns front-to-back layer-0 windows. Filter to those with
  PIDs not in the target's bundle PID set; first hit is the most-recent OTHER
  app.
- **PWA scan recursion** — macOS browsers (Brave/Chrome/Vivaldi) install PWAs
  into `~/Applications/<Browser> Apps.localized/<Name>.app`, one level deeper
  than a flat `read_dir` of `~/Applications` reaches. `installed_apps()`
  descends one extra level into any non-`.app` directory child of each root,
  but stops there (going inside a `.app` would surface nested helper bundles
  like `Foo.app/Contents/Library/Bar.app`, which are not user-launchable). PWAs
  ship with `CFBundleDisplayName=Discord` (etc.) — beckon's Name match works
  directly; the bundle ids contain a per-install hash and are not portable
  across machines.
- **Hammerspoon spoon must avoid `hs.execute(cmd, true)`** — the `true` second
  arg makes Hammerspoon source the user's login shell (`~/.zshrc`) before each
  invocation. On a typical setup that's hundreds of ms; on a heavily customised
  zsh (this user) it can exceed 10 s — fully swamping beckon's own ~50 ms hot
  path. The spoon uses
  `hs.task.new("/etc/profiles/per-user/$USER/bin/beckon", cb, {name}):start()`
  instead — non-blocking, no shell startup. Deliberately chosen over
  `hs.execute` even with `false`, because `hs.task` also gives `exitCode` and
  `stderr` in the callback for clean error surfacing.
- **AX-cycle ref counting in `windows.rs`** —
  `AXUIElementCopyAttributeValue` returns CF refs under the create rule. We
  wrap the outer `AXWindows` array via `CFArray::wrap_under_create_rule` (from
  `windows_value`), then for each window AXUIElement we `wrap_under_get_rule`
  to take an extra retain so the per-window CF lifetime extends past the array.
  The `AxElement::from_borrowed` constructor is `unsafe` and must be paired
  with `mem::forget` — see the inline comment in `windows.rs`.
