// Progressive enhancement only. Everything below is optional: with JS off the
// page still reads, every link works, and the theme follows the OS.
//
// The rule this file keeps, and it is the same one stated on .copy in
// beckon.css: nothing renders a control that silently does nothing. The theme
// button, the OS switcher and the install tablist all ship with the `hidden`
// attribute and are revealed here; the copy buttons are created here and only
// when the clipboard API exists.
//
// TWO pieces of shared state, both stamped on <html> by the inline script in
// <head> before first paint, both remembered in localStorage, both overridable
// by a control in the nav: `data-theme` and `data-os`. This file owns the
// controls and nothing else. `data-os` changes are announced on document as a
// `beckon:os` event so a consumer — today the install tablist, tomorrow
// whatever else follows the reader's OS — can subscribe without this file
// knowing about it.
(() => {
  const root = document.documentElement;
  const btn = document.getElementById('theme');
  if (!btn) return;

  // The saved theme is already applied by the inline script in <head> — before
  // first paint, which a `defer`red file cannot promise. This only owns the
  // button.
  //
  // `dataset.theme` is unset until the reader chooses, so it cannot be the only
  // input to the label: on a dark-by-default machine the ternary fell to the
  // `dark` branch and the button offered to switch to the theme already on
  // screen. aria-label is the button's only accessible name — the visible word
  // is the neutral "Theme" — so that is the whole announcement, with nothing to
  // correct it. Resolve the same way the click handler does.
  const isDark = () => root.dataset.theme
    ? root.dataset.theme === 'dark'
    : matchMedia('(prefers-color-scheme: dark)').matches;
  const label = () =>
    btn.setAttribute('aria-label', 'Switch to ' + (isDark() ? 'light' : 'dark') + ' theme');

  label();
  btn.hidden = false;
  btn.addEventListener('click', () => {
    root.dataset.theme = isDark() ? 'light' : 'dark';
    // Wrapped for the same reason the OS switcher's is, and it is not
    // theoretical: a throw here (blocked storage, quota, Safari private mode)
    // used to skip label() and leave the button offering to switch to the
    // theme already on screen — the exact defect the isDark() resolution above
    // exists to prevent, reintroduced one line later. Failing to REMEMBER the
    // choice is survivable; announcing the wrong one is not.
    try { localStorage.setItem('beckon-theme', root.dataset.theme); } catch (e) {}
    label();
  });
})();

// The OS switcher, decision for decision the same shape as the theme button
// above: the head bootstrap owns the value and applies it before first paint,
// this owns the control.
//
// No label() equivalent is needed here and that is the point of choosing a
// <select>: its accessible name is the fixed word "OS" and its accessible
// VALUE is the current OS, so it announces the state the reader is in rather
// than the one they would move to. A cycling button can only name the next
// state, which is the bug the theme button's aria-label had before it was
// resolved against matchMedia.
(() => {
  const root = document.documentElement;
  const wrap = document.getElementById('os-switch');
  const sel = document.getElementById('os-select');
  if (!wrap || !sel) return;

  // Normally already stamped by <head>. Restamped here because the bootstrap
  // is inside a try/catch — a browser that throws on localStorage (Safari
  // private mode used to) leaves `data-os` unset, and a switcher showing
  // "macOS" over a page in no OS state at all is a control that lies. The
  // fallback matches the bootstrap's: see the <head> comment for why linux.
  const os = root.dataset.os || 'linux';
  root.dataset.os = os;
  sel.value = os;
  wrap.hidden = false;

  sel.addEventListener('change', () => {
    root.dataset.os = sel.value;
    // Wrapped, unlike the theme button's: a throw here would skip the dispatch
    // below and leave the install tabs on the previous OS while the marker in
    // #setups had already moved. Failing to remember is survivable; disagreeing
    // with itself on the same page is not.
    try { localStorage.setItem('beckon-os', sel.value); } catch (e) {}
    document.dispatchEvent(new CustomEvent('beckon:os'));
  });
})();

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
  // The ARIA tab pattern is created here, with the widget. In the markup the
  // panels are plain labelled <section>s, so a JS-off reader is not told about
  // four tab panels belonging to a tablist that is display:none. tabindex="0"
  // because a panel whose only focusable content is a copy button has none at
  // all when navigator.clipboard is absent.
  panels.forEach(p => { p.setAttribute('role', 'tabpanel'); p.tabIndex = 0; });
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
  // Which tab opens first follows `data-os`, the same state the nav's switcher
  // writes and the head bootstrap detects. This used to sniff
  // navigator.userAgent right here, privately, which made the guess
  // uncorrectable: a reader on the wrong tab could click across but could not
  // tell the page it had guessed wrong, and nothing else on the page moved
  // with them. Cargo (index 2) is nobody's default — it is the from-source
  // channel and needs a toolchain — so it is reachable only by clicking.
  const order = { macos: 0, windows: 1, linux: 3 };
  const pick = () => {
    const i = order[document.documentElement.dataset.os];
    return i === undefined ? 0 : i;
  };
  show(pick());
  document.addEventListener('beckon:os', () => show(pick()));
})();

// Copy buttons. Hidden entirely when the clipboard API is absent, rather than
// rendering a button that silently does nothing.
(() => {
  if (!navigator.clipboard) return;
  document.querySelectorAll('#install pre').forEach(pre => {
    const code = pre.querySelector('code');
    const text = code.textContent.trim();
    const first = text.split('\n')[0];

    const b = document.createElement('button');
    b.className = 'copy'; b.type = 'button'; b.textContent = 'Copy';
    // Every panel holds one or two of these and "Copy" is the same word on all
    // of them, so a screen reader's button list could not tell `brew install
    // xom11/tap/beckon` from `brew services start beckon`. The command is the
    // name.
    b.setAttribute('aria-label', 'Copy ' + first);

    // The confirmation is a visible label swap, which assistive technology is
    // never told about — and it cannot be the button's own name, because the
    // aria-label above overrides its text. A sibling live region is the one
    // place the change can be announced.
    const say = document.createElement('span');
    say.className = 'sr-only'; say.setAttribute('role', 'status');

    // The API being PRESENT is not the same as the write being ALLOWED:
    // `writeText` rejects under a restrictive permissions policy, in some
    // Firefox configurations and in embedded contexts. Unhandled, that skipped
    // both the label swap and the announcement — the same silently-inert
    // button the `navigator.clipboard` guard above exists to avoid, just
    // reached by a different route. So the rejection gets its own honest label
    // and its own announcement.
    b.addEventListener('click', async () => {
      let ok = true;
      try { await navigator.clipboard.writeText(text); } catch (e) { ok = false; }
      b.textContent = ok ? 'Copied' : 'Copy failed';
      say.textContent = ok ? 'Copied ' + first
                           : 'Copy failed — select the command and copy it by hand.';
      setTimeout(() => { b.textContent = 'Copy'; say.textContent = ''; }, 1400);
    });
    pre.appendChild(b);
    pre.appendChild(say);
  });
})();

