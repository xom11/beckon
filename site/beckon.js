/* beckon — landing page behaviour.
 *
 * This file knows about the DOM and nothing about the algorithm. Every decision
 * about what a press DOES lives in site/desk.js, which is pure and has tests.
 * If you find yourself writing an `if` about window stacking in here, it
 * belongs in the other file.
 *
 * THE DIRECTION IS ALWAYS "JS REDUCES OR UPGRADES, NEVER GATES". Every section
 * is complete and readable before this file runs: the install panels are all
 * open, the desks are looping, the table is a table. What this file does is
 * take things away (three panels, two loops) and hand over controls that could
 * not have worked without it (the tabs, the press rows, the HUD, the OS
 * switch, the theme button). Nothing here is the only way to reach content.
 */
(function () {
  'use strict';

  var root = document.documentElement;

  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  /* How much OF THE VIEWPORT an element fills, 0..1. Used to decide which demo
     a keypress belongs to. Cheap enough to run per keydown, which is why there
     is no IntersectionObserver here.
   *
   * THE DENOMINATOR IS THE VIEWPORT, NOT THE ELEMENT, and that is a fix rather
   * than a preference. Measuring the visible FRACTION OF THE DEMO meant a demo
   * only counted once it was more than half on screen — and when the hero's
   * desk grew to full width the demo became 841px tall against a 900px
   * viewport, so at the top of the page it scored 0.481 and the hero was deaf
   * to every key. A reader landed, read "turn Caps Lock on, then a letter",
   * pressed, and nothing happened. Measured, not reasoned. */
  function share(node) {
    var r = node.getBoundingClientRect();
    var h = window.innerHeight || root.clientHeight;
    if (!r.height || !h) return 0;
    return Math.max(0, Math.min(r.bottom, h) - Math.max(r.top, 0)) / h;
  }


  /* --- theme ------------------------------------------------------------- */

  (function () {
    var btn = document.getElementById('theme');
    if (!btn) return;

    var isDark = function () {
      return root.dataset.theme
        ? root.dataset.theme === 'dark'
        : window.matchMedia('(prefers-color-scheme: dark)').matches;
    };
    var label = function () {
      btn.setAttribute('aria-label',
        isDark() ? 'Switch to the light theme' : 'Switch to the dark theme');
    };

    btn.hidden = false;
    label();
    btn.addEventListener('click', function () {
      var next = isDark() ? 'light' : 'dark';
      root.dataset.theme = next;
      try { localStorage.setItem('beckon-theme', next); } catch (e) {}
      label();
    });
  }());


  /* --- OS ---------------------------------------------------------------- */

  /* One piece of shared state: `data-os` on <html>. The inline script in <head>
     resolves it before first paint; this only lets the reader correct it. Three
     things read it — the "yours" markers (CSS), the install tab that opens
     first, and the desk in #how. The hero never does: its three desks are the
     cross-platform claim and are always all three. */
  var osSubs = [];
  var osSelect = null;

  function onOs(fn) { osSubs.push(fn); fn(root.dataset.os || 'linux'); }

  /* The one writer. Two controls set the OS — the nav's switcher and the strip
     over the hero desk — and they must never disagree, so neither of them
     touches `data-os` itself. */
  function setOs(os) {
    if (root.dataset.os === os) return;
    root.dataset.os = os;
    try { localStorage.setItem('beckon-os', os); } catch (e) {}
    if (osSelect) osSelect.value = os;
    osSubs.forEach(function (fn) { fn(os); });
  }

  (function () {
    var wrap = document.getElementById('os-switch');
    var sel = document.getElementById('os-select');
    if (!wrap || !sel) return;
    osSelect = sel;
    sel.value = root.dataset.os || 'linux';
    wrap.hidden = false;
    sel.addEventListener('change', function () { setOs(sel.value); });
  }());


  /* --- install tabs ------------------------------------------------------- */

  (function () {
    var list = document.querySelector('#install [role="tablist"]');
    if (!list) return;
    var tabs = [].slice.call(list.querySelectorAll('[role="tab"]'));
    var panels = tabs.map(function (t) {
      return document.getElementById(t.getAttribute('aria-controls'));
    });
    if (panels.some(function (p) { return !p; })) return;

    /* Set here rather than in the markup: shipped in the HTML it left the
       JS-off page announcing four tab panels for a widget that is display:none,
       with four aria-controls relationships pointing into it. */
    panels.forEach(function (p, i) {
      p.setAttribute('role', 'tabpanel');
      p.setAttribute('tabindex', '0');
      p.setAttribute('aria-labelledby', tabs[i].id);
    });

    function show(i) {
      tabs.forEach(function (t, j) {
        t.setAttribute('aria-selected', j === i ? 'true' : 'false');
        t.tabIndex = j === i ? 0 : -1;
        panels[j].hidden = j !== i;
      });
    }

    tabs.forEach(function (t, i) {
      t.addEventListener('click', function () { show(i); });
      t.addEventListener('keydown', function (e) {
        var d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
        if (!d) return;
        e.preventDefault();
        var n = (i + d + tabs.length) % tabs.length;
        show(n);
        tabs[n].focus();
      });
    });

    list.hidden = false;
    var order = { macos: 0, windows: 1, linux: 3 };
    onOs(function (os) { show(order[os] === undefined ? 0 : order[os]); });
  }());


  /* --- copy buttons ------------------------------------------------------- */

  (function () {
    if (!navigator.clipboard) return;
    [].forEach.call(document.querySelectorAll('pre'), function (pre) {
      var code = pre.querySelector('code');
      if (!code) return;
      var text = code.textContent.replace(/\s+$/, '');

      var b = el('button', 'copy', 'Copy');
      b.type = 'button';
      b.setAttribute('aria-label', 'Copy ' + text.split('\n')[0]);
      /* The label swap is invisible to a screen reader, so the confirmation is
         announced instead of only drawn. */
      var say = el('span', 'sr-only');
      say.setAttribute('role', 'status');

      b.addEventListener('click', function () {
        navigator.clipboard.writeText(text).then(function () {
          b.textContent = 'Copied';
          say.textContent = 'Copied to clipboard';
          setTimeout(function () { b.textContent = 'Copy'; say.textContent = ''; }, 1400);
        }, function () {
          b.textContent = 'Press ⌘C';
          setTimeout(function () { b.textContent = 'Copy'; }, 1400);
        });
      });
      pre.appendChild(b);
      pre.appendChild(say);
    });
  }());


  /* --- FAQ: one open at a time -------------------------------------------- */

  (function () {
    var all = [].slice.call(document.querySelectorAll('.faq-list details'));
    all.forEach(function (d) {
      d.addEventListener('toggle', function () {
        if (!d.open) return;
        all.forEach(function (o) { if (o !== d) o.open = false; });
      });
    });
  }());


  /* --- the Caps Lock gate -------------------------------------------------- */

  /* The demos ask for beckon's own gesture — Caps Lock and a letter — rather
   * than a bare letter, because a lone `C` does not read as a shortcut and the
   * whole point of the page is that beckon is one.
   *
   * A page cannot see a HELD Caps Lock. There is no `capsKey` on a keyboard
   * event the way there is `shiftKey`. So there are exactly two observable
   * signals, and this accepts either:
   *
   *   1. THE CAPS KEY WAS JUST TOUCHED — a `keydown`/`keyup` whose key is
   *      `CapsLock`, within ARM_MS. macOS fires only keydown on the way on and
   *      only keyup on the way off, which is why both arm it. This is also the
   *      only signal a synthetic-event test can reach.
   *   2. THE REAL CHORD ARRIVED — see `capsChord` below.
   *
   * `getModifierState('CapsLock')` WAS A THIRD SIGNAL AND HAD TO GO, which is
   * the one thing to re-read before adding it back. It reports the LOCK, and a
   * page cannot tell a held Caps Lock from a lit one — there is no separate
   * fact to read. So a reader who turned Caps Lock on to satisfy the gate left
   * it on, and from that moment every bare letter passed. The demo stopped
   * asking for anything and looked broken, which is exactly how it was
   * reported. Arming on the keypress instead makes the gesture per-press:
   * leaving the lock on buys nothing, and tapping Caps still works whichever
   * way the lock happens to be pointing.
   *
   * AND A REMAPPED CAPS LOCK SATISFIES NEITHER. kanata, PowerToys and a Hyper
   * remap all swallow the key before the browser sees anything — and that is
   * disproportionately the audience for a keyboard-driven app switcher.
   *
   * THE WAY OUT IS A BUTTON, NOT A COUNTER. Two refused presses used to open
   * the gate by themselves, which was worse than either alternative: a reader
   * whose Caps works fine still reached it by fumbling twice, and from the
   * outside the demo simply looked like it had stopped asking for Caps at all.
   * The gate now never opens on its own. After two refusals the hint offers a
   * button, and only the reader's own click opens it — for every demo at once,
   * because having answered the question here should not mean answering it
   * again further down the page. */
  var CAPS_ARM_MS = 1500;
  var capsArmed = 0;
  var capsOpen = false;      /* the escape hatch, and only a click opens it */
  var capsMiss = 0;
  var capsRows = [];         /* every press row, so one answer serves them all */

  function capsArm(e) { if (e.key === 'CapsLock') capsArmed = Date.now(); }
  document.addEventListener('keydown', capsArm, true);
  document.addEventListener('keyup', capsArm, true);

  /* EVERY ACCEPTED PRESS RE-ARMS, which is what makes holding Caps down and
   * typing `c b c b` work. Caps Lock is a toggle, not a modifier: pressing it
   * fires one keydown and holding it sends nothing more, and on macOS the
   * matching keyup does not arrive until the lock is switched off again. So
   * "still holding it" is not a fact this page can read — after ARM_MS the
   * window closed underneath a reader whose finger had never left the key.
   * Refreshing on each accepted letter makes a run of them keep itself alive;
   * only an actual pause of ARM_MS ends it. */
  function capsKeepAlive() { capsArmed = Date.now(); }

  /* SIGNAL 3, AND ON A REMAPPED MACHINE IT IS THE ONLY ONE THAT FIRES: the
   * reader's real chord arrived. If Caps Lock has been remapped to Hyper —
   * Karabiner, kanata, a Hammerspoon binding, which is exactly the setup this
   * page is recommending — then pressing Caps and C sends the browser
   * `Cmd+Ctrl+Alt+C`. The Caps key never appears at all, so signals 1 and 2 are
   * both silent, and the reader is doing precisely the right thing.
   *
   * Two of the three flags rather than a named chord, because the page cannot
   * insist on which: `Win` never reaches a browser on Windows, so Ctrl+Alt is
   * as much of `Ctrl+Win+Alt` as will ever arrive. Two is also what keeps
   * `Cmd+C` and `Ctrl+C` — one flag — out of it. */
  function capsChord(e) {
    return ((e.ctrlKey ? 1 : 0) + (e.altKey ? 1 : 0) + (e.metaKey ? 1 : 0)) >= 2;
  }

  function capsHeld() {
    return capsOpen || (Date.now() - capsArmed) < CAPS_ARM_MS;
  }

  /* The reader's own answer to "is Caps reaching this page?", given once. */
  function capsGiveUp() {
    capsOpen = true;
    capsRows.forEach(function (r) { r.opened(); });
  }


  /* --- the desks ---------------------------------------------------------- */

  /* Everything below needs site/desk.js. If that script failed to load, the
     page keeps its loops and its table and simply stays the JS-off version,
     which is a complete page — so bailing out here is safe rather than
     half-broken. */
  if (typeof deskMake !== 'function') return;

  var APPS = DESK_APPS;

  function makeWin(w) {
    var n = el('div', 'win');
    n.setAttribute('data-app', w.app);

    var bar = el('div', 'win-bar');
    var lights = el('span', 'win-lights');
    lights.appendChild(el('i')); lights.appendChild(el('i')); lights.appendChild(el('i'));
    var ctl = el('span', 'win-ctl');
    ctl.appendChild(el('i')); ctl.appendChild(el('i')); ctl.appendChild(el('i'));
    bar.appendChild(lights);
    bar.appendChild(el('span', 'win-name', w.app));
    bar.appendChild(ctl);

    n.appendChild(bar);
    n.appendChild(el('div', 'win-body'));

    /* The resize corner exists only here, never in the shipped markup, and that
       is the page's rule rather than an oversight: without this file nothing can
       drag it, and a grip that does not grip is a control that silently does
       nothing. Same reason the press rows and the OS strip ship `hidden`. */
    var grip = el('span', 'win-grip');
    grip.setAttribute('aria-hidden', 'true');
    n.appendChild(grip);
    return n;
  }

  /* Window elements are POOLED BY ID and never rebuilt, because the movement is
     the point: a replaced node has no previous transform for the transition in
     §4 to interpolate from, so a rebuild-per-press would make every raise
     teleport. */
  function renderDesk(host, desk) {
    var wins = host.querySelector('.desk-wins');
    if (!host._pool) host._pool = {};
    var pool = host._pool;

    /* MRU order, index 0 in front — on all three machines. The Linux desk used
       to be sorted by id and laid out edge to edge instead, because it drew
       sway: a tiling compositor does not stack, so focusing a window there does
       not move it. It draws a stacking desktop now — GNOME and KDE, which is
       what most Linux readers are looking at — and a stacking desktop raises,
       so there is one code path again. beckon still supports both; the picture
       just stopped being of the one where the picture is hardest to read. */
    var order = desk.wins.filter(function (w) { return !w.min; });

    order.forEach(function (w, i) {
      var n = pool[w.id];
      if (!n) { n = makeWin(w); pool[w.id] = n; n.setAttribute('data-id', String(w.id)); }
      if (n.parentNode !== wins) wins.appendChild(n);
      /* POSITION FROM THE WINDOW, STACKING FROM THE ORDER. The window's own
         slot never changes, so a raise leaves it exactly where it was and only
         brings it in front — which is what raising a window looks like.
         Wrapped at four because a fifth step of the cascade would put the
         window's right edge off the desk.
         A window the reader has DRAGGED owns its position outright, and this
         must not take it back: `_placed` is set by the drag, and from then on
         the cascade has nothing to say about where that window sits. */
      if (!n._placed) n.style.setProperty('--slot', String(w.slot % 4));
      n.style.zIndex = String(order.length - i);
      n.classList.toggle('is-focused', w.id === desk.focused);
      n.classList.toggle('is-max', !!w.max);
    });

    /* A minimised window leaves the desk but stays lit in the dock, because it
       is still running — which is exactly the difference between step 5c and
       quitting. Its NODE is kept: it carries wherever the reader dragged it to,
       and a restore that forgot that would teleport the window. A CLOSED window
       is gone from the model, so its node goes too — and if the app is launched
       again it gets a new id, a new node and the next free slot, which is what
       launching looks like. */
    Object.keys(pool).forEach(function (id) {
      var shown = order.some(function (w) { return String(w.id) === id; });
      var known = desk.wins.some(function (w) { return String(w.id) === id; });
      if (!shown && pool[id].parentNode) pool[id].parentNode.removeChild(pool[id]);
      if (!known) delete pool[id];
    });

    /* The dock says three different things about an app, and all three are
       states the algorithm actually distinguishes: not running (step 4 will
       launch it), running (step 5 will focus it), and holding focus right now
       (a press goes to 5a, 5b or 5c instead). Without the third, the dock and
       the windows above it disagreed about which app the reader was in.
       `focused === null` — what step 5c leaves behind — lights nothing, which
       is correct: after a hide there is no focused app. */
    var front = null;
    if (desk.focused !== null) {
      var w = desk.wins.filter(function (x) { return x.id === desk.focused; })[0];
      front = w ? w.app : null;
    }
    [].forEach.call(host.querySelectorAll('.dock-app'), function (d) {
      var app = d.getAttribute('data-app');
      d.classList.toggle('is-up', desk.wins.some(function (x) { return x.app === app; }));
      d.classList.toggle('is-focused', app !== null && app === front);
    });
  }

  /* --- the desk as a desk: drag, resize, and the three title-bar buttons ----
   *
   * NONE OF THIS IS BECKON. beckon focuses and launches; it never minimises,
   * never maximises, never closes and never moves a window — CLAUDE.md's *Out
   * of scope* says so outright. It is here because the demo claims to be the
   * machine the reader is sitting at, and a desktop whose title bar does
   * nothing is a screenshot with a caption. Every mouse gesture answers with a
   * sentence that points back at the key; `deskSayWindow` in desk.js is where
   * those sentences live, and two of them say plainly that beckon does not do
   * what the reader just did.
   *
   * EVERY LISTENER IS DELEGATED FROM THE DESK, never bound per window.
   * `renderDesk` pools window nodes and drops them on close, so per-node
   * listeners would leak one set per launch.
   *
   * GEOMETRY LIVES ON THE NODE AND NEVER IN THE MODEL. desk.js knows slots and
   * nothing else, which is what lets a press re-render the desk underneath a
   * window the reader has dragged without moving it a pixel.
   */

  /* .win's own base rule in beckon.css, in fractions of the desk. A drag has to
     turn that percentage layout into pixels before it can add a delta to it, so
     the two files agree on these four numbers by hand. Change one, change both;
     `place` below is the only reader. */
  var WIN_X = .08, WIN_Y = .14;
  var MIN_W = .22, MIN_H = .24;    /* no shrinking a window into a sliver */

  function clamp(v, lo, hi) { return v < lo ? lo : v > hi ? hi : v; }

  function pxOf(node, prop) {
    var v = parseFloat(getComputedStyle(node).getPropertyValue(prop));
    return isNaN(v) ? 0 : v;
  }

  /* Turn a cascaded window into a placed one, once. Until the reader touches
     it, a window sits where `--slot` puts it — an offset in percent OF ITS OWN
     WIDTH, so resizing it would also move it. From the first drag onward it is
     pixels from the desk's origin and only the drag writes them. */
  function place(node, box) {
    if (node._placed) return;
    var r = node.getBoundingClientRect();
    node._placed = true;
    node.style.setProperty('--slot', '0');
    node.style.setProperty('--dx', (r.left - box.left - box.width * WIN_X) + 'px');
    node.style.setProperty('--dy', (r.top - box.top - box.height * WIN_Y) + 'px');
    node.style.setProperty('--win-w', r.width + 'px');
    node.style.setProperty('--win-h', r.height + 'px');
  }

  /* Which of the three buttons was hit. macOS puts close / minimise / maximise
     on the left as traffic lights; Windows and the Linux desktops put minimise
     / maximise / close on the right. Read off the DOM rather than off
     `data-os`, because the two orders are two different elements and the
     element already knows which it is. */
  function buttonAt(target) {
    if (!target || !target.closest) return null;
    var lights = target.closest('.win-lights');
    var group = lights || target.closest('.win-ctl');
    if (!group) return null;
    var i = [].indexOf.call(group.children, target.closest('i'));
    if (i < 0) return null;
    return (lights ? ['close', 'min', 'max'] : ['min', 'max', 'close'])[i] || null;
  }

  /* `api` is { get, act, press }: read the desk, replace it with a sentence to
     go with it, or make the same request a key would. Both demos pass their own
     three, so this function never learns which section it is in — the same rule
     the desk component itself follows. */
  function wireDesk(host, api) {
    var wins = host.querySelector('.desk-wins');
    if (!wins) return;
    var drag = null;

    function idOf(n) { return Number(n.getAttribute('data-id')); }
    function maxOf(d, id) {
      var w = d.wins.filter(function (x) { return x.id === id; })[0];
      return !!(w && w.max);
    }

    host.addEventListener('click', function (e) {
      /* The dock is the mouse's way of doing what a key does — including
         bringing back a window that was minimised, which is otherwise
         unreachable without the keyboard. `true` because a pointer has no Caps
         Lock, and the gate teaches a keyboard gesture rather than policing the
         mouse. */
      var dock = e.target.closest ? e.target.closest('.dock-app') : null;
      if (dock) {
        var a = APPS.filter(function (x) { return x.name === dock.getAttribute('data-app'); })[0];
        if (a) api.press(a.key, true);
        return;
      }

      var kind = buttonAt(e.target);
      var win = kind && e.target.closest('.win');
      var d = win && api.get();
      if (!d) return;
      var id = idOf(win), name = win.getAttribute('data-app');
      if (kind === 'min') return api.act(deskMinimize(d, id), 'min', name);
      if (kind === 'close') return api.act(deskClose(d, id), 'close', name);
      api.act(deskToggleMax(d, id), maxOf(d, id) ? 'unmax' : 'max', name);
    });

    /* Double-clicking the title bar maximises, on all three of these desktops. */
    wins.addEventListener('dblclick', function (e) {
      if (buttonAt(e.target)) return;
      var bar = e.target.closest ? e.target.closest('.win-bar') : null;
      var d = bar && api.get();
      if (!d) return;
      var win = bar.closest('.win'), id = idOf(win);
      api.act(deskToggleMax(d, id), maxOf(d, id) ? 'unmax' : 'max', win.getAttribute('data-app'));
    });

    wins.addEventListener('pointerdown', function (e) {
      if (e.button) return;
      var win = e.target.closest ? e.target.closest('.win') : null;
      if (!win) return;
      var d = api.get();
      if (!d) return;
      var id = idOf(win), name = win.getAttribute('data-app');

      /* Touching a window anywhere raises it, which is the one thing a mouse
         and beckon genuinely agree about. */
      if (d.focused !== id) {
        api.act(deskFocus(d, id), 'focus', name);
        d = api.get();
      }

      if (buttonAt(e.target)) return;                 /* those are clicks */
      var grip = e.target.closest('.win-grip');
      if (!grip && !e.target.closest('.win-bar')) return;
      if (maxOf(d, id)) return;                       /* maximised: nowhere to go */

      var box = host.getBoundingClientRect();
      place(win, box);
      drag = {
        node: win, name: name, mode: grip ? 'size' : 'move', box: box,
        px: e.clientX, py: e.clientY,
        x: pxOf(win, '--dx'), y: pxOf(win, '--dy'),
        w: pxOf(win, '--win-w'), h: pxOf(win, '--win-h'),
        far: false
      };
      win.classList.add('is-grabbed');
      /* Capture keeps the move events coming when the pointer outruns the
         window, which at 6px of travel per frame it does immediately. It is
         guarded because `setPointerCapture` throws on a pointer id the browser
         did not mint — a synthetic event in a test harness, say — and a throw
         here would abandon the drag with the window still marked `is-grabbed`.
         Without capture the drag still works while the pointer is over the
         desk, which is the whole area it can move within anyway. */
      try { win.setPointerCapture(e.pointerId); } catch (err) {}
      /* A drag off a title bar is not a text selection, and on a touch screen
         it is not a scroll either. */
      e.preventDefault();
    });

    wins.addEventListener('pointermove', function (e) {
      if (!drag) return;
      var mx = e.clientX - drag.px, my = e.clientY - drag.py;
      if (!drag.far && (Math.abs(mx) > 3 || Math.abs(my) > 3)) drag.far = true;
      var b = drag.box, ox = b.width * WIN_X, oy = b.height * WIN_Y;
      /* Clamped to the desk on both axes. A window dragged off the edge of a
         picture of a desktop does not read as a window behind a bezel; it reads
         as the demo losing one. */
      if (drag.mode === 'move') {
        drag.node.style.setProperty('--dx',
          clamp(drag.x + mx, -ox, b.width - ox - drag.w) + 'px');
        drag.node.style.setProperty('--dy',
          clamp(drag.y + my, -oy, b.height - oy - drag.h) + 'px');
      } else {
        drag.node.style.setProperty('--win-w',
          clamp(drag.w + mx, b.width * MIN_W, b.width - ox - drag.x) + 'px');
        drag.node.style.setProperty('--win-h',
          clamp(drag.h + my, b.height * MIN_H, b.height - oy - drag.y) + 'px');
      }
    });

    function drop() {
      if (!drag) return;
      var d = drag;
      drag = null;
      d.node.classList.remove('is-grabbed');
      /* A press that never travelled is a click on a title bar, and that is a
         raise — which already happened on pointerdown. Only a real move earns a
         sentence, or every stray click would overwrite the transcript. */
      if (d.far) api.act(api.get(), d.mode, d.name);
    }
    wins.addEventListener('pointerup', drop);
    wins.addEventListener('pointercancel', drop);
  }

  /* CAPS LOCK IS STANDING IN FOR THE READER'S MODIFIER, and the hint names
     which one, because that substitution is the whole reason the demo asks for
     a chord at all. "Hold Caps and press C" teaches nothing on its own; "Caps
     Lock is your Hyper key here" is the sentence that makes the row under the
     desk — Cmd Ctrl Alt C, marked yours — mean something. */
  var CHORD_OF = {
    macos: 'Hyper (Cmd Ctrl Alt)',
    windows: 'Ctrl Win Alt',
    linux: 'Super'
  };
  function hintAsk(os) {
    return 'Caps Lock stands in for ' + (CHORD_OF[os] || CHORD_OF.linux) +
           ' here — hold it, then a letter. Or click one.';
  }
  var HINT_NUDGE = 'That one needs Caps Lock held down first, then the letter.';
  var HINT_STUCK = 'Still nothing? Some setups remap Caps Lock, and this page never sees it.';
  var HINT_OPEN = 'Caps Lock set aside — the letters work on their own now.';

  /* A press row, and the HUD, are how a reader with no keyboard — or on a
     phone, or with Caps remapped — takes part at all. So neither is
     decoration, neither is aria-hidden, and CLICKING NEVER GOES THROUGH THE
     CAPS GATE: requiring a lock key from a pointer would be asking for a
     gesture the device may not have. */
  function buildPress(host, onKey) {
    if (!host) return null;
    var caps = {};

    /* ONE `Caps` cap, then the five letters — not five `Caps + letter` pairs.
       Five pairs in a row rendered as ten keycaps with identical gaps, and read
       as ten keys to press rather than five chords sharing a modifier. This
       shape says the thing the gesture actually is: hold one key, pick a
       letter. The single cap is decorative — every letter is the button, and
       each carries the whole chord in its accessible name. */
    var lead = el('span', 'press-lead');
    lead.setAttribute('aria-hidden', 'true');
    lead.appendChild(el('kbd', 'key', 'Caps'));
    lead.appendChild(el('span', 'press-plus', '+'));
    host.appendChild(lead);

    APPS.forEach(function (a) {
      var b = el('button', 'press-key');
      b.type = 'button';
      b.setAttribute('aria-label', 'Caps Lock and ' + a.key + ' — ' + a.name);
      b.appendChild(el('kbd', 'key', a.label));
      b.addEventListener('click', function () { onKey(a.key, true); });
      caps[a.key] = b;
      host.appendChild(b);
    });

    var hint = el('p', 'press-hint');
    var words = document.createTextNode('');
    var out = el('button', 'press-out', 'Use letters only');
    out.type = 'button';
    out.hidden = true;
    out.addEventListener('click', capsGiveUp);
    hint.appendChild(words);
    hint.appendChild(out);
    host.appendChild(hint);
    host.hidden = false;

    /* One place decides what the hint says, out of two pieces of state: which
       machine the reader is on, and how the last press went. Set from several
       call sites instead, the OS strip would leave the sentence naming a
       modifier the reader had just switched away from. */
    var mode = 'ask';
    function paint() {
      words.nodeValue =
        (capsOpen ? HINT_OPEN
          : mode === 'stuck' ? HINT_STUCK
          : mode === 'nudge' ? HINT_NUDGE
          : hintAsk(root.dataset.os || 'linux')) + ' ';
      out.hidden = capsOpen || mode !== 'stuck';
    }
    onOs(paint);

    var row = {
      opened: paint,
      flash: function (key) {
        var b = caps[key];
        if (!b) return;
        b.classList.add('is-hit');
        setTimeout(function () { b.classList.remove('is-hit'); }, 160);
        mode = 'ask';
        paint();
      },
      miss: function () {
        capsMiss++;
        /* The gate does not open here. It offers. */
        mode = capsMiss >= 2 ? 'stuck' : 'nudge';
        paint();
      }
    };
    capsRows.push(row);
    return row;
  }

  function readout(host, step, say) {
    if (!host) return;
    host.replaceChildren();
    host.appendChild(el('div', 'readout-step', step));
    host.appendChild(el('p', 'readout-say', say));
    host.hidden = false;
  }

  var demos = [];

  /* The OS strip over the hero desk. Three buttons rather than a <select>,
     because unlike the nav's copy this one is part of the picture: all three
     options stay legible at once, which is the cross-platform claim. */
  function buildOsSeg(host) {
    if (!host) return;
    var btns = {};
    /* "Linux", not "Linux · sway". The desk draws a stacking desktop now —
       GNOME and KDE are what a Linux reader is most likely to be looking at —
       and naming one compositor on the button made the other seven supported
       ones look absent. The tile list in #setups is where they are enumerated;
       this is a switch between three machines. */
    [['macos', 'macOS'], ['windows', 'Windows'], ['linux', 'Linux']]
      .forEach(function (n) {
        var b = el('button', null, n[1]);
        b.type = 'button';
        b.setAttribute('aria-pressed', 'false');
        b.addEventListener('click', function () { setOs(n[0]); });
        btns[n[0]] = b;
        host.appendChild(b);
      });
    onOs(function (os) {
      Object.keys(btns).forEach(function (k) {
        btns[k].setAttribute('aria-pressed', k === os ? 'true' : 'false');
      });
    });
    host.hidden = false;
  }

  /* --- hero: one machine, the reader's --- */

  (function () {
    var demo = document.getElementById('hero-demo');
    var host = document.getElementById('hero-desk');
    if (!demo || !host) return;

    var steps = demo.querySelector('.demo-steps');
    var letters = [].slice.call(demo.querySelectorAll('.hero-chords .key.is-letter'));
    var desk = null;
    var ui = null;

    /* A new OS is a new machine, not the same desk repainted: the chrome, the
       window arrangement and the chord all change together — and anything the
       reader dragged, resized or maximised goes with the old machine, because
       the node carrying it is discarded here. */
    function reset(os) {
      desk = deskMake(os, DESK_SCENES.hero);
      host.setAttribute('data-os', os);
      host._pool = {};
      host.querySelector('.desk-wins').replaceChildren();
      renderDesk(host, desk);
    }

    /* `ok` is "the gesture was made" — a real chord, a held Caps Lock, or a
       click, which is always allowed because a pointer has no Caps Lock. The
       routing above works it out; nothing in here re-derives it. */
    function press(key, ok) {
      var app = deskAppOf(key);
      if (!app || !desk) return false;
      if (!ok) { if (ui) ui.miss(); return false; }
      var r = deskPress(desk, key);
      desk = r.desk;
      renderDesk(host, desk);
      if (ui) ui.flash(app.key);
      /* Only the LAST cap of each chord is rewritten — the letter. The
         modifiers are never touched: "one letter, whatever your modifier is"
         is the sentence these rows are drawing. */
      letters.forEach(function (l) { l.textContent = app.label; });
      if (steps) steps.textContent = deskSay(r);
      return true;
    }

    /* The mouse's own transcript replaces the keyboard's, because both describe
       the same desk and only one of them can be true at a time. */
    function act(next, kind, name) {
      desk = next;
      renderDesk(host, desk);
      if (steps) steps.textContent = deskSayWindow(kind, name);
    }

    ui = buildPress(document.getElementById('hero-press'), press);
    buildOsSeg(document.getElementById('hero-os'));
    wireDesk(host, { get: function () { return desk; }, act: act, press: press });
    demo.classList.add('is-live');
    /* The static windows in the markup are the JS-off picture. reset() clears
       them rather than adopting them: renderDesk pools by window id, so leaving
       them in place would draw every window twice. */
    onOs(reset);
    demos.push({ node: demo, press: press });
  }());

  /* --- how: the table is the scenario switcher --- */

  (function () {
    var demo = document.getElementById('how-demo');
    var table = document.getElementById('how-table');
    var host = document.getElementById('how-desk');
    if (!demo || !table || !host) return;

    var out = document.getElementById('how-readout');
    var rows = [].slice.call(table.querySelectorAll('tbody tr'));
    var desk = null;
    var ui = null;

    function mark(cls, step) {
      rows.forEach(function (r) {
        r.classList.toggle(cls, step !== null && r.getAttribute('data-step') === step);
      });
    }

    function scene(step) {
      desk = deskMake(root.dataset.os || 'linux', DESK_SCENES[step]);
      host.setAttribute('data-os', desk.os);
      host._pool = {};                       /* a new scene is new windows */
      host.querySelector('.desk-wins').replaceChildren();
      renderDesk(host, desk);
      mark('is-on', step);
      mark('is-hit', null);

      /* The precondition is read out of the table's own row rather than
         written twice. A blurb kept here would be a second copy of a sentence
         the table already carries, free to drift from it. */
      var row = rows.filter(function (r) { return r.getAttribute('data-step') === step; })[0];
      var th = row && row.querySelector('th');
      readout(out, 'Ready', (th ? th.textContent.trim() : 'Ready') + '. Press a key.');
    }

    function press(key, ok) {
      var app = deskAppOf(key);
      if (!app || !desk) return false;
      if (!ok) { if (ui) ui.miss(); return false; }
      var r = deskPress(desk, key);
      desk = r.desk;
      renderDesk(host, desk);
      if (ui) ui.flash(app.key);
      mark('is-on', null);
      mark('is-hit', r.step);
      /* The branch's NAME, the same word the row that just lit up prints. It
         used to read "Step 5a", which named nothing a reader could match to
         anything on screen. */
      readout(out, deskStepName(r.step), deskSay(r));
      return true;
    }

    /* The row headers ship as plain text and become buttons here. A disabled
       or inert button in the markup would be a control that silently does
       nothing, which is the one thing this page does not ship. */
    rows.forEach(function (r) {
      var th = r.querySelector('th');
      var step = r.getAttribute('data-step');
      if (!th || !step) return;
      var b = el('button', 'row-btn', th.textContent.trim());
      b.type = 'button';
      b.setAttribute('aria-label', 'Set the desk up: ' + th.textContent.trim());
      b.addEventListener('click', function () { scene(step); });
      th.replaceChildren(b);
    });

    /* A mouse gesture lights no row, because none of the five rows is about the
       mouse: unmarking is the honest thing for the table to do while the
       readout explains what just happened instead. */
    function act(next, kind, name) {
      desk = next;
      renderDesk(host, desk);
      mark('is-on', null);
      mark('is-hit', null);
      readout(out, 'Mouse', deskSayWindow(kind, name));
    }

    ui = buildPress(document.getElementById('how-press'), press);
    wireDesk(host, { get: function () { return desk; }, act: act, press: press });
    demo.classList.add('is-live');

    /* The shipped transcript describes the LOOP, which is what a JS-off reader
       watches. Once the reader has the wheel it is describing something that is
       no longer on screen — after one click on the 5c row it was still talking
       about two Chrome windows — so it is replaced with what is true now.
       Rewriting it is only legal because the shipped sentence stands on its own
       for everyone who never gets here. */
    var steps = demo.querySelector('.demo-steps');
    if (steps) {
      steps.textContent = 'Pick a row to set the desk up, then press Caps Lock and a letter. ' +
        'The readout names what beckon did, and the row it came from lights up.';
    }

    onOs(function () { scene(desk ? currentStep() : '5a'); });

    /* Which row the desk is currently built from, so an OS change rebuilds the
       same scenario on the new chrome instead of resetting the reader. */
    function currentStep() {
      var on = rows.filter(function (r) { return r.classList.contains('is-on'); })[0];
      return on ? on.getAttribute('data-step') : '5a';
    }

    demos.push({ node: demo, press: press });
  }());


  /* --- routing: which demo hears the key ---------------------------------- */

  if (!demos.length) return;

  /* A quarter of the screen, or nobody gets the key. Low enough that a demo
     the reader is plainly looking at always answers, high enough that one
     peeking over the fold does not swallow anything. */
  function active() {
    var best = null, score = 0.25;
    demos.forEach(function (d) {
      var s = share(d.node);
      if (s > score) { score = s; best = d; }
    });
    return best;
  }

  document.addEventListener('keydown', function (e) {
    /* A chord of two or more modifiers is not "some shortcut, leave it alone" —
       on a machine where Caps is remapped to Hyper it IS the gesture the page
       just asked for. One modifier still bails, so Cmd+C stays copy. */
    var chord = capsChord(e);
    if (!chord && (e.metaKey || e.ctrlKey || e.altKey)) return;
    var t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'SELECT' ||
              t.tagName === 'TEXTAREA' || t.isContentEditable)) return;

    /* All five bindings are plain letters now — the terminal moved from Space
       to T so the examples stop contradicting themselves — so nothing here has
       to defer to a focused control. Space used to, because it activates a
       button, a link and a <summary>, and tabbing to a table row and pressing
       it ran the demo instead of choosing the scenario just focused. */
    var name = e.key;
    if (!deskAppOf(name)) return;

    var d = active();
    if (!d) return;

    /* Only the Caps path re-arms. A real chord carries its own modifiers on
       every event, so it needs no window — and re-arming from one would let
       the NEXT bare letter through, which is the leak this split avoids. */
    var viaCaps = !chord && capsHeld();
    /* The key is only swallowed when it actually drove a demo. A press the
       gate turned away must NOT be swallowed — a refused key should do
       whatever the browser would have done with it. */
    if (!d.press(name, chord || viaCaps)) return;
    e.preventDefault();
    if (viaCaps) capsKeepAlive();
  });


  /* --- the HUD ------------------------------------------------------------ */

  (function () {
    var hud = document.getElementById('hud');
    if (!hud) return;
    hud.appendChild(el('div', 'hud-cap', 'Keys'));
    APPS.forEach(function (a) {
      var b = el('button');
      b.type = 'button';
      b.setAttribute('aria-label', 'Caps Lock and ' + a.key + ' — ' + a.name);
      var chord = el('span', 'chord');
      chord.appendChild(el('kbd', 'key', 'Caps'));
      chord.appendChild(el('kbd', 'key', a.label));
      b.appendChild(chord);
      b.appendChild(el('span', null, a.name));
      b.addEventListener('click', function () {
        var d = active() || demos[0];
        d.node.scrollIntoView({ block: 'center' });
        /* `true`: a click is a pointer, and the caps gate is about teaching a
           keyboard gesture, not about gatekeeping the mouse. */
        d.press(a.key, true);
      });
      hud.appendChild(b);
    });
    hud.hidden = false;
  }());

}());
