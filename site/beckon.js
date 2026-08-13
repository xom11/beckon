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

  /* How much of an element is on screen, 0..1. Used to decide which demo a
     keypress belongs to. Cheap enough to run per keydown, which is why there is
     no IntersectionObserver here. */
  function share(node) {
    var r = node.getBoundingClientRect();
    var h = window.innerHeight || root.clientHeight;
    if (!r.height) return 0;
    var vis = Math.max(0, Math.min(r.bottom, h) - Math.max(r.top, 0));
    return vis / Math.min(r.height, h);
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
  function onOs(fn) { osSubs.push(fn); fn(root.dataset.os || 'linux'); }

  (function () {
    var wrap = document.getElementById('os-switch');
    var sel = document.getElementById('os-select');
    if (!wrap || !sel) return;

    sel.value = root.dataset.os || 'linux';
    wrap.hidden = false;
    sel.addEventListener('change', function () {
      root.dataset.os = sel.value;
      try { localStorage.setItem('beckon-os', sel.value); } catch (e) {}
      osSubs.forEach(function (fn) { fn(sel.value); });
    });
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

    var live = desk.wins.filter(function (w) { return !w.min; });

    /* sway does not stack, so its order on screen is CREATION order — a window
       keeps its place in the tree when it takes focus. Everywhere else the
       order is MRU and index 0 is the front. */
    var order = desk.os === 'linux'
      ? live.slice().sort(function (a, b) { return a.id - b.id; })
      : live;

    order.forEach(function (w, i) {
      var n = pool[w.id];
      if (!n) { n = makeWin(w); pool[w.id] = n; }
      if (n.parentNode !== wins) wins.appendChild(n);
      n.style.setProperty('--slot', String(i));
      n.style.zIndex = String(order.length - i);
      n.classList.toggle('is-focused', w.id === desk.focused);
    });

    /* Tiling is laid out by DOM order, so it has to be corrected explicitly. */
    if (desk.os === 'linux') {
      order.forEach(function (w) { wins.appendChild(pool[w.id]); });
    }

    /* A minimised window leaves the desk but stays lit in the dock, because it
       is still running — which is exactly the difference between step 5c and
       quitting. */
    Object.keys(pool).forEach(function (id) {
      var keep = order.some(function (w) { return String(w.id) === id; });
      if (!keep && pool[id].parentNode) pool[id].parentNode.removeChild(pool[id]);
    });

    [].forEach.call(host.querySelectorAll('.dock-app'), function (d) {
      var up = desk.wins.some(function (w) { return w.app === d.getAttribute('data-app'); });
      d.classList.toggle('is-up', up);
    });
  }

  /* A press row, and the HUD, are the same five buttons twice. Both are how a
     reader with no keyboard — or on a phone — takes part at all, so neither is
     decoration and neither is aria-hidden. */
  function keyButton(app, onKey) {
    var b = el('button', 'key', app.label);
    b.type = 'button';
    b.setAttribute('aria-label', app.key + ', ' + app.name);
    b.addEventListener('click', function () { onKey(app.key); });
    return b;
  }

  function buildPress(host, onKey) {
    if (!host) return null;
    var caps = {};
    APPS.forEach(function (a) {
      var b = keyButton(a, onKey);
      caps[a.key] = b;
      host.appendChild(b);
    });
    host.appendChild(el('span', 'press-hint', 'Press a key — or click one'));
    host.hidden = false;
    return caps;
  }

  function flash(caps, key) {
    var b = caps && caps[key];
    if (!b) return;
    b.classList.add('is-hit');
    setTimeout(function () { b.classList.remove('is-hit'); }, 160);
  }

  function readout(host, step, say) {
    if (!host) return;
    host.replaceChildren();
    host.appendChild(el('div', 'readout-step', step));
    host.appendChild(el('p', 'readout-say', say));
    host.hidden = false;
  }

  var demos = [];

  /* --- hero: three machines, one letter --- */

  (function () {
    var demo = document.getElementById('hero-demo');
    if (!demo) return;
    var slots = [].slice.call(demo.querySelectorAll('.desk-slot'));
    if (!slots.length) return;

    var machines = slots.map(function (slot) {
      var host = slot.querySelector('.desk');
      return {
        host: host,
        letter: slot.querySelector('.key.is-letter'),
        desk: deskMake(host.getAttribute('data-os'), DESK_SCENES.hero)
      };
    });

    var steps = demo.querySelector('.demo-steps');
    var caps = null;

    function press(key) {
      var app = deskAppOf(key);
      if (!app) return;
      var last = null;
      machines.forEach(function (m) {
        var r = deskPress(m.desk, key);
        m.desk = r.desk;
        renderDesk(m.host, m.desk);
        /* Only the LAST cap of each chord is rewritten — the letter. The
           modifiers are never touched, because "three different chords, one
           shared letter" is the sentence the hero is drawing. */
        if (m.letter) m.letter.textContent = app.label;
        last = r;
      });
      flash(caps, app.key);
      if (steps && last) {
        steps.textContent = 'All three machines, one press. ' + deskSay(last);
      }
    }

    caps = buildPress(document.getElementById('hero-press'), press);
    demo.classList.add('is-live');
    /* The static windows in the markup are the JS-off picture. They are cleared
       rather than adopted: renderDesk pools by window id, so leaving them in
       place would draw every window twice. The rebuilt picture is identical, so
       nothing moves at the swap. */
    machines.forEach(function (m) {
      m.host.querySelector('.desk-wins').replaceChildren();
      renderDesk(m.host, m.desk);
    });
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
    var caps = null;

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

    function press(key) {
      var app = deskAppOf(key);
      if (!app || !desk) return;
      var r = deskPress(desk, key);
      desk = r.desk;
      renderDesk(host, desk);
      flash(caps, app.key);
      mark('is-on', null);
      mark('is-hit', r.step);
      readout(out, 'Step ' + r.step, deskSay(r));
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

    caps = buildPress(document.getElementById('how-press'), press);
    demo.classList.add('is-live');

    /* The shipped transcript describes the LOOP, which is what a JS-off reader
       watches. Once the reader has the wheel it is describing something that is
       no longer on screen — after one click on the 5c row it was still talking
       about two Claude windows — so it is replaced with what is true now.
       Rewriting it is only legal because the shipped sentence stands on its own
       for everyone who never gets here. */
    var steps = demo.querySelector('.demo-steps');
    if (steps) {
      steps.textContent = 'Pick a row above to set the desk up, then press a key. ' +
        'The readout names the step that fired, and the row it came from lights up.';
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

  function active() {
    var best = null, score = 0.5;      /* half on screen, or nobody gets it */
    demos.forEach(function (d) {
      var s = share(d.node);
      if (s > score) { score = s; best = d; }
    });
    return best;
  }

  document.addEventListener('keydown', function (e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    var t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'SELECT' ||
              t.tagName === 'TEXTAREA' || t.isContentEditable)) return;

    var name = e.key === ' ' || e.key === 'Spacebar' ? 'Space' : e.key;
    if (!deskAppOf(name)) return;

    /* Space is the one bound key a focused control already owns: it activates a
       button, a link and a <summary>. Letters are not, so only Space defers.
       Without this, tabbing to a table row and pressing Space ran the demo
       instead of choosing the scenario the reader had just focused. */
    if (name === 'Space' && t && t.closest &&
        t.closest('button, a, summary, details, select, input, textarea')) return;

    var d = active();
    if (!d) return;
    /* Only swallowed once a demo owns more than half the viewport, which is
       what keeps Space usable for scrolling everywhere else on the page. */
    e.preventDefault();
    d.press(name);
  });


  /* --- the HUD ------------------------------------------------------------ */

  (function () {
    var hud = document.getElementById('hud');
    if (!hud) return;
    hud.appendChild(el('div', 'hud-cap', 'Keys'));
    APPS.forEach(function (a) {
      var b = el('button');
      b.type = 'button';
      b.setAttribute('aria-label', a.key + ', ' + a.name);
      var cap = el('kbd', 'key', a.label);
      b.appendChild(cap);
      b.appendChild(el('span', null, a.name));
      b.addEventListener('click', function () {
        var d = active() || demos[0];
        d.node.scrollIntoView({ block: 'center' });
        d.press(a.key);
      });
      hud.appendChild(b);
    });
    hud.hidden = false;
  }());

}());