// Native <details> already works. This only closes siblings.
document.querySelectorAll('#faq details').forEach(d =>
  d.addEventListener('toggle', () => {
    if (!d.open) return;
    d.parentElement.querySelectorAll('details[open]').forEach(o => { if (o !== d) o.open = false; });
  }));



/* ==========================================================================
   TRY IT — the hero press, and the playground that replaces #how's two loops.

   THE PATTERN IS THE SAME ONE THE REST OF THIS FILE KEEPS, applied to a much
   bigger piece of DOM: the markup ships the working, non-interactive thing and
   JS only ever takes it away. So neither of these is a `hidden` block waiting
   for a script. The hero's three-card CSS loop is in index.html and keeps
   running if this file never loads; the two looping demos in #how are in
   index.html and stay there. What follows CANCELS the hero's animation and
   REPLACES the two demos with a subtree it builds itself. Delete beckon.js and
   the page is exactly what it was before this feature existed — which is the
   only definition of progressive enhancement that survives contact with a
   feature this size.

   THE TRIGGER IS CAPS LOCK PLUS A LETTER, WHICH IS BECKON'S OWN GESTURE, and
   this replaced a bare stand-in `C`. The block that used to sit here opened
   "A web page cannot have beckon's chord" and made the stand-in the honest
   half of the pitch. Half of that is still true and half of it is now
   measured to be false, so the whole framing had to be rewritten rather than
   patched:

     - STILL TRUE: a page never sees `ctrl+super+alt+c` / `cmd+ctrl+alt+c` /
       `super+c`. Those are taken before any ordinary window is offered them,
       for a different reason on each OS — see WHY below, which is where the
       three reasons and their repo citations live — and on top of that a page
       only gets keys while the browser is in front, which is the one moment
       nobody needs a hotkey.
     - NOW FALSE: that no beckon key can reach a page. Caps Lock is not
       swallowed by the shell or the compositor the way `Win` and `Super` are;
       a page receives it, and `KeyboardEvent.getModifierState('CapsLock')`
       reports the lock. So the demos listen for beckon's real gesture instead
       of standing in for it.

   WHAT THE PAGE CAN AND CANNOT MATCH, because the gap is the argument rather
   than an embarrassment:

     - beckon reads Caps as a key you HOLD (`beckon-core`'s `caps::decide` is a
       tap-vs-hold state machine on `HOLD_TIMEOUT_MS`). A page cannot: at the
       OS level Caps is a LOCK, not a held modifier, and the events are
       asymmetric on macOS — keydown on the on-transition, keyup on the
       off-transition — so a held-key model is wrong on at least one platform.
       The page therefore reads the LOCK and asks the reader to switch it on
       first. Every line of copy says "turn Caps Lock on", never "hold Caps".
     - beckon SWALLOWS the Caps key-down (`caps.rs`: `(VK_CAPITAL, Edge::Down)
       => Action::Swallow`) inside a `WH_KEYBOARD_LL` hook, which CLAUDE.md
       records as running "before the keystroke reaches any queue". So the lock
       never flips, and the chord goes out as one `SendInput` burst instead.
       A page cannot do any of that, so pressing Caps here really does turn the
       reader's Caps Lock on. `.caps-truth` says so, and turns the cost into
       the argument. NOTHING HERE CALLS preventDefault ON THE CAPS KEY: whether
       that would stop the lock has not been measured, and a page that tried it
       and failed would be quietly lying about the mechanism.

   CLICK IS STILL THE PRIMARY PATH. A phone has no Caps Lock at all, and a
   screen-reader user very often has Caps Lock bound as the NVDA/JAWS modifier,
   where it never reaches the lock. Every demo is fully operable by pointer:
   one button per letter, each with the app it beckons on its face.
   ========================================================================== */

// README.md's letter table — `examples/windows/serve/apps.toml` binds the same
// five — and the same five rows #setups prints, which tools/check-site.sh pins
// to README byte for byte. `key` is what is printed on the cap AND what is
// compared against the event; `Space` is the one whose label is not its
// `KeyboardEvent.key`.
const beckonKeys = [
  { key: 'C',     app: 'Claude' },
  { key: 'B',     app: 'Brave' },
  { key: 'E',     app: 'Cursor' },
  { key: 'D',     app: 'Discord' },
  { key: 'Space', app: 'terminal' },
];

// `e.key`, not `e.code`. With Caps Lock on a letter arrives uppercase already,
// and `key` is the letter PRINTED on the reader's keycap — on AZERTY or
// Dvorak, `code` would name a position the reader is not looking at, while
// this page's letter table is about letters.
const beckonKeyOf = e => {
  if (e.key === ' ') return 'Space';
  return typeof e.key === 'string' && e.key.length === 1 ? e.key.toUpperCase() : null;
};

// The reader's own chord, per OS, MODIFIERS ONLY: the letter is appended by
// `beckonChordEl`, because the letter is now whichever of the five was last
// pressed rather than a constant. README.md's modifier defaults — `Super` on
// Linux, Hyper on macOS, `Ctrl+Win+Alt` on Windows. Same values as the three
// hero cards in index.html, which are markup because they must survive JS
// being off.
const beckonChord = {
  macos:   ['Cmd', 'Ctrl', 'Alt'],
  windows: ['Ctrl', 'Win', 'Alt'],
  linux:   ['Super'],
};

// beckon's own `keyboard.caps_hold` default, in Windows spelling, because the
// Caps feature is Windows-only: `ctrl+super+alt`, where `super` is the Windows
// key (README, *Caps Lock as the beckon key*; `Chord::default()` in
// beckon-core). Held separately from `beckonChord.windows` even though the two
// read the same today — one is what a machine's bindings use, the other is
// what beckon injects, and `keyboard.caps_hold` can change the second alone.
const beckonCapsHold = ['Ctrl', 'Win', 'Alt'];

const beckonEl = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};

const beckonChordEl = (os, letter) => {
  const ch = beckonEl('span', 'chord');
  beckonChord[os].concat(letter ? [letter] : []).forEach((k, i) => {
    if (i) ch.appendChild(beckonEl('span', 'plus', '+'));
    ch.appendChild(beckonEl('kbd', 'key', k));
  });
  return ch;
};

