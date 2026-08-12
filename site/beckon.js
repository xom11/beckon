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
    localStorage.setItem('beckon-theme', root.dataset.theme);
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

    b.addEventListener('click', async () => {
      await navigator.clipboard.writeText(text);
      b.textContent = 'Copied';
      say.textContent = 'Copied ' + first;
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
   shortcut. A web page cannot have beckon's chord. On Windows the shell takes
   `Win` before any ordinary window is offered the keystroke — that is the same
   fact CLAUDE.md records about beckon's own chord capture, which is why beckon
   needs a WH_KEYBOARD_LL hook and not a window message. On Linux the
   compositor keeps `Super` for itself, which is why beckon does not register
   Linux hotkeys at all. And on every OS a page only receives keys while the
   browser is in front, which is the one moment nobody needs a hotkey. So the
   demos below listen for a bare `C` — the letter README's own example table
   binds to Claude — and say so, in the reader's own chord, before the first
   press.
   ========================================================================== */

const beckonKey = 'C';

const beckonEl = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
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
//    field, a select, contenteditable, or any button other than one of ours.
//    `[data-press]` is the exemption — those buttons have no native `C`
//    behaviour to collide with, and excluding them would mean that clicking
//    the key once (which leaves it focused) silently stopped `C` working.
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
    'input, textarea, select, [contenteditable=""], [contenteditable="true"], button:not([data-press])'
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
  btn.dataset.press = '';
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

  const out = beckonEl('p', 'try-out');
  out.setAttribute('role', 'status');
  out.setAttribute('aria-live', 'polite');

  let front = 'brave';
  let pressed = false;

  const say = () => {
    out.textContent = !pressed
      ? 'Claude is running behind Brave on all three. Press to focus it.'
      : front === 'claude'
        ? 'step 5 — running but not focused, so beckon focuses it. One press, three different chords, the same instant.'
        : 'step 5b — one window, already focused, and Brave is open, so beckon switches back to the app you came from.';
  };

  const fire = () => {
    pressed = true;
    front = front === 'claude' ? 'brave' : 'claude';
    stage.dataset.front = front;
    beckonTap(caps);
    say();
  };

  const t = beckonTryRow('Press ' + beckonKey + ' — run beckon Claude on all three', fire);

  stage.dataset.front = front;
  demo.classList.add('is-live');
  demo.insertBefore(t.row, steps);
  demo.insertBefore(out, steps);
  say();

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
  const root = document.documentElement;

  const CHORD = {
    macos:   ['Cmd', 'Ctrl', 'Alt', beckonKey],
    windows: ['Ctrl', 'Win', 'Alt', beckonKey],
    linux:   ['Super', beckonKey],
  };
  // Each is the true reason for THAT OS, not one reason stretched over three.
  const WHY = {
    macos: 'macOS gives a system-wide chord to whatever registered it first ' +
           '(beckon serve uses RegisterEventHotKey), and a web page has no way to ask.',
    windows: 'Windows gives Win to the shell before any ordinary window is offered it — ' +
             'the same wall that makes beckon reach for a low-level keyboard hook.',
    linux: 'your compositor keeps Super for itself, which is why beckon leaves the binding ' +
           'to your own bindsym line.',
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
      steps: 'Three Claude windows and Brave. Once Claude has focus the ring never exits step ' +
             '5a: 1/3 to 2/3 to 3/3 and back to 1/3, one window per press, every window exactly ' +
             'once per lap. Brave and hide are both out of reach until Claude is down to a single ' +
             'window. Verified live on sway with three foot windows.',
    },
    {
      pick: 'One window, nothing else open',
      slots: [{ app: 'Claude' }],
      init: { running: true, cur: null, hidden: true, other: false },
      ready: 'One Claude window, hidden, and nothing else is running.',
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
      return ['step 5',
        'Claude is running but not focused, so beckon focuses its most recent window.'];
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
  const why   = beckonEl('p', 'pg-why');
  const picks = beckonEl('div', 'pg-picks');
  picks.setAttribute('role', 'group');
  picks.setAttribute('aria-label', 'Scenario');

  const main  = beckonEl('div', 'pg-main');
  const left  = beckonEl('div', 'pg-left');
  const stage = beckonEl('div', 'pg-stage');
  stage.setAttribute('aria-hidden', 'true');

  // The readout is the thing that actually teaches, so it is a panel of its
  // own beside the drawing rather than a caption under it. role=status +
  // aria-live=polite, never assertive: a reader pressing the key repeatedly
  // must not have every other announcement cut off.
  const out    = beckonEl('div', 'pg-readout');
  out.setAttribute('role', 'status');
  out.setAttribute('aria-live', 'polite');
  const stepEl = beckonEl('p', 'pg-step');
  const saidEl = beckonEl('p', 'pg-said');
  out.append(stepEl, saidEl);

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
    b.addEventListener('click', () => select(i));
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
    const os = root.dataset.os in CHORD ? root.dataset.os : 'linux';
    why.textContent = '';
    why.append(
      beckonEl('strong', null, 'This page cannot see your real chord.'),
      document.createTextNode(' Yours is '));

    const ch = beckonEl('span', 'chord');
    CHORD[os].forEach((k, i) => {
      if (i) ch.appendChild(beckonEl('span', 'plus', '+'));
      ch.appendChild(beckonEl('kbd', 'key', k));
    });
    why.appendChild(ch);

    why.appendChild(document.createTextNode(
      ', and ' + WHY[os] + ' A page also only gets keys while the browser is in front, ' +
      'which is the one moment nobody needs a hotkey. So below, a bare '));
    why.appendChild(beckonEl('kbd', 'key', beckonKey));
    why.appendChild(document.createTextNode(
      ' stands in for it — the letter beckon’s own examples bind to Claude.'));
  };

  left.append(stage, t.row);
  main.append(left, out);
  demo.append(beckonEl('h3', 'demo-cap', 'Try it'), why, picks, main, steps);

  drawWhy();
  document.addEventListener('beckon:os', drawWhy);

  // Only now is the working, JS-off content taken away. Everything above this
  // line is construction; if any of it had thrown, the reader would still have
  // the two looping demos.
  host.textContent = '';
  host.classList.add('is-live');
  host.appendChild(demo);
  select(0);

  beckonPressables.push({ el: demo, fire: fire });
})();
