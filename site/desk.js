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
/* `label` is what a keycap shows — the key's own name, the same way the keycap
   component spells out "Cmd" and "Super". */
var DESK_APPS = [
  { key: 'T', name: 'Terminal', label: 'T' },
  { key: 'C', name: 'Chrome', label: 'C' },
  { key: 'V', name: 'VS Code', label: 'V' },
  { key: 'F', name: 'Files', label: 'F' },
  { key: 'S', name: 'Spotify', label: 'S' }
];

/* HOW MANY PLACES THERE ARE ON THE DESK, and it is five because there are five
   letters. With four, pressing all five put the fifth window EXACTLY on top of
   the first — same left, same top, same size — and a reader who pressed F last
   watched VS Code disappear rather than watched a window launch. Two windows at
   one address does not read as a cascade wrapping round; it reads as the demo
   losing one.

   The number lives here rather than in the renderer because it is what
   `deskFreeSlot` hands out. site/beckon.css is tuned so the fifth step of the
   cascade still lands inside the desk's work area, and site/beckon.js wraps
   `--slot` on it — change this and re-measure both. */
var DESK_SLOTS = 5;

function deskAppOf(letter) {
  var k = String(letter).toLowerCase();
  for (var i = 0; i < DESK_APPS.length; i++) {
    if (DESK_APPS[i].key.toLowerCase() === k) return DESK_APPS[i];
  }
  return null;
}

/* A desk is:
 *
 *   { os, wins: [ { id, app, min, max, slot } ], focused }
 *
 * `wins` is MRU order, most recent first — the same thing every backend reads
 * out of the compositor (sway's tree walk, Hyprland's focusHistoryID, X11's
 * _NET_CLIENT_LIST_STACKING, CGWindowList on macOS, EnumWindows on Windows).
 * `id` doubles as the window's address: monotonic, assigned at creation, and
 * never reused. Step 5a depends on that — see below.
 *
 * `slot` IS WHERE THE WINDOW SITS ON THE DESK, and it is fixed for the
 * window's lifetime. Nothing in this file ever changes it. That is the whole
 * point: FOCUSING A WINDOW DOES NOT MOVE IT. Raising it changes what is in
 * front, not what is where — which is what every stacking window manager does
 * and what a reader expects to see.
 *
 * The renderer used to derive the position from MRU order instead, so focusing
 * Chrome slid it into the place the terminal had been occupying while the
 * terminal slid out. With two windows of similar size that does not read as
 * "Chrome came forward"; it reads as "the terminal window was renamed to
 * Chrome", which is the opposite of the thing the demo exists to show.
 *
 * `focused` is an id or null. Null is a real state, not an error: it is what
 * step 5c leaves behind.
 *
 * `max` is the window's own maximised state and NOTHING IN THE ALGORITHM READS
 * IT — `deskPress` never tests it and never sets it. It lives here rather than
 * on the DOM node for one reason: the renderer pools nodes by id but rebuilds
 * them whenever the scene or the OS changes, so a fact that has to survive a
 * press has to survive in the model. Drag offsets and drag sizes do NOT have to
 * survive one, which is exactly why they are not here — they stay CSS custom
 * properties on the node, and this file keeps its promise to know no geometry.
 */
function deskMake(os, spec) {
  var wins = spec.map(function (w, i) {
    return { id: i + 1, app: w.app, min: !!w.min, max: false, slot: i };
  });
  var first = wins.filter(function (w) { return !w.min; })[0];
  return {
    os: os,
    wins: wins,
    focused: first ? first.id : null,
    next: wins.length + 1,
    nextSlot: wins.length
  };
}

function deskClone(d) {
  return {
    os: d.os,
    wins: d.wins.map(function (w) {
      return { id: w.id, app: w.app, min: w.min, max: w.max, slot: w.slot };
    }),
    focused: d.focused,
    next: d.next,
    nextSlot: d.nextSlot
  };
}

function deskWin(d, id) {
  for (var i = 0; i < d.wins.length; i++) if (d.wins[i].id === id) return d.wins[i];
  return null;
}