const beckonCapsChordEl = () => {
  const ch = beckonEl('span', 'chord');
  beckonCapsHold.forEach((k, i) => {
    if (i) ch.appendChild(beckonEl('span', 'plus', '+'));
    ch.appendChild(beckonEl('kbd', 'key', k));
  });
  return ch;
};

// Falls back to linux for the same reason the <head> bootstrap does: it is the
// bucket, not a product. See the comment there.
const beckonOs = () => {
  const os = document.documentElement.dataset.os;
  return os in beckonChord ? os : 'linux';
};


// --- the lock ---------------------------------------------------------------
//
// `null` until an event tells us, and it is announced as "unknown" for exactly
// that long. Guessing would be worse than saying so: the whole keyboard path
// is gated on this value, and a page that claimed "off" before it knew would
// send a reader with the lock already on to turn on a lock that is on.
//
// A READER WHO ARRIVED WITH CAPS LOCK ALREADY ON IS IN A LEGITIMATE STATE, not
// an error: the first key they press resolves `beckonCapsOn` to true, the
// indicator says so, and the gesture works immediately. Nothing here asks them
// to toggle it off and on again.
let beckonCapsOn = null;
const beckonCapsSubs = [];

// Subscribers are called once on registration, so a caller may build its
// initial text through the same path that later updates it — no second copy of
// the wording, and nothing populated after insertion.
const beckonOnCaps = fn => { beckonCapsSubs.push(fn); fn(); };

// Fed by keydown, by keyup and by clicks on the try buttons. KEYUP IS NOT A
// SECOND GESTURE LISTENER and never fires a demo — it exists because macOS
// reports the lock asymmetrically: keydown on the on-transition, keyup on the
// off-transition, so a keydown-only reader would latch "on" and never come
// back. `MouseEvent` carries `getModifierState` too, which is how a reader who
// clicks before typing gets a resolved indicator instead of "unknown".
const beckonCapsRead = e => {
  if (!e || typeof e.getModifierState !== 'function') return;
  let v;
  try { v = e.getModifierState('CapsLock'); } catch (err) { return; }
  if (typeof v !== 'boolean' || v === beckonCapsOn) return;
  beckonCapsOn = v;
  beckonCapsSubs.forEach(fn => fn());
};

// The pressed look is the shared .key contract's own `data-down`, held long
// enough to see and then dropped. Not an animation: the reduced-motion block
// pins animation-duration, and a cap that has to depress and come back is the
// one shape that block cannot land correctly.
const beckonTap = caps => {
  caps.filter(Boolean).forEach(k => k.setAttribute('data-down', ''));
  setTimeout(() => caps.filter(Boolean).forEach(k => k.removeAttribute('data-down')), 130);
};

// Every pressable demo registers here, and ONE document-level keydown listener
// serves all of them. Three things make that safe rather than a trap.
//
// 1. It refuses to act on a keystroke that belongs to something else: any
//    modifier held, a repeat, an IME composition, or a target that is a text
//    field, contenteditable, or a `<select>` (where a letter is typeahead).
//
//    A `<button>` IS NOT ON THAT LIST, and the list used to hold every button
//    but the two keycaps, which was a trap rather than caution. Chromium
//    focuses a button on mousedown, so the focus outlives the click: after
//    clicking a scenario pick or `Reset` — the first two things this page asks
//    a reader to do — the keydown target was an excluded button and the letter
//    went dead, while the hint beside it kept saying to press it. The
//    exemption list was the bug: it had to be extended by hand for every
//    button anyone added near a demo, and it was not. A letter has no native
//    behaviour on a button to protect, so there is nothing here to exclude.
//
//    SPACE IS THE ONE EXCEPTION AND IT IS NOT THAT LIST COMING BACK. Space
//    really is the native activation key of whatever has focus, and stealing
//    it would break every button on the page for a reader holding the lock on.
//    The guard is one state check — is anything focused at all — rather than a
//    roll-call of element types, so it cannot go stale the way the old list
//    did: focus something and Space belongs to it, focus nothing and Space is
//    ours. The four letters are never gated this way.
// 2. It only reaches a demo the reader can actually SEE. Without that, a
//    letter typed while reading the FAQ would walk a ring three sections up
//    the page and the reader would find it moved when they scrolled back. The
//    measure is how much of the demo is inside the viewport, taken at keypress
//    time — no observer, no scroll listener, nothing running when nobody is
//    typing.
// 3. A demo that cannot answer a letter says so instead of going silent. The
//    hero draws Claude and Brave, so `E` there has no window to act on; it
//    nudges the instruction line rather than swallowing the press. Same for a
//    letter pressed with the lock off — that is the single most likely way to
//    meet this feature and get nothing, so it is the one case that must
//    explain itself.
const beckonPressables = [];

const beckonSeen = node => {
  const r = node.getBoundingClientRect();
  const h = window.innerHeight || document.documentElement.clientHeight;
  if (r.height <= 0 || r.bottom <= 0 || r.top >= h) return 0;
  return (Math.min(r.bottom, h) - Math.max(r.top, 0)) / Math.min(r.height, h);
};

document.addEventListener('keyup', beckonCapsRead);

document.addEventListener('keydown', e => {
  beckonCapsRead(e);
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
  if (e.repeat || e.isComposing) return;
  const name = beckonKeyOf(e);
  if (!name) return;
  const hit = beckonKeys.find(k => k.key === name);
  if (!hit) return;
  const t = e.target;
  if (t && t.closest && t.closest(
    'input, textarea, select, [contenteditable=""], [contenteditable="true"]'
  )) return;
  if (name === 'Space') {
    const a = document.activeElement;
    if (a && a !== document.body && a !== document.documentElement) return;
  }

  let best = null, seen = 0.4;
  beckonPressables.forEach(p => {
    const v = beckonSeen(p.el);
    if (v > seen) { seen = v; best = p; }
  });
  if (!best) return;

  // No preventDefault on either nudge path: nothing happened, so nothing was
  // consumed, and Space must still scroll the page when it did not act.
  if (beckonCapsOn !== true) { best.nudge('caps', hit); return; }
  if (!best.takes(hit)) { best.nudge('key', hit); return; }
  e.preventDefault();
  best.fire(hit);
});

