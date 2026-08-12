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

   THE STAND-IN KEY, and it is the honest half of the pitch rather than a
   shortcut. A web page cannot have beckon's chord, and the reason is not one
   reason stretched over three OSes — see WHY below, which is where the three
   are written out and where the repo citation for each one lives. What is
   common to all three is smaller and is stated everywhere the stand-in is:
   a page only receives keys while the browser is in front, which is the one
   moment nobody needs a hotkey.

   So both demos listen for a bare `C` — the letter README's own example table
   binds to Claude — and BOTH say so, in the reader's own chord, before the
   first press. "Both" is load-bearing: the hero taps ten caps across three
   cards on a press, so a hero without that line shows five modifiers going
   down that the reader did not touch and explains it a section and a half
   later.
   ========================================================================== */

const beckonKey = 'C';

// The reader's own chord, per OS, and the ONE copy of it in this file: the
// hero's note and the playground's constraint paragraph both spell it out, and
// two copies would drift. README.md's modifier defaults — `Super` on Linux,
// Hyper on macOS, `Ctrl+Win+Alt` on Windows — with the letter from README's own
// letter table. Same values as the three hero cards in index.html, which are
// markup because they must survive JS being off.
const beckonChord = {
  macos:   ['Cmd', 'Ctrl', 'Alt', beckonKey],
  windows: ['Ctrl', 'Win', 'Alt', beckonKey],
  linux:   ['Super', beckonKey],
};

const beckonEl = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};

