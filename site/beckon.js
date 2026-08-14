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
    /* The button carries no words, so this is now the ONLY name it has — and
       the glyph beside it is the same promise drawn: sun while dark, moon while
       light, i.e. the theme you get by pressing rather than the one you are in.
       Written here rather than in CSS because "follow the system" is a third
       state with no `[data-theme]` to key off. */
    var label = function () {
      var dark = isDark();
      btn.setAttribute('aria-label',
        dark ? 'Switch to the light theme' : 'Switch to the dark theme');
      btn.dataset.icon = dark ? 'sun' : 'moon';
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

  /* --- what the reader just pressed, drawn on the desk itself ---------------
   *
   * A press moved a window and nothing on the desk said why. On the hero that
   * was survivable — the reader's own finger was on the key — but #how presses
   * for them on a timer, and a window that rearranges itself with no visible
   * cause is the "it just jumped" complaint this exists to answer.
   *
   * ON THE BAR'S RAIL, right-hand end, centred on the bar's own centre line —
   * the strip below `.desk-wins`, which is the one part of the desk no window
   * can be dragged or resized onto. It used to float in the bottom-right
   * corner of the work area, which the cascade never reaches but a reader's
   * own mouse can. `--bar-mid` in beckon.css holds the line.
   *
   * It prints `Caps + <letter>`, which is the gesture the hero's chord rows
   * teach, not the raw chord — the whole page's claim is that Caps Lock stands
   * in for the reader's real modifier, and this is the same sentence said in
   * the same words one screen down.
   *
   * BUILT HERE, NEVER IN THE MARKUP, like `.win-grip`: without this file
   * nothing can press anything, so a keycap sitting on a JS-off desk would be
   * announcing a gesture that page cannot answer.
   *
   * THE CAP GOES UP FIRST AND THE DESK ANSWERS A BEAT LATER. The two used to
   * happen in the same frame, which reads as one event with two halves — and
   * the entire job of this cap is to say WHY the desk moved, which is a claim
   * about order. A cause that arrives with its effect is not read as a cause.
   *
   * `KEY_LEAD_MS` is deliberately near the floor of what a reader can separate:
   * far enough that the order is felt, short enough that nobody reads it as the
   * page being slow. It is the same shape as the real thing — a machine lights
   * its indicator off the keystroke and gets round to moving the window after.
   *
   * IT IS PAIRED WITH THE `.09s` FADE-IN in beckon.css and neither number means
   * much alone: the cap finishes arriving as the desk starts moving, so the
   * thing doing the causing is fully on screen before the thing being caused
   * stirs. Raising the lead without raising that fade just adds dead air. The
   * fade-OUT stays slow, which is what an OSD on all three of these machines
   * does. */
  var KEY_LEAD_MS = 90;    /* cap first, desk this much later */
  var KEY_HOLD_MS = 1000;  /* and the cap stays up this long once it is there */

  function keyHud(host, letter, then) {
    /* A press still owing its desk change is settled NOW, before this one is
       drawn. Two presses inside the lead window would otherwise resolve out of
       order, and the later one would be painted over by the earlier one. */
    keyHudSettle(host);

    var hud = host._hud;
    if (!hud) {
      hud = el('div', 'desk-key');
      hud.setAttribute('aria-hidden', 'true');   /* the readout is the accessible answer */
      hud.appendChild(el('kbd', 'desk-cap is-caps', 'Caps'));
      hud.appendChild(el('span', 'desk-plus', '+'));
      hud.appendChild(el('kbd', 'desk-cap is-letter', ''));
      host.appendChild(hud);
      host._hud = hud;
    }
    hud.lastChild.textContent = letter;
    /* Restart the animation on a second press of the same key: an animation
       only replays if it is taken off and put back with a reflow in between,
       and `void offsetWidth` is the reflow. Without it, pressing C twice shows
       the cap once and the reader reads the second press as having done
       nothing. */
    hud.classList.remove('is-on');
    void hud.offsetWidth;
    hud.classList.add('is-on');
    clearTimeout(host._hudOff);
    host._hudOff = setTimeout(function () { hud.classList.remove('is-on'); }, KEY_HOLD_MS);

    if (!then) return;
    host._hudThen = then;
    host._hudLead = setTimeout(function () { keyHudSettle(host); }, KEY_LEAD_MS);
  }

  /* Pay whatever the last press still owes, now. Idempotent, and it takes the
     debt off the books BEFORE running it: `keyHud` opens by settling, so a
     `then` that ever reached a press would otherwise re-enter this with its own
     entry still pending and run it twice. Nothing passes such a `then` today —
     the ordering is what keeps that from being a thing to remember. */
  function keyHudSettle(host) {
    var then = host._hudThen;
    keyHudDrop(host);
    if (then) then();
  }

  function keyHudDrop(host) {
    if (host._hudLead) { clearTimeout(host._hudLead); host._hudLead = null; }
    host._hudThen = null;
  }

  /* Withdraw the cap, and the press behind it.
     DROPPED, NOT SETTLED, and that is the whole reason this is a function
     rather than one line at each call site. Every caller here has just redrawn
     the desk itself, so a deferred render still holding the press's own
     snapshot would repaint the desk that press left rather than the one now on
     screen — an OS switch would land back on the previous machine's layout. */
  function keyHudClear(host) {
    keyHudDrop(host);
    clearTimeout(host._hudOff);
    if (host._hud) host._hud.classList.remove('is-on');
  }

  /* Hang a class on a node for one animation's length.
     THE CLASS CANNOT BE LEFT ON. `renderDesk` takes a minimised window OUT of
     the DOM and puts it back when it is restored, and a node re-inserted still
     wearing `.is-new` replays the opening animation — an app that has been
     running for a minute appearing to launch itself. The timeout is deliberately
     longer than the longest animation here (.3s on Linux) and deliberately not
     an `animationend` listener: that event bubbles from the title bar and the
     lights too, so it would need a target check to be correct, and a timer needs
     nothing to be correct. */
  function flashFor(node, cls) {
    /* Off, reflow, on — an animation replays only if the class is taken away
       and put back with a style recalculation in between. A second scene set
       inside the timeout below would otherwise cut with no fade, because
       `classList.add` of a class already there does nothing. The forced layout
       is one per press or per scene change, both of which are human-paced. */
    node.classList.remove(cls);
    void node.offsetWidth;
    node.classList.add(cls);
    setTimeout(function () { node.classList.remove(cls); }, 700);
  }

  /* --- a window leaving the desk -------------------------------------------
   *
   * Hiding used to be a `removeChild` in one frame, which looks exactly like a
   * scene cut — so the one branch that takes something away was drawn with the
   * same non-verb as the machinery between branches. The window now shrinks
   * into its own dock icon, which is what all three of these machines draw, and
   * `ox`/`oy` are that icon's centre expressed relative to the window's own top
   * left, so it lands on the right icon on each.
   *
   * THE NODE OUTLIVES THE ANIMATION BY 420ms ON PURPOSE. A reader can click the
   * dock icon to bring the window straight back, and `renderDesk` restores it
   * by un-hiding this same node — which still carries wherever they dragged it
   * to. `unhide` cancels the removal, so the two cannot race.
   *
   * The timer re-checks `parentNode` because `scene()` throws the pool away and
   * may have detached this node in the meantime. */
  function hideFor(node, ox, oy) {
    if (node._gone) return;
    node.style.transformOrigin = ox + 'px ' + oy + 'px';
    node.classList.add('is-gone');
    node._gone = setTimeout(function () {
      node._gone = null;
      if (node.parentNode) node.parentNode.removeChild(node);
    }, 700);
  }

  /* Called unconditionally for every window `renderDesk` is about to show, and
     unconditionally is the point: without it, clicking a dock icon during the
     420ms grace above leaves the node in the DOM (so the `parentNode` check
     skips re-appending it), still wearing `.is-gone` (so it is invisible), with
     a live timer that then deletes a window the model says has focus. */
  function unhide(node) {
    if (node._gone) { clearTimeout(node._gone); node._gone = null; }
    if (node.classList.contains('is-gone')) {
      node.classList.remove('is-gone');
      node.style.transformOrigin = '';
    }
  }

  /* The tour's Pause/Play. Returns a `set(paused)` the tour calls whenever it
     changes state for any reason, so the button can never disagree with what
     the desk is doing — including when a reader takes over by clicking a row.
     NO `aria-pressed`: the label already carries the state, and a button that
     says "Play" while reporting pressed=false is two answers to one question. */
  function buildTour(hostEl, onToggle) {
    if (!hostEl) return null;
    var b = el('button', 'ghost', 'Pause');
    b.type = 'button';
    b.addEventListener('click', onToggle);
    hostEl.appendChild(b);
    hostEl.hidden = false;
    return {
      set: function (paused) {
        b.textContent = paused ? 'Play' : 'Pause';
        b.setAttribute('aria-label', paused
          ? 'Play the walk through the five branches'
          : 'Pause the walk through the five branches');
      }
    };
  }

  /* Window elements are POOLED BY ID and never rebuilt, because the movement is
     the point: a replaced node has no previous transform for the transition in
     §4 to interpolate from, so a rebuild-per-press would make every raise
     teleport.

     `born` is the id of a window that did not exist before this render, and it
     comes from `deskPress`'s launch branch — the ONLY branch that returns one.
     It is not inferred here, and it deliberately cannot be: "a node that is not
     in the pool" would also catch every window on the desk the first time it is
     drawn and every window again after an OS switch, which would open the hero
     with three launch animations for three apps that were already running. */
  function renderDesk(host, desk, born) {
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

    /* WHICH WINDOW OF ITS APP THIS IS, numbered along the ring `deskPress`
       actually walks — `mine.sort(by id)` at desk.js:202-206 — so "Chrome — 2"
       is the window the next press really goes to. This is the only channel the
       Cycle branch has: two windows of one app are two rectangles with the same
       name, and a press that swaps which is in front is invisible unless they
       can be told apart. An app with one window stays plain "Chrome"; a numeral
       there would be noise about a ring of one.
       COMPUTED OVER `desk.wins`, NOT `order`. `order` drops minimised windows,
       so minimising one of two Chromes would make the numeral vanish from the
       other and come back on restore — the number would be describing the
       drawing rather than the ring. */
    var rank = {}, seen = {}, total = {};
    desk.wins.forEach(function (w) { total[w.app] = (total[w.app] || 0) + 1; });
    desk.wins.slice().sort(function (a, b) { return a.id - b.id; })
      .forEach(function (w) { seen[w.app] = (seen[w.app] || 0) + 1; rank[w.id] = seen[w.app]; });

    order.forEach(function (w, i) {
      var n = pool[w.id];
      if (!n) {
        n = makeWin(w); pool[w.id] = n; n.setAttribute('data-id', String(w.id));
        if (w.id === born) flashFor(n, 'is-new');
      }
      /* Before the parentNode test, and unconditionally — see `unhide`. */
      unhide(n);
      if (n.parentNode !== wins) wins.appendChild(n);
      /* Written on every render rather than once in `makeWin`: nodes are pooled,
         so a window that becomes the second of its app long after it was
         created would otherwise keep the name it was born with. */
      var nm = n.querySelector('.win-name');
      if (nm) nm.textContent = total[w.app] > 1 ? w.app + ' — ' + rank[w.id] : w.app;
      /* POSITION FROM THE WINDOW, STACKING FROM THE ORDER. The window's own
         slot never changes, so a raise leaves it exactly where it was and only
         brings it in front — which is what raising a window looks like.
         WRAPPED AT `DESK_SLOTS`, WHICH IS FIVE, and the model hands out five
         distinct places for five letters, so this modulo is a guard rather than
         the thing that decides. It used to be a bare `% 4` against a model that
         counted launches without limit, and that threw away the guarantee
         desk.js's own test asserts — "a launched window takes a new place, not
         somebody else's" was true of `w.slot` and false of what got drawn.
         A window the reader has DRAGGED owns its position outright, and this
         must not take it back: `_placed` is set by the drag, and from then on
         the cascade has nothing to say about where that window sits. */
      if (!n._placed) n.style.setProperty('--slot', String(w.slot % DESK_SLOTS));
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
       launching looks like.

       MINIMISE AND CLOSE PART COMPANY HERE, and they did not used to. Both were
       one `removeChild`, so hiding a window — the whole of step 5c — was drawn
       with the same nothing as a scene cut. A close still goes in one frame,
       which is right: the window is gone from the model and there is nothing to
       come back to. A hide shrinks into the app's own dock icon, where the icon
       stays lit, which is exactly the difference the branch is about. */
    Object.keys(pool).forEach(function (id) {
      var shown = order.some(function (w) { return String(w.id) === id; });
      var known = desk.wins.some(function (w) { return String(w.id) === id; });
      var n = pool[id];
      if (!shown && n.parentNode) {
        if (known && !n._gone) {
          var w2 = desk.wins.filter(function (x) { return String(x.id) === id; })[0];
          var icon = w2 && host.querySelector('.dock-app[data-app="' + w2.app + '"]');
          var nb = n.getBoundingClientRect();
          var ox = nb.width / 2, oy = nb.height / 2;
          if (icon) {
            var ib = icon.getBoundingClientRect();
            ox = ib.left + ib.width / 2 - nb.left;
            oy = ib.top + ib.height / 2 - nb.top;
          }
          hideFor(n, ox, oy);
        } else if (!known) {
          n.parentNode.removeChild(n);
        }
      }
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
    var bornApp = null;
    if (born) {
      var b = desk.wins.filter(function (x) { return x.id === born; })[0];
      bornApp = b ? b.app : null;
    }
    [].forEach.call(host.querySelectorAll('.dock-app'), function (d) {
      var app = d.getAttribute('data-app');
      d.classList.toggle('is-up', desk.wins.some(function (x) { return x.app === app; }));
      d.classList.toggle('is-focused', app !== null && app === front);
      /* The icon of the app that just launched moves, because the state change
         it already has — .32 to lit — happens over 300ms of opacity in the
         corner of the eye while the reader is watching the middle of the desk. */
      if (app === bornApp) flashFor(d, 'is-launching');
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

  /* .win's own base rule in beckon.css, in fractions of THE WORK AREA — the
     `.desk-wins` box, which stops short of the desk's bottom edge by whatever
     the bar owns. Every rect this file measures for a drag is that box and not
     the desk, or the two disagree by `--bar-h` and a window jumps the moment it
     is picked up.
     A drag has to turn the percentage layout into pixels before it can add a
     delta to it, so the two files agree on these four numbers by hand. Change
     one, change both; `place` below is the only reader. */
  var WIN_X = .08, WIN_Y = .12;
  var MIN_W = .22, MIN_H = .24;    /* no shrinking a window into a sliver */

  function clamp(v, lo, hi) { return v < lo ? lo : v > hi ? hi : v; }

  function pxOf(node, prop) {
    var v = parseFloat(getComputedStyle(node).getPropertyValue(prop));
    return isNaN(v) ? 0 : v;
  }

  /* Turn a cascaded window into a placed one, once. Until the reader touches
     it, a window sits where `--slot` puts it — an offset in percent OF ITS OWN
     WIDTH, so resizing it would also move it. From the first drag onward it is
     pixels from the work area's origin and only the drag writes them. */
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

      /* The WORK AREA's rect, not the desk's — `.win` is positioned inside
         `.desk-wins`, so that is the box its percentages resolve against and
         the box a drag has to be clamped to. Off `host` both go wrong by the
         bar: `place` would write a `--dy` short by `WIN_Y x --bar-h` and the
         window would hop the moment it was picked up, and the bottom clamp
         would be a whole `--bar-h` too generous — letting a drag put a window
         over the dock, which is the overlap `.desk-wins` exists to make
         unrepresentable. Measured with this line in place: 0.00px of movement
         on pointerdown. */
      var box = wins.getBoundingClientRect();
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
      /* Clamped to the work area on both axes. A window dragged off the edge of
         a picture of a desktop does not read as a window behind a bezel; it
         reads as the demo losing one — and one dragged over the dock reads as
         the demo being broken. */
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
  /* THESE ARE THE CHORD ROWS READ BACK, in words, and the two must not drift:
     the row draws the keys and this sentence names them, about six lines apart
     on screen. So it is spelled out rather than glyphed — a hint that printed
     ⌘ would be repeating the picture instead of captioning it — and each one
     lists its modifiers in the row's own order, which is the order the keys sit
     in under the reader's hand. */
  var CHORD_OF = {
    macos: 'Control, Option and Command',
    windows: 'Ctrl, the Windows key and Alt',
    linux: 'Ctrl, Super and Alt'
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
       ones look absent.
       This comment used to end "the tile list in #setups is where they are
       enumerated". That section is gone: the 2026-08 redesign deleted the
       compatibility grid, and the page now names no compositor anywhere — the
       three machines are the whole vocabulary, and the enumeration lives in the
       README. This is a switch between three machines, and that is all it is. */
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
      /* A new machine has not been pressed on yet, so the cap from the last one
         must not still be sitting on the bar claiming otherwise — nor may the
         desk change that cap was still leading, which belongs to the machine
         just discarded. */
      keyHudClear(host);
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
      /* THE KEY, THEN THE ANSWER. Everything that is about the gesture runs
         now; everything that is about what beckon DID with it is handed to
         `keyHud` and runs a beat later. `r` is captured rather than re-read
         from `desk`, so if a second press arrives inside that beat the first
         one still settles onto its own snapshot before the second draws.

         `r.born` is set by the launch branch and by no other, so this is the
         one call site on the hero that can open a window rather than move one.
         `act` below never passes it: no mouse gesture launches anything, and
         closing an app and pressing its key again is a launch that comes back
         through here. */
      keyHud(host, app.label, function () {
        renderDesk(host, r.desk, r.born);
        if (steps) steps.textContent = deskSay(r);
      });
      if (ui) ui.flash(app.key);
      /* Only the LAST cap of each chord is rewritten — the letter. The
         modifiers are never touched: "one letter, whatever your modifier is"
         is the sentence these rows are drawing. */
      letters.forEach(function (l) { l.textContent = app.label; });
      return true;
    }

    /* The mouse's own transcript replaces the keyboard's, because both describe
       the same desk and only one of them can be true at a time.
       A press still inside its lead is DROPPED rather than settled: `desk` was
       updated the moment that key was pressed, so `next` already contains what
       it did, and the full redraw below draws it. Settling instead would repaint
       the pre-drag desk on top of the drag. */
    function act(next, kind, name) {
      keyHudDrop(host);
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

    /* THE READER-HAS-TAKEN-OVER HOOK, and `takeover` starts as a NO-OP rather
       than as null on purpose. `onOs` at the foot of this closure calls its
       callback SYNCHRONOUSLY the moment it is registered, and it runs BEFORE
       the tour IIFE that assigns the real `pause` here — so a direct reference
       would throw a ReferenceError out of the outermost IIFE and take the
       keyboard router and the HUD down with it. `inTour` is how the two shared
       entry points below tell the tour's own calls from a reader's. */
    var inTour = false;
    var takeover = function () {};

    function mark(cls, step) {
      rows.forEach(function (r) {
        r.classList.toggle(cls, step !== null && r.getAttribute('data-step') === step);
      });
    }

    function scene(step) {
      desk = deskMake(root.dataset.os || 'linux', DESK_SCENES[step]);
      host.setAttribute('data-os', desk.os);
      host._pool = {};                       /* a new scene is new windows */
      var wins = host.querySelector('.desk-wins');
      wins.replaceChildren();
      renderDesk(host, desk);
      /* A SCENE CHANGE IS A CUT, AND IT HAS TO LOOK LIKE ONE. Building the desk
         swaps every window on it in a single frame, which reads as the windows
         having moved — the same misreading `.is-new` exists to prevent, one
         level up. A short fade over the whole `.desk-wins` says "different
         desktop" instead of "these windows did something", and it is on the
         CONTAINER rather than on each window precisely so it cannot be mistaken
         for five things opening at once. */
      flashFor(wins, 'is-cut');
      /* The key cap belongs to the press, not to the setup, so a cut clears it:
         leaving `Caps T` on screen next to a desk that has just been rebuilt
         claims the reader pressed something to get here. The desk change that
         cap was leading goes with it — this scene is what is on screen now, and
         a press from the previous one must not land on top of it. */
      keyHudClear(host);
      mark('is-on', step);
      mark('is-hit', null);

      /* THE LABEL IS THE BRANCH NAME IN BOTH STATES, and the sentence under it
         is what changes tense. Setting a scene reads "Launch — Chrome is not
         running."; the press that follows reads "Launch — Chrome was not
         running. beckon launched it." One word, present then past, so a reader
         watching the tour sees the same name twice and reads the second line as
         the answer to the first.

         It used to say "Ready", which named a state of the demo rather than
         anything beckon does, and left the five words this section exists to
         teach appearing only for the half-second after a press.

         THE SENTENCE IS DERIVED FROM THE DESK, NOT WRITTEN OUT PER STEP. It
         used to echo the table row — "not running. Press a key." — which was
         accurate and abstract at the same time: the picture beside it has
         actual windows with actual names in them, and the line under it named
         none of them. `readyLine` reads the scene the desk was just built from,
         so it cannot drift from what is on screen, and it stays correct if a
         scene in desk.js is ever changed.

         `deskSceneKey` and not a hard-coded 'c': the Launch scene presses T on
         an empty desk, and the sentence has to be about the app that press is
         going to open. */
      readout(out, deskStepName(step), readyLine(desk, deskSceneKey(step)));
      if (!inTour) takeover();
    }

    /* What is true right now, for the app the tour presses. Short, and it names
       the app: "Chrome is not running." beats "not running." beside a desktop
       that is drawing Chrome. */
    function readyLine(d, key) {
      var app = deskAppOf(key);
      if (!app || !d) return '';
      var mine = d.wins.filter(function (w) { return w.app === app.name; });
      if (!mine.length) return app.name + ' is not running.';
      var f = d.wins.filter(function (w) { return w.id === d.focused; })[0];
      if (!f || f.app !== app.name) {
        return app.name + ' is open, behind ' + (f ? f.app : 'another window') + '.';
      }
      if (mine.length > 1) return app.name + ' is focused. Two windows.';
      var others = d.wins.filter(function (w) { return w.app !== app.name; });
      /* "So is Terminal." was the shorter phrasing and it was WRONG: it says
         the other app is focused too, which is the one thing this scene is
         about not being true. Two words back for a sentence that is correct. */
      if (others.length) return app.name + ' is focused. ' + others[0].app + ' is open.';
      return app.name + ' is focused. Nothing else is open.';
    }

    function press(key, ok) {
      var app = deskAppOf(key);
      if (!app || !desk) return false;
      if (!ok) { if (ui) ui.miss(); return false; }
      /* AFTER the gate, not before it. A bare `t` this demo rejects changes
         nothing on screen, so it must not stop the tour either — that is the
         small version of the `document` keydown bug the tour no longer has. */
      if (!inTour) takeover();
      var r = deskPress(desk, key);
      desk = r.desk;
      /* The cap now, the answer a beat later — see `keyHud`. It matters more
         here than on the hero: the tour presses for the reader, so there is no
         finger on a key to supply the cause, and the cap IS the cause.
         The row that lights up and the readout under the desk are both answers
         to the press, so they go with the desk rather than with the cap. */
      keyHud(host, app.label, function () {
        renderDesk(host, r.desk, r.born);      /* born is set by the launch branch only */
        mark('is-on', null);
        mark('is-hit', r.step);
        /* The branch's NAME, the same word the row that just lit up prints. It
           used to read "Step 5a", which named nothing a reader could match to
           anything on screen. */
        readout(out, deskStepName(r.step), deskSay(r));
      });
      if (ui) ui.flash(app.key);
      return true;
    }

    /* THE WHOLE ROW IS THE TARGET, not the words in its first cell. The row is
       one statement — a condition and what beckon does about it — and only the
       left half of it used to be clickable, so the obvious place to aim (the
       branch name, which is the part set in full ink) did nothing.

       ONE HANDLER, ON THE `tr`. The button stays because a table cell is not
       focusable and a mouse-only target is not a target for everyone; it no
       longer carries its own listener, so Enter or Space on the focused button
       fires a click that bubbles to the row and takes the same path a mouse
       does. Two handlers would have run `scene` twice for one click.

       The markup still ships plain text: a disabled or inert button in the
       markup would be a control that silently does nothing, which is the one
       thing this page does not ship. */
    rows.forEach(function (r) {
      var th = r.querySelector('th');
      var step = r.getAttribute('data-step');
      if (!th || !step) return;
      var b = el('button', 'row-btn', th.textContent.trim());
      b.type = 'button';
      b.setAttribute('aria-label', 'Set the desk up: ' + th.textContent.trim());
      th.replaceChildren(b);
      r.addEventListener('click', function () { scene(step); });
    });
    /* Only now is any of the row a target, so only now may it look like one. */
    table.classList.add('is-live');

    /* A mouse gesture lights no row, because none of the five rows is about the
       mouse: unmarking is the honest thing for the table to do while the
       readout explains what just happened instead.
       A press still inside its lead is dropped rather than settled, for the
       reason the hero's `act` gives. */
    function act(next, kind, name) {
      keyHudDrop(host);
      desk = next;
      renderDesk(host, desk);
      mark('is-on', null);
      mark('is-hit', null);
      readout(out, 'Mouse', deskSayWindow(kind, name));
      if (!inTour) takeover();
    }

    /* NO PRESS ROW IN THIS SECTION any more — the element is gone from the
       markup, not merely hidden. `buildPress` returns null for a null host and
       every call site below is already guarded, so this stays a one-word change
       rather than a special case: the desk is still driven by `wireDesk`, the
       row buttons still set scenes, and the document's keydown routing still
       reaches `press`. What is lost is the flash on a hit and the nudge on a
       miss, both of which were painting a control that is no longer there.
       The hero prints the gesture one screen up; printing it twice was pushing
       the readout — the one line that actually changes per branch — down. */
    ui = buildPress(document.getElementById('how-press'), press);
    wireDesk(host, { get: function () { return desk; }, act: act, press: press });
    demo.classList.add('is-live');

    /* THE TRANSCRIPT IS HIDDEN, NOT REWRITTEN. It describes the LOOP, which is
       what a JS-off reader watches, and it has to stay in the markup for them.
       Once the reader has the wheel the readout says the same kind of thing per
       branch and says it about what is on screen right now, so a second caption
       under it was two captions for one picture. The previous version rewrote
       this paragraph into a third sentence about how to use the section, which
       is the copy this change exists to delete. */
    var steps = demo.querySelector('.demo-steps');
    if (steps) steps.hidden = true;

    /* THE ARRIVAL SCENE IS LAUNCH, and it used to be Cycle. The tour's first
       act is now a press rather than a scene build, so whatever is on the desk
       when a reader arrives is what that press acts on — and the press this
       section should open with is the one the whole page is about: an empty
       desk, a key, an app. Cycle as the opener meant the first thing on screen
       was three windows disappearing at once to make room for it.
       This is called synchronously, before the tour exists — which is why
       `takeover` above must already be callable. */
    onOs(function () { scene(desk ? currentStep() : '4'); });

    /* Which row the desk is currently built from, so an OS change rebuilds the
       same scenario on the new chrome instead of resetting the reader. */
    function currentStep() {
      var on = rows.filter(function (r) { return r.classList.contains('is-on'); })[0];
      return on ? on.getAttribute('data-step') : '4';
    }

    /* The idle tour.
     *
     * `is-live` above turns off the CSS loop a JS-off reader watches, so with
     * JS on this section stood perfectly still until somebody pressed
     * something. Measured on the deployed page: eight seconds, not one pixel.
     * A reader who scrolls past therefore learned nothing from the one section
     * carrying the whole argument, and the two sentences under the desk had to
     * do a job a picture was right there to do.
     *
     * It drives `scene` and `press` — the same two functions the row buttons
     * and the key caps call, `press` with the same `true` a cap click passes —
     * so it can never show a state a reader could not reach themselves, and
     * desk.js stays the only thing that decides what a press does.
     *
     * THE LETTER COMES FROM `deskSceneKey`, which is DESK_SCENES' own pairing
     * rather than a choice made here — a scene and the key that answers it are
     * one fact and they live together. It was `c` in every scene until the
     * Launch scene became an empty desk, which is the one case where the key
     * has something to open rather than something to move.
     */
    (function () {
      if (!window.IntersectionObserver) return;

      /* NO `prefers-reduced-motion` CHECK ANY MORE, and its removal is the
         point rather than an oversight. It used to `return` here, so a reader
         who asks for less motion got a still photograph of the one section
         whose job is to show five different answers — measured over 12s with a
         control: 5-11 distinct states at `no-preference`, exactly 1 at
         `reduce`. The preference is about motion, not about information; the
         difference now lives entirely in beckon.css §8, where every beat that
         moves or resizes becomes a cut and every beat that changes colour or
         opacity keeps its duration. There is no `matchMedia` branch anywhere in
         this file, and adding one back would split the choreography across two
         languages. */

      var STEPS = ['4', '5', '5a', '5b', '5c'];

      /* THE PACE IS DERIVED FROM THE WORD COUNT, not chosen. The tour prints
         two lines per turn — the precondition and the answer — and at 238 wpm
         (Brysbaert 2019, a meta-analysis of 190 studies of silent English
         reading) the old copy came to 24,958 ms of reading inside a 20,000 ms
         loop of two constants. Every turn replaced a sentence the reader had
         not finished.
         So the copy was cut to 71 words (see `deskSay` and `readyLine`) and
         each branch got its own pair:
             READ   = 300 + words x 252,  rounded to 50, floor 1400
             ANSWER = 300 + words x 252 + 400
             TURN   = SEAM + READ + KEY_LEAD_MS + ANSWER
         252ms is 60000/238; the 300 is the eye's trip from the table row to the
         readout, and the 400 is a rest before the seam. `BEATS` is [READ, TURN]
         and the loop comes to 25,250 ms. */
      var BEATS = {
        '4':  [1550, 4500],
        '5':  [1800, 4750],
        '5a': [1800, 5250],
        '5b': [2050, 5250],
        '5c': [2050, 5500]
      };
      var SEAM = 400;      /* the dip's whole length, `deskSeam` in §4 */
      var SEAM_MID = 180;  /* ...and the bottom of it, where the scene swaps */
      var CUE_AT = 260;    /* the ring, once the desk is back up */
      var ARRIVE = 700;    /* a beat before the FIRST press, so a reader who
                              lands mid-scroll sees the empty desk it acts on */
      var RESUME = 900;    /* ...and a longer one after Play, which is a
                              deliberate act and can afford to be answered */

      /* IN THE TABLE'S OWN ORDER, top to bottom, and that is the whole point of
         the mark: it walks the list the way the reader reads it.
         IT OPENS ON A PRESS, NOT ON A SCENE, and that is what makes the first
         thing a reader ever sees the sentence they were promised: an empty
         desk, a cap in the corner, a Terminal opening. It used to open by
         BUILDING a scene, so the first event on screen was three windows
         vanishing at once with nobody pressing anything — which is what Launch
         looks like, performed as stage machinery. `onOs` builds the Launch
         scene on arrival and `phase` starts at 'press', so the machinery has
         already happened before anyone is looking. */
      var at = 0, phase = 'press', paused = false, started = false;
      var timer = null, onScreen = false, pending = [];
      var tourUi = null;

      /* EVERY TIMER THE TOUR SETS GOES THROUGH HERE, because a turn now has
         timers inside it — the scene lands mid-seam, the cue lands after that —
         and a reader who takes over at t=300 must not still receive a cue ring
         at t=440 on a desk that is no longer the tour's to draw. */
      function later(fn, ms) { pending.push(setTimeout(fn, ms)); }

      function clear() {
        if (timer) { clearTimeout(timer); timer = null; }
        pending.forEach(clearTimeout);
        pending = [];
      }

      function queue(ms) {
        if (paused || !onScreen || timer) return;
        timer = setTimeout(tick, ms);
      }

      /* `scene` and `press` are shared with the row buttons and the keyboard,
         and both call `takeover()` so that a reader's own action pauses the
         tour. The tour calls the same two functions, so it has to say which of
         them is speaking. */
      function drive(fn) { inTour = true; try { fn(); } finally { inTour = false; } }

      /* The ring that names the window this press is about to touch. It asks
         the MODEL rather than guessing: `deskPress` clones before it mutates,
         so calling it here to look at the answer is free and cannot drift from
         what the press will actually do a second later. */
      function cue(step) {
        if (!desk) return;
        var r = deskPress(desk, deskSceneKey(step));
        var ids = step === '4' ? []
          : step === '5c' ? [desk.focused]
          : step === '5a' ? [desk.focused, r.desk.focused]
          : [r.desk.focused];
        ids.forEach(function (id) {
          if (id === null || id === undefined) return;
          var n = host.querySelector('.win[data-id="' + id + '"]');
          if (n) flashFor(n, 'is-cued');
        });
      }

      function tick() {
        timer = null;
        if (paused) return;
        var step = STEPS[at];
        if (phase === 'seam') {
          /* The desk sinks, the new scene is built at the bottom of the dip
             where the swap cannot be read as an outcome, and the cue follows it
             out. */
          flashFor(host, 'is-seam');
          later(function () {
            drive(function () { scene(step); });
            later(function () { cue(step); }, CUE_AT);
          }, SEAM_MID);
          phase = 'press';
          queue(SEAM + BEATS[step][0]);
        } else {
          drive(function () { press(deskSceneKey(step), true); });
          at = (at + 1) % STEPS.length;
          phase = 'seam';
          queue(BEATS[step][1] - SEAM - BEATS[step][0]);
        }
      }

      function pause() {
        if (paused) return;
        paused = true;
        clear();
        /* Hand the table's marks back to their reader-driven meaning. */
        table.classList.toggle('is-touring', false);
        if (tourUi) tourUi.set(true);
      }

      /* Resume at the START of a turn, never mid-sentence: a tour that picked
         up at "press" would fire a key on a desk the reader has since
         rearranged, and the readout would answer a question nobody asked. */
      function play() {
        if (!paused) return;
        paused = false;
        phase = 'seam';
        table.classList.toggle('is-touring', true);
        if (tourUi) tourUi.set(false);
        queue(RESUME);
      }

      tourUi = buildTour(document.getElementById('how-tour'), function () {
        if (paused) play(); else pause();
      });
      takeover = pause;

      /* Capture-phase, so it lands before the control's own handler moves
         anything. The table is a sibling of the demo, not a child, so it needs
         its own listener.
         THE GUARD IS LOAD-BEARING: the Pause button lives INSIDE `#how-demo`,
         so without it a click on Play would pause on `pointerdown` and then
         un-pause on `click`, and the button could never pause anything.
         THERE IS NO `document` KEYDOWN LISTENER ANY MORE. There used to be, and
         it killed the tour permanently for any key pressed anywhere on the page
         — a Tab in the nav, before the reader had ever scrolled this far, left
         #how frozen with nothing to say why. Presses that actually reach this
         demo go through `press()`, which calls `takeover()` itself, and only
         after the hit/miss gate: a bare `t` that this demo rejects changes
         nothing on screen and so must not stop anything either. */
      function readerTook(e) {
        if (tourHost && tourHost.contains(e.target)) return;
        pause();
      }
      var tourHost = document.getElementById('how-tour');
      table.classList.add('is-touring');
      demo.addEventListener('pointerdown', readerTook, true);
      table.addEventListener('pointerdown', readerTook, true);

      /* The sentence that used to be prepended here — "It walks the five rows
         on its own, and stops for good the moment you touch it." — is gone with
         the rest of this section's prose. It was narrating something the reader
         can see happening: the mark moves down the table, the desk rebuilds
         under it, and the readout names each branch as it fires. A caption that
         describes a visible animation is the kind of text this redesign is
         removing, and it was the third sentence in a section whose whole point
         is that one key needs no explaining. */

      /* Off screen the tour would spend its laps unwatched and leave a reader
         arriving in the middle of a branch. Hold, and pick up at the start of a
         turn — never mid-sentence.
         `threshold: 0` and NOT a fraction: at the stacked breakpoint the demo
         is taller than a short window, so a threshold of .6 could never be
         reached and the section would stand still with nothing anywhere to say
         why. The "reader scrolled past too fast" problem is solved by ARRIVE
         and by opening on the Launch press instead. */
      new IntersectionObserver(function (entries) {
        onScreen = entries[0].isIntersecting;
        if (!onScreen) { clear(); return; }
        if (paused) return;
        if (!started) { started = true; queue(ARRIVE); }
        else { phase = 'seam'; queue(RESUME); }
      }, { threshold: 0 }).observe(demo);
    }());

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