// Builds the press control: the instruction, then the row.
//
//   <p class="try-lead">Turn Caps Lock on, then press C or B…</p>
//   <div class="try">
//     <span class="try-caps"><kbd class="key">Caps</kbd><span class="plus">+</span></span>
//     <button class="try-press"><kbd class="key">C</kbd><span class="try-app">Claude</span></button>
//     …one per letter…
//     <span class="try-lock">Caps Lock: on</span>
//     <button class="try-reset">Reset</button>          (playground only)
//   </div>
//
// THE INSTRUCTION IS ABOVE THE ROW, not beside it and not after it: it names a
// precondition the reader has to satisfy BEFORE the first press means anything,
// and a hint that arrives after the press has already failed is not an
// instruction. It doubles as the nudge surface for the two ways a press can do
// nothing, which is why it is `role="status"` — filled before insertion, so
// the live region announces presses rather than announcing itself at load.
//
// The buttons are TRANSPARENT WRAPPERS around real `.key`s, not a second keycap
// style — .key's contract in beckon.css says do not fork it, and this does not:
// the button owns the hit target and the font-size that scales the cap, and the
// pressed look is still `.key[data-down]`. The Caps cap in front of them is not
// a button; it is the chord's first half and, via `.key[data-lock]`, the
// lock indicator itself — a locked key drawn as a key that is down.
const beckonTryRow = (items, onPress, opts) => {
  const o = opts || {};
  const wrap = beckonEl('div', 'try-wrap');

  const names = items.map(i => i.key);
  const list = names.length > 1
    ? names.slice(0, -1).join(', ') + ' or ' + names[names.length - 1]
    : names[0];
  const leadText = 'Turn Caps Lock on, then press ' + list +
    '. Or click a key — that always works.';

  const lead = beckonEl('p', 'try-lead', leadText);
  lead.setAttribute('role', 'status');

  const row = beckonEl('div', 'try');

  const capsWrap = beckonEl('span', 'try-caps');
  const capsCap = beckonEl('kbd', 'key', 'Caps');
  capsCap.setAttribute('aria-hidden', 'true');
  capsWrap.append(capsCap, beckonEl('span', 'plus', '+'));
  row.appendChild(capsWrap);

  const caps = {};
  items.forEach(it => {
    const b = beckonEl('button', 'try-press');
    b.type = 'button';
    // The visible name is aria-hidden and the whole gesture is the accessible
    // name, because "C" and "Claude" as two separate strings read as two
    // controls in a button list.
    b.setAttribute('aria-label', 'Run beckon ' + it.app + ' — Caps Lock plus ' + it.key);
    const cap = beckonEl('kbd', 'key', it.key);
    cap.setAttribute('aria-hidden', 'true');
    const nm = beckonEl('span', 'try-app', it.app);
    nm.setAttribute('aria-hidden', 'true');
    b.append(cap, nm);
    b.addEventListener('click', ev => { beckonCapsRead(ev); onPress(it); });
    caps[it.key] = cap;
    row.appendChild(b);
  });

  // Not a live region, deliberately, though it is real content and is not
  // aria-hidden. There is one of these per demo, so two would announce the
  // same lock change twice, and the reader's own OS already says the lock
  // flipped. The word is there to be read, by eye or in browse mode.
  const lock = beckonEl('span', 'try-lock');
  row.appendChild(lock);

  if (o.onReset) {
    const r = beckonEl('button', 'try-reset', 'Reset');
    r.type = 'button';
    r.addEventListener('click', ev => { beckonCapsRead(ev); o.onReset(); });
    row.appendChild(r);
  }

  beckonOnCaps(() => {
    const s = beckonCapsOn === null ? 'unknown' : beckonCapsOn ? 'on' : 'off';
    lock.dataset.caps = s;
    lock.textContent = 'Caps Lock: ' + s;
    if (beckonCapsOn) capsCap.setAttribute('data-lock', '');
    else capsCap.removeAttribute('data-lock');
  });

  let timer = 0;
  // Putting the instruction back is `settle`, and every press calls it —
  // including a successful one. Without that, a reader who pressed with the
  // lock off, read "Caps Lock is off", switched it on and pressed again got
  // the demo advancing under a line still telling them the lock was off.
  const settle = () => {
    clearTimeout(timer);
    timer = 0;
    delete lead.dataset.nudge;
    lead.textContent = leadText;
  };
  const nudge = (kind, hit) => {
    clearTimeout(timer);
    lead.dataset.nudge = '';
    lead.textContent =
      kind === 'key'   ? hit.key + ' is ' + hit.app + '. ' + (o.unknownSay || '')
      : beckonCapsOn === null
        ? 'This browser will not tell the page whether Caps Lock is on. Click a key instead.'
        : 'Caps Lock is off — switch it on, then press ' + hit.key +
          '. Or click ' + hit.key + '.';
    timer = setTimeout(settle, 3200);
  };

  wrap.append(lead, row);
  return { wrap: wrap, caps: caps, capsCap: capsCap, nudge: nudge, settle: settle };
};

// The readout, and it is ONE component used twice — the hero's and the
// playground's are the same panel, the same two-line split, the same live
// region. They were not: the hero shipped a bare <p> at 15px sitting one pixel
// and one token away from the static transcript below it, so the line that
// changes on every press and the line that never changes read as one
// paragraph. The changing surface has to LOOK like the changing surface in
// both places, or the hero teaches the reader that nothing moved.
//
// role=status + aria-live=polite, never assertive: a reader pressing the key
// repeatedly must not have every other announcement cut off. Callers must fill
// it BEFORE it enters the document — a live region populated after insertion
// announces itself at page load, which is three unsolicited announcements
// before the reader has done anything.
const beckonReadout = () => {
  const el   = beckonEl('div', 'readout');
  el.setAttribute('role', 'status');
  el.setAttribute('aria-live', 'polite');
  const step = beckonEl('p', 'readout-step');
  const said = beckonEl('p', 'readout-said');
  el.append(step, said);
  return { el: el, step: step, said: said };
};