/* Where a launched window goes: THE LOWEST FREE PLACE, not the next one nobody
   has ever used.
   `d.nextSlot++` was the latter, and it drifted off the end of the ring: close
   Chrome and press C again and the new window took place 5, which the renderer
   wraps to place 0 — a window that was still open and still visible. A freed
   place is free, so a relaunch takes its old spot back and the desk keeps one
   window per place.
   `nextSlot` stays as the fallback for the one case a ring of five cannot
   serve — more than five windows at once, which only the Cycle scene's two
   Chromes can reach. There a wrap is honest: it is what a real cascade does
   when it runs out of desk. */
function deskFreeSlot(d) {
  var used = {};
  d.wins.forEach(function (w) { used[w.slot % DESK_SLOTS] = true; });
  for (var i = 0; i < DESK_SLOTS; i++) if (!used[i]) return i;
  return d.nextSlot % DESK_SLOTS;
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

  /* Step 4 — nothing of this app is running, so launch it. A new window takes
     the next free place on the desk; it does not take someone else's.
     THIS IS THE ONLY BRANCH THAT RETURNS `born`, and that is the whole reason
     it exists: it is the only one that puts something on the desk that was not
     there a moment ago, so it is the only one the renderer should announce.
     Every other branch rearranges windows the reader can already see. The
     caller reads `r.born` and nothing else has to know what a launch is —
     `undefined` everywhere else is the answer "nothing was born", which is
     both true and falsy. */
  if (mine.length === 0) {
    var born = { id: d.next++, app: app.name, min: false, max: false, slot: deskFreeSlot(d) };
    d.nextSlot++;
    d.wins = [born].concat(d.wins);
    d.focused = born.id;
    return { desk: d, step: '4', app: app, born: born.id };
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

/* --- what the MOUSE does ---------------------------------------------------
 *
 * NONE OF THIS IS BECKON, and that is the point of saying so here. beckon
 * focuses and launches; it never minimises, never maximises, never closes and
 * never moves a window — CLAUDE.md's *Out of scope* is explicit about the last
 * one. These four exist because the demo is a picture of the reader's own
 * desktop, and a desktop whose title-bar buttons do nothing is a screenshot.
 *
 * They live in this file rather than in the renderer for the same reason
 * `deskPress` does: they are decisions about the model, they are pure, and
 * site/desk.test.mjs walks them. The renderer owns pixels and nothing else.
 *
 * Each one takes the desk it was given and returns a new one, so a caller can
 * never half-apply a change.
 */

/* Click a window and it comes forward. The same thing the key does — which is
   why this is `deskRaise` and not a second implementation of it. */
function deskFocus(desk, id) {
  var d = deskClone(desk);
  return deskWin(d, id) ? deskRaise(d, id) : d;
}

/* Focus does NOT go to null here the way it does in step 5c: minimising the
   front window on a real desktop hands focus to whatever was behind it, and
   only an empty desk leaves nothing focused. The minimised window keeps its
   place in the MRU list, so the next press finds it exactly where the algorithm
   expects — un-minimising is already part of `deskRaise`. */
function deskMinimize(desk, id) {
  var d = deskClone(desk);
  var w = deskWin(d, id);
  if (!w || w.min) return d;
  w.min = true;
  if (d.focused === id) {
    var next = d.wins.filter(function (x) { return !x.min; })[0];
    d.focused = next ? next.id : null;
  }
  return d;
}

/* Closing is the one gesture that can empty the desk, and that is useful rather
   than a hazard: the dock stays, the key still works, and the next press is
   step 4 — a launch — which is the branch a demo can otherwise only reach by
   being told about it. */
function deskClose(desk, id) {
  var d = deskClone(desk);
  if (!deskWin(d, id)) return d;
  d.wins = d.wins.filter(function (x) { return x.id !== id; });
  if (d.focused === id) {
    var next = d.wins.filter(function (x) { return !x.min; })[0];
    d.focused = next ? next.id : null;
  }
  return d;
}

/* Maximising raises, on every window manager there is. */
function deskToggleMax(desk, id) {
  var d = deskClone(desk);
  var w = deskWin(d, id);
  if (!w) return d;
  w.max = !w.max;
  return deskRaise(d, id);
}

/* The name each branch goes by, and THESE REPLACED THE STEP NUMBERS ON THE PAGE.
 *
 * The table in #how used to print `4`, `5`, `5a`, `5b`, `5c` — CLAUDE.md's own
 * numbering, carried over as a citation. It cited nothing a reader could reach:
 * they do not have that document open, and the numbering starts at 4 because
 * steps 1-3 are read-the-id, resolve-the-name and scan-the-windows, none of
 * which is visible from outside. So the column announced three missing steps
 * and explained none of them, and `5b` said nothing at all about what happens.
 *
 * A name does. The keys of this map stay the step numbers because that is what
 * `deskPress` returns and what `data-step` in the markup uses to set a scene —
 * they are an internal id now, not something the page shows.
 */
var DESK_STEP_NAMES = {
  '4':  'Launch',
  '5':  'Focus',
  '5a': 'Cycle',
  '5b': 'Back',
  '5c': 'Hide'
};

function deskStepName(step) {
  return DESK_STEP_NAMES[step] || '';
}

/* What the readout says. One sentence per step, written so it is true on a
   tiling compositor as well as a stacking one — hence "takes focus" rather than
   "comes to the front". The desks draw stacking desktops, but beckon's Linux
   support is mostly tiling compositors and the sentence has to hold there. */
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

/* The transcript for a mouse gesture. Every one of these sentences ends by
   pointing back at the keyboard, because the demo is not selling a window
   manager: the mouse is here to make the desk feel like a desk, and the claim
   being made is that you never need it. `move` and `size` say outright that
   beckon does not do what the reader just did, which is the honest reading of
   a demo that lets them do it. */
function deskSayWindow(kind, name) {
  switch (kind) {
    case 'focus':
      return name + ' took focus because you clicked it. One key does the same thing, ' +
             'with your hand where it already was.';
    case 'min':
      return name + ' is minimised — still running, still lit in the dock. Press its key and ' +
             'it comes straight back.';
    case 'max':
      return name + ' is maximised. Double-click the title bar to put it back — its key does ' +
             'the same thing either way.';
    case 'unmax':
      return name + ' is a window again. Drag the title bar to move it, or pull the ' +
             'bottom-right corner to resize it; beckon leaves both to you.';
    case 'close':
      return name + ' is closed, and its key still works: the next press launches it. That is ' +
             'step 4.';
    case 'move':
      return 'You moved ' + name + '. beckon never does — it focuses and launches, and leaves ' +
             'every window exactly where you put it.';
    case 'size':
      return 'You resized ' + name + '. beckon never does that either. The desk is yours to ' +
             'arrange; beckon only decides which window has focus.';
    default:
      return '';
  }
}

/* The starting desks. Both live here rather than in the renderer because they
   are the preconditions the page makes claims about, and the tests assert on
   them directly. */
var DESK_SCENES = {
  /* Hero: three apps up, VS Code in front, Files and Spotify not running yet.
     So C focuses Chrome (step 5), T focuses the terminal (5), F and S launch
     (4) — the obvious outcome for four of the five letters, and the fifth, V,
     is the interesting one the section below is about.
     VS CODE IS IN FRONT AND CHROME IS BEHIND IT, not the other way round: the
     chord rows print `C` until a press rewrites them, so the letter on screen
     has to be one whose outcome is worth watching. Pressing the app already in
     front is step 5b, which is a fine thing to demonstrate but a strange thing
     to open with. */
  hero: [{ app: 'VS Code' }, { app: 'Chrome' }, { app: 'Terminal' }],

  /* One per row of the algorithm table in #how. Pressing C in each of these is
     what makes that row's step fire; every other letter still runs the real
     algorithm and lights whichever row it lands on. */
  '4':  [{ app: 'Terminal' }],
  '5':  [{ app: 'Terminal' }, { app: 'Chrome' }],
  '5a': [{ app: 'Chrome' }, { app: 'Chrome' }, { app: 'Terminal' }],
  '5b': [{ app: 'Chrome' }, { app: 'Terminal' }],
  '5c': [{ app: 'Chrome' }]
};

if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    DESK_APPS: DESK_APPS,
    DESK_SCENES: DESK_SCENES,
    DESK_SLOTS: DESK_SLOTS,
    deskAppOf: deskAppOf,
    deskMake: deskMake,
    deskPress: deskPress,
    deskSay: deskSay,
    deskStepName: deskStepName,
    deskFocus: deskFocus,
    deskMinimize: deskMinimize,
    deskClose: deskClose,
    deskToggleMax: deskToggleMax,
    deskSayWindow: deskSayWindow
  };
}