const beckonChordEl = os => {
  const ch = beckonEl('span', 'chord');
  beckonChord[os].forEach((k, i) => {
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

// The pressed look is the shared .key contract's own `data-down`, held long
// enough to see and then dropped. Not an animation: the reduced-motion block
// pins animation-duration, and a cap that has to depress and come back is the
// one shape that block cannot land correctly.
const beckonTap = caps => {
  caps.forEach(k => k.setAttribute('data-down', ''));
  setTimeout(() => caps.forEach(k => k.removeAttribute('data-down')), 130);
};

// Every pressable demo registers here, and ONE document-level listener serves
// all of them. Two things make that safe rather than a trap.
//
// 1. It refuses to act on a keystroke that belongs to something else: any
//    modifier held, a repeat, an IME composition, or a target that is a text
//    field, contenteditable, or a `<select>` (where a letter is typeahead).
//
//    A `<button>` IS NOT ON THAT LIST, and the list used to hold every button
//    but the two keycaps, which was a trap rather than caution. Chromium
//    focuses a button on mousedown, so the focus outlives the click: after
//    clicking a scenario pick or `Reset` — the first two things this page asks
//    a reader to do — the keydown target was an excluded button and `C` went
//    dead, while the hint beside it kept saying "or press C". The exemption
//    list was the bug: it had to be extended by hand for every button anyone
//    added near a demo, and it was not. A button has no native `C` behaviour
//    to protect, so there is nothing here to exclude, and the visibility gate
//    below is what keeps a keystroke aimed at the far side of the page from
//    reaching a demo.
// 2. It only reaches a demo the reader can actually SEE. Without that, `C`
//    typed while reading the FAQ would walk a ring three sections up the page
//    and the reader would find it moved when they scrolled back. The measure
//    is how much of the demo is inside the viewport, taken at keypress time —
//    no observer, no scroll listener, nothing running when nobody is typing.
const beckonPressables = [];

const beckonSeen = node => {
  const r = node.getBoundingClientRect();
  const h = window.innerHeight || document.documentElement.clientHeight;
  if (r.height <= 0 || r.bottom <= 0 || r.top >= h) return 0;
  return (Math.min(r.bottom, h) - Math.max(r.top, 0)) / Math.min(r.height, h);
};

document.addEventListener('keydown', e => {
  if (e.key !== 'c' && e.key !== 'C') return;
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
  if (e.repeat || e.isComposing) return;
  const t = e.target;
  if (t && t.closest && t.closest(
    'input, textarea, select, [contenteditable=""], [contenteditable="true"]'
  )) return;

  let best = null, seen = 0.4;
  beckonPressables.forEach(p => {
    const v = beckonSeen(p.el);
    if (v > seen) { seen = v; best = p; }
  });
  if (!best) return;
  e.preventDefault();
  best.fire();
});

// Builds the `.try` row: the keycap button, its hint, and optionally a reset.
// The button is a transparent wrapper around a real `.key` rather than a
// second keycap style — see the CONTRACT comment on .key in beckon.css.
const beckonTryRow = (label, onPress, onReset) => {
  const row = beckonEl('div', 'try');

  const btn = beckonEl('button', 'try-press');
  btn.type = 'button';
  btn.setAttribute('aria-label', label);
  const cap = beckonEl('kbd', 'key', beckonKey);
  cap.setAttribute('aria-hidden', 'true');
  btn.appendChild(cap);
  btn.addEventListener('click', onPress);

  row.append(btn, beckonEl('span', 'try-hint', 'Click the key, or press ' + beckonKey + '.'));

  if (onReset) {
    const r = beckonEl('button', 'try-reset', 'Reset');
    r.type = 'button';
    r.addEventListener('click', onReset);
    row.appendChild(r);
  }
  return { row: row, cap: cap };
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


// --- the hero -------------------------------------------------------------
//
// The three OS cards are untouched: they are the page's cross-platform claim
// and stay three across, all three chords spelled out, at every `data-os`.
// This only takes the wheel. `.is-live` cancels every animation inside the
// demo and the stage's `data-front` becomes the single input to what is drawn,
// so the CSS has exactly two states instead of a timeline — see §4a.
(() => {
  const demo = document.querySelector('.hero-demo');
  if (!demo) return;
  const stage = demo.querySelector('.hero-stage');
  const steps = demo.querySelector('.demo-steps');
  if (!stage || !steps) return;

  const caps = [...stage.querySelectorAll('.os-chord .key')];
  if (!caps.length) return;

  const out = beckonReadout();

  let front = 'brave';
  let pressed = false;

  // NO STEP NUMBERS HERE, and that is a deliberate difference from the
  // playground's readout rather than an oversight. This is the first
  // interactive feedback on the page, and `5b` is CLAUDE.md's internal
  // numbering — a citation the reader has not been given a referent for yet.
  // The table in #how introduces the numbers (its last column) and the
  // playground under it quotes them; up here the branch is named in the
  // table's own words instead.
  const say = () => {
    const s = !pressed
      ? ['Ready', 'Claude is running behind Brave on all three. Press to focus it.']
      : front === 'claude'
        ? ['Focus it',
           'Running but not focused, so beckon focuses it. One press, three different ' +
           'chords, the same instant.']
        : ['Switch back',
           'One window, already focused, and Brave is open — so beckon switches back to ' +
           'the app you came from.'];
    out.step.textContent = s[0];
    out.said.textContent = s[1];
  };

  const fire = () => {
    pressed = true;
    front = front === 'claude' ? 'brave' : 'claude';
    stage.dataset.front = front;
    beckonTap(caps);
    say();
  };

  const t = beckonTryRow('Press ' + beckonKey + ' — run beckon Claude on all three', fire);

  // A press taps every cap in all three chords at once, which is the whole
  // point — and without this line the reader's first press on the page
  // depresses Cmd, Ctrl, Alt, Win and Super untouched, and is not told why
  // until a section and a half further down. Same claim as the playground's,
  // one sentence long, and rebuilt with the reader's own chord when the OS
  // axis moves.
  const note = beckonEl('p', 'try-note caps');
  const drawNote = () => {
    note.textContent = '';
    note.append(
      document.createTextNode('A page cannot hold your real chord — yours is '),
      beckonChordEl(beckonOs()),
      document.createTextNode(' — so a bare '),
      beckonEl('kbd', 'key', beckonKey),
      document.createTextNode(' stands in for it here.'));
  };
  drawNote();
  document.addEventListener('beckon:os', drawNote);

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
  demo.insertBefore(t.row, steps);
  demo.insertBefore(out.el, steps);

  beckonPressables.push({ el: demo, fire: fire });
})();


// --- the playground -------------------------------------------------------
//
// ONE block, four scenarios, covering every branch of the focus algorithm. The
// branch is not scripted per scenario: `advance` below IS the algorithm from
// CLAUDE.md's *Focus algorithm*, run against a tiny model of the desk, and the
// readout names whichever branch it took. A scenario is therefore only a
// starting state plus a set of windows, and it is impossible for the readout
// and the drawing to disagree about which step fired.
//
// Scenario 3 is the one people get wrong, including an earlier draft of this
// page: with more than one window of the same app the ring NEVER exits step
// 5a, so Brave is unreachable and hide is unreachable. That falls out of the
// order of the branches here rather than being asserted — 5a is tested before
// 5b and 5c, so as long as `wins > 1` the other two cannot be reached.
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

  const TAG = {
    absent:  'not running',
    hidden:  'hidden',
    focused: 'focused',
    idle:    'background',
  };

  const SC = [
    {
      pick: 'Claude is not running',
      slots: [{ app: 'Claude' }],
      init: { running: false, cur: null, hidden: false, other: false },
      ready: 'Nothing of Claude’s is open, and nothing else is running.',
      steps: 'Nothing of Claude’s is open, so the first press launches it. After that this is ' +
             'the one-window case — press again to hide it, once more to bring it back.',
    },
    {
      pick: 'One window, Brave also open',
      slots: [{ app: 'Claude' }, { app: 'Brave' }],
      init: { running: true, cur: null, hidden: false, other: true },
      ready: 'One Claude window; Brave is in front.',
      steps: 'One Claude window, with Brave in front. The first press focuses Claude and the ' +
             'second goes back to Brave, and it keeps alternating — the same key gets you there ' +
             'and back, so it doubles as the switch between the two apps you are actually using.',
    },
    {
      pick: 'Three windows open',
      slots: [
        { app: 'Claude', count: '1/3' },
        { app: 'Claude', count: '2/3' },
        { app: 'Claude', count: '3/3' },
        { app: 'Brave' },
      ],
      init: { running: true, cur: null, hidden: false, other: true },
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
      slots: [{ app: 'Claude' }],
      // FOCUSED at rest, and that is the only starting state that makes this
      // scenario worth having. It shipped `{ cur: null, hidden: true }`, so
      // press 1 was step 5 (focus) and the 5c this scenario exists to isolate
      // only arrived on press 2 — while its own transcript promised 5c first.
      // Worse, from press 2 on it was byte-identical to scenario 1 after that
      // scenario's launch, so it demonstrated nothing scenario 1 did not.
      // `cur: 0` is also the only state coherent with `other: false`: Claude
      // is the only app running, so something of Claude's has the focus.
      init: { running: true, cur: 0, hidden: false, other: false },
      ready: 'One Claude window, focused, and nothing else is running.',
      steps: 'One Claude window and nothing else running. The press that would switch back to ' +
             'another app has nowhere to go, so it hides Claude instead — and the next press ' +
             'brings it back.',
    },
  ];

  // CLAUDE.md, *Focus algorithm*, steps 4 and 5. The order of the three
  // sub-branches is the whole of step 5's behaviour and must not be reordered:
  // 5a before 5b before 5c.
  const advance = s => {
    if (!s.running) {
      s.running = true; s.hidden = false; s.cur = 0;
      return ['step 4',
        'no window of Claude’s exists, so beckon reads the launch command out of the ' +
        'OS’s own metadata — .desktop on Linux, LaunchServices on macOS, the Start menu ' +
        'on Windows — and runs it.'];
    }
    if (s.hidden || s.cur === null) {
      s.hidden = false; s.cur = 0;
      // "its first window", not "its most recent". CLAUDE.md's step 5 is
      // "focus first window", `algorithm.rs` picks the minimum of
      // recency-then-address, and on sway and i3 every recency is 0 so the
      // address alone decides — the test block there is literally headed
      // "sway-style: every recency=0, ties broken by address". "Most recent"
      // also contradicted the 5a line one press later, which says the ring is
      // ordered by address. MRU belongs to 5b, and only 5b.
      return ['step 5',
        'Claude is running but not focused, so beckon focuses its first window.'];
    }
    if (s.wins > 1) {
      s.cur = (s.cur + 1) % s.wins;
      return ['step 5a', s.cur === 0
        ? 'same app has another window, so focus the next one — and that is the lap closing. ' +
          'The ring hands 3/3 back to 1/3, never to Brave: while Claude has more than one ' +
          'window, steps 5b and 5c cannot be reached at all.'
        : 'same app has another window, so focus the next one. The ring is ordered by the ' +
          'window’s own address, so it visits every window exactly once per lap.'];
    }
    if (s.other) {
      s.cur = null;
      return ['step 5b',
        'one window, already focused, and another app is open — so beckon switches back to the ' +
        'app you came from.'];
    }
    s.cur = null; s.hidden = true;
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

  let idx = 0, st = null, wins = [];

  const paint = () => {
    let k = 0;
    wins.forEach(w => {
      let state;
      if (w.dataset.app === 'Claude') {
        const mine = k++;
        state = !st.running ? 'absent'
              : st.hidden   ? 'hidden'
              : st.cur === mine ? 'focused' : 'idle';
      } else {
        state = st.cur === null ? 'focused' : 'idle';
      }
      w.dataset.state = state;
      w.tagCell.textContent = TAG[state];
    });
  };

  const rest = word => {
    stepEl.textContent = word;
    saidEl.textContent = SC[idx].ready + ' Press ' + beckonKey + ' to run beckon Claude.';
  };

  const reset = word => {
    const sc = SC[idx];
    st = Object.assign({ wins: sc.slots.filter(s => s.app === 'Claude').length }, sc.init);
    paint();
    rest(word);
  };

  const select = i => {
    idx = i;
    picks.querySelectorAll('button').forEach((b, n) =>
      b.setAttribute('aria-pressed', String(n === i)));
    stage.textContent = '';
    wins = SC[i].slots.map(s => { const w = buildWin(s); stage.appendChild(w); return w; });
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
    b.addEventListener('click', () => { if (i !== idx) select(i); });
    picks.appendChild(b);
  });

  const fire = () => {
    const r = advance(st);
    paint();
    beckonTap([t.cap]);
    stepEl.textContent = r[0];
    saidEl.textContent = r[1];
  };

  const t = beckonTryRow(
    'Press ' + beckonKey + ' — run beckon Claude',
    fire,
    () => reset('Reset'));

  // The constraint sentence is rebuilt whenever the OS axis moves, because the
  // chord in it is the reader's own and so is the reason beside it. The window
  // chrome needs no rebuild — it is CSS keyed on :root[data-os].
  const drawWhy = () => {
    const os = beckonOs();
    why.textContent = '';
    why.append(
      beckonEl('strong', null, 'This page cannot see your real chord.'),
      document.createTextNode(' Yours is '),
      beckonChordEl(os),
      document.createTextNode(
        ', and ' + WHY[os] + ' A page also only gets keys while the browser is in front, ' +
        'which is the one moment nobody needs a hotkey. So here, a bare '),
      beckonEl('kbd', 'key', beckonKey),
      document.createTextNode(
        ' stands in for it — the letter beckon’s own examples bind to Claude.'));
  };

  // ORDER: the affordance first, the caveat after it. `.pg-why` used to sit
  // between the caption and the controls, which put five lines of grey prose
  // about what the page CANNOT do — 143px of it — between "Try it" and the
  // first thing a reader can click, and the keycap itself 578px below the only
  // label that says this block is interactive. The caveat is still above the
  // transcript and still at prose size; it is simply no longer the thing
  // standing in the doorway.
  left.append(stage, t.row);
  main.append(left, out.el);
  demo.append(beckonEl('h3', 'demo-cap', 'Try it'), picks, main, why, steps);

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

  beckonPressables.push({ el: demo, fire: fire });
})();