// The punchline, and it is the honest cost turned into the argument. ONE copy
// on the page, in the playground, where there is room for it; the hero carries
// the warning in a clause and links here.
//
// Every clause traces, and each cost the first draft something:
//   "a page is only told that a key happened"  — observational, and the proof
//       is on the reader's own keyboard by the time they read it. NOT "a page
//       cannot swallow the key": whether preventDefault stops the lock has not
//       been measured, and this page does not try it.
//   "beckon is asked first"                    — CLAUDE.md: a WH_KEYBOARD_LL
//       callback "runs before the keystroke reaches any queue and before shell
//       hotkey processing".
//   "tick the box — Windows only, off until you do"
//                                              — README *Caps Lock as the
//       beckon key (Windows, opt-in)*; CLAUDE.md "off by default, on one OS".
//   "swallows the Caps key-down"               — caps.rs, `(VK_CAPITAL,
//       Edge::Down) => Action::Swallow`, unconditional.
//   "sends … on as one burst"                  — CLAUDE.md "The chord is
//       injected as one burst", and `chord()` builds exactly one Vec<Stroke>.
//   "the lock never flips"                     — nothing on the used-hold path
//       injects VK_CAPITAL; the up emits `release_modifiers` only.
//   "a bare tap still toggles it, because beckon puts that keystroke back"
//                                              — `CapsTap::CapsLock` is
//       `#[default]` and its arm is `SwallowAndInject(tap(VK_CAPITAL))`.
//       README: "Tapping Caps on its own still toggles Caps Lock by default."
//       NOT "a tap does nothing unless you configure it" — that is backwards.
const beckonCapsTruth = () => {
  const p = beckonEl('p', 'caps-truth caps');
  beckonOnCaps(() => {
    p.textContent = '';
    p.append(
      beckonEl('strong', null, beckonCapsOn
        ? 'Your Caps Lock is on, and pressing it here is what did that.'
        : 'Press Caps here and it really will turn your Caps Lock on.'),
      document.createTextNode(
        ' A page is only told that a key happened; beckon is asked first. With the Caps Lock ' +
        'box ticked in Settings — Windows only, and off until you tick it — its low-level ' +
        'keyboard hook swallows the Caps key-down and sends '),
      beckonCapsChordEl(),
      document.createTextNode(
        ' on as one burst instead, so the chord fires and the lock never flips. A bare Caps ' +
        'tap still toggles the lock, but only because beckon puts that keystroke back itself.'));
  });
  return p;
};


// --- the hero -------------------------------------------------------------
//
// The three OS cards are untouched: they are the page's cross-platform claim
// and stay three across, all three chords spelled out, at every `data-os`.
// This only takes the wheel. `.is-live` cancels every animation inside the
// demo and the stage's `data-front` becomes the single input to what is drawn,
// so the CSS has exactly two states instead of a timeline — see §4a.
//
// TWO LETTERS HERE, FIVE IN THE PLAYGROUND, and that is the drawing's doing
// rather than a shortcut. These cards hold one Brave window and one Claude
// window each, in markup, three times over; `E` has nothing here to launch.
// Rather than go silent it nudges and points down the page, where the
// playground models a whole desk and takes all five.
//
// The LETTER inside all three chords follows the press — the same letter in
// three different chords is the hero's actual claim, so watching it change to
// `B` in all three at once is the claim happening. The modifiers never move.
(() => {
  const demo = document.querySelector('.hero-demo');
  if (!demo) return;
  const stage = demo.querySelector('.hero-stage');
  const steps = demo.querySelector('.demo-steps');
  if (!stage || !steps) return;

  const caps = [...stage.querySelectorAll('.os-chord .key')];
  if (!caps.length) return;

  // The last cap of each chord is the letter; everything before it is the
  // machine's modifiers, which are not ours to rewrite.
  const letterCaps = [...stage.querySelectorAll('.os-chord')].map(c => {
    const k = c.querySelectorAll('.key');
    return k[k.length - 1];
  });

  const mine = beckonKeys.filter(k => k.app === 'Claude' || k.app === 'Brave');
  const out = beckonReadout();

  let front = 'brave';
  let letter = 'C';
  let pressed = false;

  // NO STEP NUMBERS HERE, and that is a deliberate difference from the
  // playground's readout rather than an oversight. This is the first
  // interactive feedback on the page, and `5b` is CLAUDE.md's internal
  // numbering — a citation the reader has not been given a referent for yet.
  // The table in #how introduces the numbers (its last column) and the
  // playground under it quotes them; up here the branch is named in the
  // table's own words instead.
  const say = (app, back, other) => {
    const s = !pressed
      ? ['Ready', 'Claude is running behind Brave on all three.']
      : back
        ? ['Switch back',
           'One window, already focused, and ' + other + ' is open — so beckon switches back ' +
           'to the app you came from.']
        : ['Focus it',
           app + ' is running but not focused, so beckon focuses it. One press, three ' +
           'different chords, the same instant.'];
    out.step.textContent = s[0];
    out.said.textContent = s[1];
  };

  const fire = hit => {
    const target = hit.app === 'Brave' ? 'brave' : 'claude';
    // `front === target` and nothing else. Gating this on "has the reader
    // pressed yet" was wrong in the one case the reader is most likely to try
    // second: at rest Brave IS the front window, so a first press of `B` on
    // `pressed === false` fell to the focus branch and announced "Brave is
    // running but not focused" over a drawing showing Brave in front. The
    // drawing is the state; the state is the only input to the branch.
    const back = front === target;
    pressed = true;
    front = back ? (target === 'claude' ? 'brave' : 'claude') : target;
    letter = hit.key;
    stage.dataset.front = front;
    letterCaps.forEach(k => { k.textContent = letter; });
    t.settle();
    beckonTap(caps.concat([t.capsCap, t.caps[hit.key]]));
    say(hit.app, back, hit.app === 'Brave' ? 'Claude' : 'Brave');
    // The transcript names the line that is being run, so it follows the
    // letter too. With JS off the sentence in index.html is untouched.
    steps.textContent = 'One line — beckon ' + hit.app + ' — bound to each machine’s own ' +
      'modifier. One press, and all three machines move at the same instant.';
    drawNote();
  };

  const t = beckonTryRow(mine, fire, {
    unknownSay: 'These three cards only draw Claude and Brave — the playground under ' +
                '“Focus is only the first press.” takes all five.',
  });

  // The one-sentence version of the playground's .pg-why, plus the warning the
  // punchline down there answers at length. It cannot wait for that section: a
  // press up here taps every cap in all three chords, and turns the reader's
  // Caps Lock on. Rebuilt when the OS axis moves, when the lock changes and
  // when the letter changes, because all three are in the sentence.
  const note = beckonEl('p', 'try-note caps');
  const drawNote = () => {
    const os = beckonOs();
    // Self-describing link text, and the same words in both caps states: a
    // link labelled "beckon does not" reads fine in the sentence and is
    // useless in a screen reader's link list, which is the one place a link
    // has to stand on its own.
    const link = beckonEl('a', null, 'why beckon’s own Caps key does not');
    link.href = '#how';
    note.textContent = '';
    note.append(
      document.createTextNode('Your chord is '),
      beckonChordEl(os, letter),
      document.createTextNode(os === 'windows'
        ? ' — a page never sees that, but it does see Caps Lock, and on Windows beckon can '
          + 'fold the chord onto Caps for real, off until you tick the box. '
        : ' — a page never sees that, but it does see Caps Lock. beckon’s Caps Lock mode is '
          + 'Windows-only, so here it is only the trigger. '),
      document.createTextNode(beckonCapsOn
        ? 'Your Caps Lock is on now, and this page turned it on: '
        : 'Pressing Caps here really does turn your Caps Lock on: '),
      link,
      document.createTextNode('.'));
  };
  document.addEventListener('beckon:os', drawNote);
  beckonOnCaps(drawNote);   // also fills it, once, before insertion

  // The shipped transcript ends "Claude comes to the front on all three",
  // which is true of the loop it describes and false on every other press once
  // the reader has the wheel. Live, it drops the outcome and keeps the
  // precondition; the readout above is the thing that names what just
  // happened. With JS off the sentence in index.html is untouched.
  steps.textContent = 'One line — beckon Claude — bound to each machine’s own ' +
    'modifier. One press, and all three machines move at the same instant.';

  stage.dataset.front = front;
  say();
  demo.classList.add('is-live');
  demo.insertBefore(note, steps);
  demo.insertBefore(t.wrap, steps);
  demo.insertBefore(out.el, steps);

  beckonPressables.push({
    el: demo,
    fire: fire,
    nudge: t.nudge,
    takes: hit => mine.indexOf(hit) >= 0,
  });
})();


