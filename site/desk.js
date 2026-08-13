/* beckon — the desk model.
 *
 * This file holds the landing page's ONLY copy of beckon's focus algorithm,
 * and it holds nothing else: no DOM, no styling, no event handling. That split
 * is the point. The page makes a claim about what beckon does when you press
 * the key a second time, and before this file that claim was carried by a
 * sentence sitting next to an animation — neither of which any test could
 * check. `press` is a pure function, so `site/desk.test.mjs` can walk all five
 * branches on every CI run.
 *
 * `press` is CLAUDE.md's *Focus algorithm* transcribed, in the order that
 * document tests its branches, and it is deliberately the same shape as
 * `beckon-linux`'s `algorithm::decide` — one entry point returning a decision,
 * with the caller responsible for drawing the result.
 *
 * CLASSIC SCRIPT, NOT A MODULE, and that is a requirement rather than a style
 * choice: `tools/check-site.sh` forbids external subresources partly so the
 * page opens from `file://`, and a browser refuses `<script type="module">`
 * over that scheme on CORS grounds. The `module.exports` tail at the bottom is
 * how `node --test` gets at the same source without a build step.
 */

/* Straight out of README.md's table. `tools/check-site.sh` pins the same five
   pairs into index.html, so a rebinding in examples/ cannot leave this map
   teaching the old letter while the page teaches the new one. */
/* `label` is what a keycap shows. It is the key's own name rather than a glyph
   for the same reason the keycap component accepts "Cmd" and "Super": U+2423
   OPEN BOX, the conventional mark for a space bar, renders at x-height in the
   UI stacks here and reads as a stray tick rather than as a key. */
var DESK_APPS = [
  { key: 'Space', name: 'terminal', label: 'Space' },
  { key: 'C', name: 'Claude', label: 'C' },
  { key: 'B', name: 'Brave', label: 'B' },
  { key: 'E', name: 'Cursor', label: 'E' },
  { key: 'D', name: 'Discord', label: 'D' }
];

function deskAppOf(letter) {
  var k = String(letter).toLowerCase();
  for (var i = 0; i < DESK_APPS.length; i++) {
    if (DESK_APPS[i].key.toLowerCase() === k) return DESK_APPS[i];
  }
  return null;
}

/* A desk is:
 *
 *   { os, wins: [ { id, app, min } ], focused }
 *
 * `wins` is MRU order, most recent first — the same thing every backend reads
 * out of the compositor (sway's tree walk, Hyprland's focusHistoryID, X11's
 * _NET_CLIENT_LIST_STACKING, CGWindowList on macOS, EnumWindows on Windows).
 * `id` doubles as the window's address: monotonic, assigned at creation, and
 * never reused. Step 5a depends on that — see below.
 *
 * `focused` is an id or null. Null is a real state, not an error: it is what
 * step 5c leaves behind.
 */
function deskMake(os, spec) {
  var wins = spec.map(function (w, i) {
    return { id: i + 1, app: w.app, min: !!w.min };
  });
  var first = wins.filter(function (w) { return !w.min; })[0];
  return {
    os: os,
    wins: wins,
    focused: first ? first.id : null,
    next: wins.length + 1
  };
}

function deskClone(d) {
  return {
    os: d.os,
    wins: d.wins.map(function (w) { return { id: w.id, app: w.app, min: w.min }; }),
    focused: d.focused,
    next: d.next
  };
}

function deskWin(d, id) {
  for (var i = 0; i < d.wins.length; i++) if (d.wins[i].id === id) return d.wins[i];
  return null;
}

/* Move a window to the head of the MRU list and give it focus. Un-minimises on
   the way, because every backend's focus path does: the X11 one maps the window
   before activating it, the KWin script clears `minimized` first, and
   Main.activateWindow on GNOME unminimises as part of its contract. */
function deskRaise(d, id) {
  var w = deskWin(d, id);
  if (!w) return d;
  w.min = false;
  d.wins = [w].concat(d.wins.filter(function (x) { return x.id !== id; }));
  d.focused = id;
  return d;
}

/* The algorithm. Returns the new desk and the step that fired, using the step
 * numbers CLAUDE.md itself uses — which is why the page prints them: they are a
 * citation, not decoration.
 *
 * Returns step `null` for a letter that is not bound to anything, so the caller
 * can stay silent rather than invent an outcome.
 */