// --- the playground -------------------------------------------------------
//
// ONE block, four scenarios, five letters, covering every branch of the focus
// algorithm. The branch is not scripted per scenario: `advance` below IS the
// algorithm from CLAUDE.md's *Focus algorithm*, run against a small model of
// the desk, and the readout names whichever branch it took. A scenario is
// therefore only a starting state plus a set of windows, and it is impossible
// for the readout and the drawing to disagree about which step fired.
//
// THE MODEL IS A DESK, NOT ONE APP PLUS A BOOLEAN. It used to be
// `{ running, cur, hidden, other }` — Claude, plus a flag saying whether some
// other app existed — which was enough while a bare `C` was the only trigger.
// Five letters need five apps, and the algorithm's own steps then fall out of
// the same model instead of being special-cased: pressing the letter of the
// app that is already focused walks step 5 (5a cycle / 5b back / 5c hide)
// exactly as before, and pressing a different app's letter is step 5 (focus)
// or step 4 (launch) if nothing of it is running. Every one of those is a row
// in the table one screen up, so the five letters make that table live rather
// than adding a behaviour it does not describe.
//
// Scenario 3 is the one people get wrong, including an earlier draft of this
// page: with more than one window of the same app the ring NEVER exits step
// 5a, so another app is unreachable and hide is unreachable. That falls out of
// the order of the branches here rather than being asserted — 5a is tested
// before 5b and 5c, so as long as the app has two windows the other two cannot
// be reached.
(() => {
  const host = document.querySelector('#how .how-demos');
  if (!host) return;

  // Each is the true reason for THAT OS, not one reason stretched over three —
  // and each is held to what the repo actually says, which cost all three of
  // them a clause:
  //
  //   macOS   — the first draft asserted that macOS awards a system-wide chord
  //             to whoever registered it first. Nothing in README.md,
  //             CLAUDE.md or examples/macos/ says that. What the repo does say
  //             is that `serve` takes the chord with RegisterEventHotKey.
  //   Windows — the first draft said "Windows gives Win to the shell", which
  //             is stronger than CLAUDE.md's fact and false for the very chord
  //             the sentence names: the shell eats Win-key SHELL hotkeys
  //             (Win+T, Win+X, Win+D … , measured on a14), while
  //             `ctrl+super+alt+c` — examples/windows/serve/apps.toml:22 — is
  //             delivered to beckon because beckon registered it, and
  //             RegisterHotKey is not even subject to UIPI.
  //   Linux   — the first draft ended "which is why beckon leaves the binding
  //             to your own bindsym line". CLAUDE.md's *Wayland hotkey* entry
  //             was rewritten specifically to retire that causal story; its
  //             three reasons are no single API, the portal model cannot carry
  //             the shortcuts TOML, and negative value, and the FAQ on this
  //             page already states them. Why the PAGE cannot see the key is a
  //             different question and is all this sentence may answer.
  //
  // These three survived the move from a stand-in key to the real Caps
  // gesture unchanged, because they were never about Caps: they are why the
  // MODIFIER CHORD cannot reach a page, which is still true and is still the
  // reason the trigger here is Caps and not that chord.
  const WHY = {
    macos: 'beckon serve holds it through RegisterEventHotKey — a web page has no way to ask ' +
           'for a system-wide hotkey at all.',
    windows: 'a chord beckon has registered with RegisterHotKey is delivered to beckon rather ' +
             'than to whatever window you are looking at — and Windows hands the shell its own ' +
             'Win-key shortcuts before any ordinary window is offered them, which is the wall ' +
             'that makes beckon reach for a low-level keyboard hook.',
    linux: 'your compositor takes that chord before any client sees it — a browser is just ' +
           'another client.',
  };

  // What the reader may actually switch on, per OS. The Windows branch is the
  // only one that describes a setting; the other two say plainly that this is
  // an illustration, so nobody goes looking for a check box that is not there.
  // UIPI is named because README lists it first of the three caveats and
  // CLAUDE.md measured both halves on a14 against an elevated Task Manager.
  const OWN = {
    windows: 'On Windows this is a real setting rather than an illustration: tick the Caps Lock ' +
             'box in Settings and Caps stands in for that chord, so your bindings do not ' +
             'change. It does nothing while an elevated window has focus — typing the chord by ' +
             'hand still works there. ',
    macos: 'beckon’s Caps Lock mode is Windows-only, so on macOS this is the demo’s trigger and ' +
           'not something you can switch on. ',
    linux: 'beckon’s Caps Lock mode is Windows-only, so on Linux this is the demo’s trigger and ' +
           'not something you can switch on. ',
  };

  // The gap between what beckon reads and what a page can read, in one
  // sentence, shared by every OS branch so it cannot drift between them.
  const HOLD_VS_LOCK = 'beckon reads Caps as a key you hold; a page is only told the lock ' +
    'changed, so here you switch the lock on first.';

  const TAG = {
    absent:  'not running',
    hidden:  'hidden',
    focused: 'focused',
    idle:    'background',
  };

  // A scenario is a list of windows in creation order plus which one has the
  // focus. `present: false` is a window the drawing shows as an outline
  // because the app is not running yet — scenario 1's whole point.
  const SC = [
    {
      pick: 'Claude is not running',
      slots: [{ app: 'Claude', present: false }],
      focus: null,
      ready: 'Nothing of Claude’s is open, and nothing else is running.',
      steps: 'Nothing of Claude’s is open, so the first press launches it. After that this is ' +
             'the one-window case — press again to hide it, once more to bring it back.',
    },
    {
      pick: 'One window, Brave also open',
      slots: [{ app: 'Claude', present: true }, { app: 'Brave', present: true }],
      focus: 1,
      ready: 'One Claude window; Brave is in front.',
      steps: 'One Claude window, with Brave in front. The first press focuses Claude and the ' +
             'second goes back to Brave, and it keeps alternating — the same key gets you there ' +
             'and back, so it doubles as the switch between the two apps you are actually using.',
    },
    {
      pick: 'Three windows open',
      slots: [
        { app: 'Claude', count: '1/3', present: true },
        { app: 'Claude', count: '2/3', present: true },
        { app: 'Claude', count: '3/3', present: true },
        { app: 'Brave', present: true },
      ],
      focus: 3,
      ready: 'Three Claude windows; Brave is in front.',
      // The readout narrates the ring on every press, so this says the one
      // thing the readout cannot: that the behaviour was measured. It used to
      // repeat "every window exactly once per lap" and "Brave and hide are out
      // of reach" back at the reader while the readout was saying both, 40px
      // apart on the same panel.
      steps: 'Three Claude windows and Brave, with Brave in front. Verified live on sway: ' +
             'three foot windows, seven presses, 35 → 36 → 37 → 35.',
    },
    {
      pick: 'One window, nothing else open',
      // FOCUSED at rest, and that is the only starting state that makes this
      // scenario worth having. It shipped unfocused, so press 1 was step 5
      // (focus) and the 5c this scenario exists to isolate only arrived on
      // press 2 — while its own transcript promised 5c first. Worse, from
      // press 2 on it was byte-identical to scenario 1 after that scenario's
      // launch. Focused is also the only state coherent with nothing else
      // running: Claude is the only app up, so something of Claude's has the
      // focus.
      slots: [{ app: 'Claude', present: true }],
      focus: 0,
      ready: 'One Claude window, focused, and nothing else is running.',
      steps: 'One Claude window and nothing else running. The press that would switch back to ' +
             'another app has nowhere to go, so it hides Claude instead — and the next press ' +
             'brings it back.',
    },
  ];

  // --- the desk -------------------------------------------------------------
  //
  // `slots` is every window, in creation order, and THAT ORDER IS THE RING for
  // step 5a: CLAUDE.md's step-5a note says the ring is ordered by the window's
  // own address, that addresses are "stable for the window's lifetime and
  // ordered by creation", and that rotating over them "visits every window
  // exactly once per lap". Ordering the ring by recency instead is the 2-cycle
  // bug that note exists to record.
  //
  // `mru` is every index, most-recently-focused first, and it is step 5b's
  // input and nothing else's — "switch to most-recent window of a DIFFERENT
  // app". Keeping the two orders apart in the model is what keeps 5a and 5b
  // from quietly becoming the same walk.
  let idx = 0;
  let slots = [];
  let focus = null;
  let mru = [];

  const focusTo = i => {
    focus = i;
    slots[i].hidden = false;
    mru = [i].concat(mru.filter(n => n !== i));
  };

  // CLAUDE.md, *Focus algorithm*, steps 4 and 5. The order of the three
  // sub-branches is the whole of step 5's behaviour and must not be reordered:
  // 5a before 5b before 5c.
  const advance = app => {
    const mine = slots
      .map((s, i) => i)
      .filter(i => slots[i].app === app && slots[i].present);

    if (!mine.length) {
      // Step 4. A letter for an app the scenario never drew gets a window of
      // its own here — which is what launching is, and is the honest answer
      // to `Caps+D` on a desk with no Discord.
      let i = slots.findIndex(s => s.app === app && !s.present);
      if (i < 0) i = addSlot({ app: app, present: false });
      slots[i].present = true;
      focusTo(i);
      return ['step 4',
        'no window of ' + app + '’s exists, so beckon reads the launch command out of the ' +
        'OS’s own metadata — .desktop on Linux, LaunchServices on macOS, the Start menu ' +
        'on Windows — and runs it.'];
    }

    if (focus === null || slots[focus].app !== app) {
      // "its first window", not "its most recent". CLAUDE.md's step 5 is
      // "focus first window", `algorithm.rs` picks the minimum of
      // recency-then-address, and on sway and i3 every recency is 0 so the
      // address alone decides — the test block there is literally headed
      // "sway-style: every recency=0, ties broken by address". "Most recent"
      // also contradicts the 5a line one press later, which says the ring is
      // ordered by address. MRU belongs to 5b, and only 5b.
      focusTo(mine[0]);
      return ['step 5',
        app + ' is running but not focused, so beckon focuses its first window.'];
    }

    if (mine.length > 1) {
      const next = mine[(mine.indexOf(focus) + 1) % mine.length];
      focusTo(next);
      return ['step 5a', next === mine[0]
        ? 'same app has another window, so focus the next one — and that is the lap closing. ' +
          'The ring hands the last window back to the first, never to another app: while ' +
          app + ' has more than one window, steps 5b and 5c cannot be reached at all.'
        : 'same app has another window, so focus the next one. The ring is ordered by the ' +
          'window’s own address, so it visits every window exactly once per lap.'];
    }

    const back = mru.filter(i =>
      i !== focus && slots[i].present && !slots[i].hidden && slots[i].app !== app)[0];
    if (back !== undefined) {
      const name = slots[back].app;
      focusTo(back);
      return ['step 5b',
        'one window, already focused, and another app is open — so beckon switches back to ' +
        name + ', the app you came from.'];
    }

    slots[focus].hidden = true;
    focus = null;
    return ['step 5c',
      'one window, already focused, and nothing else to switch to — so beckon hides it.'];
  };

  // --- the subtree ---------------------------------------------------------
  const demo  = beckonEl('div', 'demo pg');
  // `caps` is not decoration: it is the page's existing solution for keycaps
  // set inside running prose (extra leading so wrapped lines do not touch),
  // and this paragraph is that exact shape. It used to fork it with its own
  // line-height and its own .chord margin.
  const why   = beckonEl('p', 'pg-why caps');
  const truth = beckonCapsTruth();
  const picks = beckonEl('div', 'pg-picks');
  picks.setAttribute('role', 'group');
  picks.setAttribute('aria-label', 'Scenario');

  const main  = beckonEl('div', 'pg-main');
  const left  = beckonEl('div', 'pg-left');
  const stage = beckonEl('div', 'pg-stage');
  stage.setAttribute('aria-hidden', 'true');

  // The readout is the thing that actually teaches, so it is a panel of its
  // own beside the drawing rather than a caption under it — the same component
  // the hero uses, built by the same function.
  const out    = beckonReadout();
  const stepEl = out.step;
  const saidEl = out.said;

  const steps = beckonEl('p', 'demo-steps');

  const buildWin = slot => {
    const w = beckonEl('div', 'pg-win');
    w.dataset.app = slot.app;

    const bar  = beckonEl('div', 'pg-bar');
    const dots = beckonEl('span', 'pg-dots');
    dots.append(beckonEl('span'), beckonEl('span'), beckonEl('span'));
    const ctl  = beckonEl('span', 'pg-ctl');
    ctl.append(beckonEl('span', 'pg-min'), beckonEl('span', 'pg-max'), beckonEl('span', 'pg-close'));
    bar.append(dots, beckonEl('span', 'pg-name', slot.app), ctl);

    const face = beckonEl('div', 'pg-face');
    const tag  = beckonEl('span', 'pg-tag');
    face.append(beckonEl('span', 'pg-count', slot.count || ''), tag);

    w.append(bar, face);
    w.tagCell = tag;
    return w;
  };

  const addSlot = spec => {
    const s = { app: spec.app, count: spec.count, present: !!spec.present, hidden: false };
    s.el = buildWin(s);
    stage.appendChild(s.el);
    slots.push(s);
    mru.push(slots.length - 1);
    return slots.length - 1;
  };

  const paint = () => {
    slots.forEach((s, i) => {
      const state = !s.present ? 'absent'
                  : s.hidden   ? 'hidden'
                  : focus === i ? 'focused' : 'idle';
      s.el.dataset.state = state;
      s.el.tagCell.textContent = TAG[state];
    });
  };

  const rest = word => {
    stepEl.textContent = word;
    saidEl.textContent = SC[idx].ready;
  };

  const reset = word => {
    const sc = SC[idx];
    stage.textContent = '';
    slots = []; mru = []; focus = null;
    sc.slots.forEach(addSlot);
    // The starting focus goes through focusTo so `mru` starts consistent with
    // it — 5b reads that list, and a scenario that seeded focus without it
    // would answer "the app you came from" with whichever window happened to
    // be first in the markup.
    if (sc.focus !== null) focusTo(sc.focus);
    paint();
    rest(word);
  };

  const select = i => {
    idx = i;
    picks.querySelectorAll('button').forEach((b, n) =>
      b.setAttribute('aria-pressed', String(n === i)));
    steps.textContent = SC[i].steps;
    reset('Ready');
  };

  SC.forEach((sc, i) => {
    const b = beckonEl('button', 'pg-pick', sc.pick);
    b.type = 'button';
    b.setAttribute('aria-pressed', 'false');
    // Re-activating the scenario already chosen is INERT, not a reset. It ran
    // `select(i)` unconditionally, so clicking the button that already read
    // aria-pressed="true" threw away a lap in progress and jumped the readout
    // back to "Ready" with nothing on screen saying why. A pressed toggle that
    // does neither of the two things a pressed toggle can do — toggle off, or
    // nothing — is the one behaviour it must not have.
    b.addEventListener('click', ev => { beckonCapsRead(ev); if (i !== idx) select(i); });
    picks.appendChild(b);
  });

  let letter = 'C';

  const fire = hit => {
    const r = advance(hit.app);
    paint();
    letter = hit.key;
    t.settle();
    beckonTap([t.capsCap, t.caps[hit.key]]);
    stepEl.textContent = r[0];
    saidEl.textContent = r[1];
    drawWhy();
  };

  const t = beckonTryRow(beckonKeys, fire, { onReset: () => reset('Reset') });

  // The constraint sentence is rebuilt whenever the OS axis moves, because the
  // chord in it is the reader's own and so is the reason beside it — and
  // whenever the letter changes, because the chord ends in the letter that was
  // last pressed. The window chrome needs no rebuild: it is CSS keyed on
  // :root[data-os].
  const drawWhy = () => {
    const os = beckonOs();
    why.textContent = '';
    why.append(
      beckonEl('strong', null, 'Caps Lock is the one beckon key a page can see.'),
      document.createTextNode(' Your chord is '),
      beckonChordEl(os, letter),
      document.createTextNode(
        ', and ' + WHY[os] + ' A page also only gets keys while the browser is in front, ' +
        'which is the one moment nobody needs a hotkey. ' + OWN[os] + HOLD_VS_LOCK));
  };

  // ORDER: the affordance first, the caveat after it. `.pg-why` used to sit
  // between the caption and the controls, which put five lines of grey prose
  // about what the page CANNOT do — 143px of it — between "Try it" and the
  // first thing a reader can click, and the keycap itself 578px below the only
  // label that says this block is interactive. The two caveat paragraphs are
  // still above the transcript and still at prose size; they are simply no
  // longer the thing standing in the doorway.
  // The two caveat paragraphs are the two halves of one argument — what a page
  // cannot have, and what it does to you when it takes what it can — so they
  // share a wrapper and sit side by side from 900px up. Stacked, they were 450px
  // of grey prose under the controls; the wrapper is what keeps §5 owning the
  // breakpoint instead of this block growing one of its own.
  const prose = beckonEl('div', 'pg-prose');
  prose.append(why, truth);

  left.append(stage, t.wrap);
  main.append(left, out.el);
  demo.append(beckonEl('h3', 'demo-cap', 'Try it'), picks, main, prose, steps);

  drawWhy();
  document.addEventListener('beckon:os', drawWhy);

  // BEFORE the subtree enters the document, so the readout's first words are
  // its initial state rather than a live-region announcement fired at page
  // load. (And still before the JS-off content is taken away: everything up to
  // `host.textContent = ''` is construction, so a throw anywhere in it leaves
  // the reader with the two looping demos.)
  select(0);

  host.textContent = '';
  host.classList.add('is-live');
  host.appendChild(demo);

  beckonPressables.push({
    el: demo,
    fire: fire,
    nudge: t.nudge,
    takes: () => true,
  });
})();