function deskPress(desk, letter) {
  var app = deskAppOf(letter);
  if (!app) return { desk: desk, step: null, app: null };

  var d = deskClone(desk);
  var mine = d.wins.filter(function (w) { return w.app === app.name; });

  /* Step 4 — nothing of this app is running, so launch it. */
  if (mine.length === 0) {
    var born = { id: d.next++, app: app.name, min: false };
    d.wins = [born].concat(d.wins);
    d.focused = born.id;
    return { desk: d, step: '4', app: app };
  }

  var cur = d.focused === null ? null : deskWin(d, d.focused);

  /* Step 5 — running, but the reader is somewhere else. Take the most recent
     window of the app, which is the first one `wins` reports. */
  if (!cur || cur.app !== app.name) {
    return { desk: deskRaise(d, mine[0].id), step: '5', app: app };
  }

  /* Step 5a — the app owns another window, so walk its ring.
   *
   * THE RING IS ORDERED BY ID, NOT BY RECENCY, and that is load-bearing.
   * Picking "the least-recent other window of this app" reads correct and is a
   * 2-cycle on every backend whose recency is real focus history: focusing a
   * window promotes it and demotes the one you just left, so the next press
   * goes straight back and windows 3..N are unreachable. Ids are the
   * compositor's own addresses — stable for the window's lifetime, ordered by
   * creation — so rotating over them visits every window exactly once per lap.
   * Verified live on sway before it was written down: three foot windows, seven
   * presses, 35 -> 36 -> 37 -> 35. */
  if (mine.length > 1) {
    var ring = mine.slice().sort(function (a, b) { return a.id - b.id; });
    var at = ring.findIndex(function (w) { return w.id === cur.id; });
    var nxt = ring[(at + 1) % ring.length];
    return { desk: deskRaise(d, nxt.id), step: '5a', app: app };
  }

  /* Step 5b — one window, but something else is open: go back to it. */
  var other = d.wins.filter(function (w) { return w.app !== app.name; })[0];
  if (other) {
    return { desk: deskRaise(d, other.id), step: '5b', app: app };
  }

  /* Step 5c — one window and nothing else at all: hide it. */
  cur.min = true;
  d.focused = null;
  return { desk: d, step: '5c', app: app };
}

/* What the readout says. One sentence per step, written so it is true on a
   tiling compositor as well as a stacking one — hence "takes focus" rather than
   "comes to the front", which has no meaning on sway. */
function deskSay(res) {
  var n = res.app ? res.app.name : '';
  switch (res.step) {
    case '4':  return n + ' was not running. beckon launched it.';
    case '5':  return n + ' was already running. One press and it takes focus.';
    case '5a': return 'Still ' + n + '. The press walks to its next window, and wraps round at the end.';
    case '5b': return n + ' already had focus, so the press goes back to where you came from.';
    case '5c': return n + ' was the only thing open, so the press hides it.';
    default:   return '';
  }
}

/* The starting desks. Both live here rather than in the renderer because they
   are the preconditions the page makes claims about, and the tests assert on
   them directly. */
var DESK_SCENES = {
  /* Hero: three apps up, Brave in front, Cursor and Discord not installed yet.
     So C focuses Claude (step 5), Space focuses the terminal (5), E and D
     launch (4) — the obvious outcome for four of the five letters, and the
     fifth, B, is the interesting one the section below is about. */
  hero: [{ app: 'Brave' }, { app: 'Claude' }, { app: 'terminal' }],

  /* One per row of the algorithm table in #how. Pressing C in each of these is
     what makes that row's step fire; every other letter still runs the real
     algorithm and lights whichever row it lands on. */
  '4':  [{ app: 'Brave' }],
  '5':  [{ app: 'Brave' }, { app: 'Claude' }],
  '5a': [{ app: 'Claude' }, { app: 'Claude' }, { app: 'Brave' }],
  '5b': [{ app: 'Claude' }, { app: 'Brave' }],
  '5c': [{ app: 'Claude' }]
};

if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    DESK_APPS: DESK_APPS,
    DESK_SCENES: DESK_SCENES,
    deskAppOf: deskAppOf,
    deskMake: deskMake,
    deskPress: deskPress,
    deskSay: deskSay
  };
}
