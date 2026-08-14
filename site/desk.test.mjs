/* The landing page's claim about beckon's algorithm, checked by a machine.
 *
 * Every assertion here mirrors one row of the table in #how, which mirrors one
 * branch of CLAUDE.md's *Focus algorithm*. If a row of that table is ever
 * reworded into something the code does not do, this fails.
 *
 * Run by tools/check-site.sh:  node --test site/desk.test.mjs
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const {
  DESK_APPS, DESK_SCENES, DESK_SCENE_KEY, DESK_SLOTS,
  deskMake, deskPress, deskAppOf, deskSceneKey, deskSay, deskStepName,
  deskFocus, deskMinimize, deskClose, deskToggleMax, deskSayWindow
} = require('./desk.js');

const scene = (name, os = 'macos') => deskMake(os, DESK_SCENES[name]);
const front = d => (d.focused === null ? null : d.wins.find(w => w.id === d.focused));

test('the letter map is exactly README.md’s table', () => {
  assert.deepEqual(
    DESK_APPS.map(a => [a.key, a.name]),
    [['T', 'Terminal'], ['C', 'Chrome'], ['V', 'VS Code'], ['F', 'Files'], ['S', 'Spotify']]
  );
});

test('letters resolve case-insensitively, like every beckon resolver', () => {
  assert.equal(deskAppOf('c').name, 'Chrome');
  assert.equal(deskAppOf('C').name, 'Chrome');
  assert.equal(deskAppOf('t').name, 'Terminal');
});

test('every binding is a plain letter', () => {
  /* The terminal moved off Space, which is what let the key routing drop its
     "defer to a focused control" case: Space activates a button, a link and a
     <summary>, and letters do not. If a binding ever goes back to a named key,
     that case has to come back with it. */
  for (const a of DESK_APPS) {
    assert.match(a.key, /^[A-Za-z]$/, `${a.key} is not a single letter`);
  }
});

test('an unbound letter fires no step and says nothing', () => {
  const r = deskPress(scene('hero'), 'Q');
  assert.equal(r.step, null);
  assert.equal(deskSay(r), '');
});

test('an unbound letter leaves the desk untouched', () => {
  const before = scene('hero');
  const r = deskPress(before, 'Q');
  assert.equal(r.desk, before, 'the same object, not a copy');
});

/* --- the five rows of the table in #how ---------------------------------- */

test('step 4 — app is not running, so launch it', () => {
  const r = deskPress(scene('4'), 'C');
  assert.equal(r.step, '4');
  assert.equal(front(r.desk).app, 'Chrome');
  assert.equal(r.desk.wins.filter(w => w.app === 'Chrome').length, 1);
});

test('step 5 — running but not focused, so focus it', () => {
  const r = deskPress(scene('5'), 'C');
  assert.equal(r.step, '5');
  assert.equal(front(r.desk).app, 'Chrome');
});

test('step 5a — focused with another window, so walk the ring', () => {
  const r = deskPress(scene('5a'), 'C');
  assert.equal(r.step, '5a');
  assert.equal(front(r.desk).app, 'Chrome');
});

test('step 5b — focused, one window, something else open, so go back', () => {
  const r = deskPress(scene('5b'), 'C');
  assert.equal(r.step, '5b');
  assert.equal(front(r.desk).app, 'Terminal');
});

test('step 5c — focused, one window, nothing else, so hide it', () => {
  const r = deskPress(scene('5c'), 'C');
  assert.equal(r.step, '5c');
  assert.equal(r.desk.focused, null);
  assert.equal(r.desk.wins[0].min, true);
});

/* --- focusing raises a window, it does not move it ------------------------ */

test('a focused window keeps the place it already had', () => {
  /* The defect this pins: the renderer used to take a window's position from
     its MRU index, so focusing Chrome slid it into the spot the terminal had
     been in while the terminal slid out of it. Two similar windows swapping
     places does not read as "Chrome came forward" — it reads as the terminal
     window having been renamed, which is the opposite of what the demo is for. */
  const before = scene('5');                    // Terminal in front, Chrome behind
  const chrome = before.wins.find(w => w.app === 'Chrome');
  const term = before.wins.find(w => w.app === 'Terminal');

  const after = deskPress(before, 'C').desk;
  const chrome2 = after.wins.find(w => w.id === chrome.id);
  const term2 = after.wins.find(w => w.id === term.id);

  assert.equal(after.focused, chrome.id, 'Chrome took focus');
  assert.equal(chrome2.slot, chrome.slot, 'and did not move');
  assert.equal(term2.slot, term.slot, 'and the terminal did not move either');
});

test('no press of any letter ever changes a slot', () => {
  let d = scene('hero');
  const before = d.wins.map(w => [w.id, w.slot]);
  for (const k of ['C', 'V', 'C', 'T', 'V', 'C', 'C']) d = deskPress(d, k).desk;
  const after = d.wins.filter(w => before.some(b => b[0] === w.id)).map(w => [w.id, w.slot]);
  assert.deepEqual(after.sort(), before.sort());
});

test('a launched window takes a new place, not somebody else’s', () => {
  const before = scene('hero');
  const taken = before.wins.map(w => w.slot);
  const after = deskPress(before, 'F').desk;      // step 4, Files is not running
  const born = after.wins.find(w => w.app === 'Files');
  assert.ok(born, 'Files launched');
  assert.ok(!taken.includes(born.slot), `slot ${born.slot} was already occupied`);
});

/* --- the places on the desk ------------------------------------------------
 *
 * These three exist because the renderer draws `slot % DESK_SLOTS`, so a slot
 * number the model is happy with is only worth anything if it survives that
 * modulo. It did not: the model counted launches without limit and the page
 * wrapped at four, which put the fifth window exactly on top of the first —
 * same left, same top, same size — while the test above went on passing,
 * because it asserted on `w.slot` and the defect was in what got drawn.
 */

test('one place per letter, so five launches never share one', () => {
  assert.equal(DESK_SLOTS, DESK_APPS.length,
    'a letter with no place of its own launches on top of another window');
});

test('pressing all five letters fills five distinct places', () => {
  let d = deskMake('macos', DESK_SCENES.hero);
  for (const k of ['T', 'C', 'V', 'F', 'S']) d = deskPress(d, k).desk;
  assert.equal(d.wins.length, 5);
  const drawn = d.wins.map(w => w.slot % DESK_SLOTS);
  assert.equal(new Set(drawn).size, 5, `two windows drawn at one place: ${drawn}`);
});

test('a relaunch takes back the place it freed, rather than drifting off the ring', () => {
  /* Close Chrome and press C again. `nextSlot++` handed out place 5, which the
     renderer draws at place 0 — where VS Code was still sitting. */
  let d = deskMake('macos', DESK_SCENES.hero);
  const chrome = d.wins.find(w => w.app === 'Chrome');
  d = deskClose(d, chrome.id);
  d = deskPress(d, 'C').desk;
  const reborn = d.wins.find(w => w.app === 'Chrome');
  assert.equal(reborn.slot, chrome.slot, 'Chrome came back somewhere else');
  const drawn = d.wins.map(w => w.slot % DESK_SLOTS);
  assert.equal(new Set(drawn).size, d.wins.length, `two windows drawn at one place: ${drawn}`);
});

/* --- each scene and the key that answers it -------------------------------- */

test('every row of #how fires its own branch, with its own letter', () => {
  /* The table in #how sets a scene per row and the tour then presses for the
     reader. If a scene and its key ever stop matching, the mark lands on a row
     the desk is not demonstrating — and nothing else would notice, because both
     halves are individually valid. */
  for (const step of ['4', '5', '5a', '5b', '5c']) {
    const r = deskPress(scene(step), deskSceneKey(step));
    assert.equal(r.step, step, `scene ${step} + ${deskSceneKey(step)} fired ${r.step}`);
  }
});

test('the Launch scene is an empty desk, and its key opens the first window', () => {
  /* It used to hold a Terminal while the row beside it read "not running" and
     the line under it named Chrome — a picture with a window in it, captioned
     about an app that was not that window. */
  assert.deepEqual(DESK_SCENES['4'], []);
  const d = scene('4');
  assert.equal(d.wins.length, 0);
  assert.equal(d.focused, null);

  const r = deskPress(d, deskSceneKey('4'));
  assert.equal(r.step, '4');
  assert.equal(r.desk.wins.length, 1);
  assert.equal(front(r.desk).app, 'Terminal', 'T on an empty desk opens the terminal');
  assert.ok(r.born, 'and the renderer is told to animate it open');
});

test('every scene key is a real letter, and an unknown step still answers', () => {
  for (const [step, key] of Object.entries(DESK_SCENE_KEY)) {
    assert.ok(deskAppOf(key), `scene ${step} is keyed to ${key}, which is bound to nothing`);
  }
  assert.ok(deskAppOf(deskSceneKey('nope')), 'the fallback letter is unbound');
});

/* --- what the renderer is allowed to animate ------------------------------- */

test('only a launch reports a window as born', () => {
  /* The page zooms a window open on step 4 and on nothing else, because step 4
     is the only branch that puts something on the desk that was not there. If
     any other branch started reporting a `born`, a raise would announce itself
     as a launch — and telling those two apart is the whole point of the demo. */
  const launch = deskPress(scene('4'), 'C');            // Chrome is not running
  assert.equal(launch.step, '4');
  assert.equal(launch.born, launch.desk.wins.find(w => w.app === 'Chrome').id);

  for (const [name, d, key] of [
    ['5',  scene('5'),  'C'],
    ['5a', scene('5a'), 'C'],
    ['5b', scene('5b'), 'C'],
    ['5c', scene('5c'), 'C'],
  ]) {
    const r = deskPress(d, key);
    assert.equal(r.step, name, `scene ${name} did not fire its own branch`);
    assert.ok(!r.born, `step ${name} reported a window as born`);
  }
});

test('an unbound letter reports nothing at all, born included', () => {
  const r = deskPress(scene('hero'), 'Q');
  assert.equal(r.step, null);
  assert.ok(!r.born);
});

/* --- the two properties the ring exists to have --------------------------- */

test('the ring visits every window exactly once per lap, and wraps', () => {
  /* Three windows of one app. This is the case a recency-ordered "focus the
     least-recent other window" gets wrong: it 2-cycles between the newest pair
     and window 3 is never reachable. Ids are addresses, so the lap is
     1 -> 2 -> 3 -> 1, which is what sway measured. */
  let d = deskMake('linux', [{ app: 'Chrome' }, { app: 'Chrome' }, { app: 'Chrome' }]);
  const seen = [];
  for (let i = 0; i < 4; i++) {
    const r = deskPress(d, 'C');
    assert.equal(r.step, '5a');
    d = r.desk;
    seen.push(d.focused);
  }
  assert.deepEqual(seen, [2, 3, 1, 2], 'one lap, then round again');
});

test('with several windows open the ring never exits 5a', () => {
  /* The reason #how needs the table rows as a reset control at all: no number
     of presses gets a three-window app to 5b or 5c. */
  let d = scene('5a');
  for (let i = 0; i < 6; i++) {
    const r = deskPress(d, 'C');
    assert.equal(r.step, '5a');
    d = r.desk;
  }
});

/* --- the hero's start state ---------------------------------------------- */

test('four of the five letters do the obvious thing in the hero', () => {
  const h = scene('hero');
  assert.equal(deskPress(h, 'C').step, '5', 'Chrome is running, behind VS Code');
  assert.equal(deskPress(h, 'T').step, '5', 'the terminal is running too');
  assert.equal(deskPress(h, 'F').step, '4', 'Files is not running');
  assert.equal(deskPress(h, 'S').step, '4', 'Spotify is not running');
  assert.equal(deskPress(h, 'V').step, '5b', 'VS Code is already in front');
});

test('one press moves all three desks to the same app', () => {
  const out = ['macos', 'windows', 'linux'].map(os => {
    const r = deskPress(deskMake(os, DESK_SCENES.hero), 'C');
    return [r.step, front(r.desk).app];
  });
  assert.deepEqual(out, [['5', 'Chrome'], ['5', 'Chrome'], ['5', 'Chrome']],
    'the hero’s whole argument: same letter, three machines, same outcome');
});

/* --- pressing is pure ----------------------------------------------------- */

test('press never mutates the desk it was given', () => {
  const before = scene('5');
  const snapshot = JSON.stringify(before);
  deskPress(before, 'C');
  deskPress(before, 'F');
  deskPress(before, 'V');
  assert.equal(JSON.stringify(before), snapshot);
});

test('every step that fires has a sentence to show for it', () => {
  for (const name of ['4', '5', '5a', '5b', '5c']) {
    const r = deskPress(scene(name), 'C');
    assert.equal(r.step, name);
    assert.match(deskSay(r), /\S/, `step ${name} has no readout text`);
  }
});

/* --- what the mouse does, and what it must not disturb -------------------- */

test('the title-bar buttons never mutate the desk they were given', () => {
  /* Same guarantee as `press never mutates`, and it matters more here: the
     renderer calls these from a pointer handler that still holds the previous
     desk while it draws the next one. */
  const before = scene('hero');
  const snapshot = JSON.stringify(before);
  const id = before.wins[1].id;
  deskFocus(before, id);
  deskMinimize(before, id);
  deskToggleMax(before, id);
  deskClose(before, id);
  assert.equal(JSON.stringify(before), snapshot);
});

test('minimising the front window hands focus to the one behind it', () => {
  const d = scene('hero');                     // VS Code in front, then Chrome
  const after = deskMinimize(d, d.focused);
  assert.equal(front(after).app, 'Chrome', 'not left with nothing focused');
  assert.ok(after.wins.find(w => w.app === 'VS Code').min, 'VS Code is minimised');
});

test('minimising the last window leaves nothing focused, like step 5c', () => {
  const d = scene('5c');                          // one Chrome window, alone
  const after = deskMinimize(d, d.focused);
  assert.equal(after.focused, null);
});

test('a minimised window is still running, so its key brings it back', () => {
  /* The whole difference between minimising and closing, and the reason the
     dock stays lit for one and goes dark for the other. */
  const d = deskMinimize(scene('5b'), scene('5b').focused);   // Chrome minimised
  const r = deskPress(d, 'C');
  assert.equal(r.step, '5', 'found it running rather than launching a second copy');
  assert.equal(front(r.desk).app, 'Chrome');
  assert.equal(front(r.desk).min, false, 'raising un-minimises, as every backend does');
});

test('closing the last window of an app makes the next press a launch', () => {
  const d = scene('5b');                                     // Chrome + Terminal
  const claude = d.wins.find(w => w.app === 'Chrome');
  const after = deskClose(d, claude.id);
  assert.equal(after.wins.some(w => w.app === 'Chrome'), false);
  assert.equal(deskPress(after, 'C').step, '4', 'step 4 — it is not running now');
});

test('closing every window leaves an empty desk that still answers a key', () => {
  let d = scene('hero');
  while (d.wins.length) d = deskClose(d, d.wins[0].id);
  assert.equal(d.focused, null);
  const r = deskPress(d, 'C');
  assert.equal(r.step, '4');
  assert.equal(front(r.desk).app, 'Chrome');
});

test('maximising raises the window and toggles back', () => {
  const d = scene('5b');
  const claude = d.wins.find(w => w.app === 'Chrome');
  const up = deskToggleMax(d, claude.id);
  assert.equal(up.focused, claude.id, 'maximising raises, like every window manager');
  assert.equal(up.wins.find(w => w.id === claude.id).max, true);
  assert.equal(deskToggleMax(up, claude.id).wins.find(w => w.id === claude.id).max, false);
});

test('the algorithm never reads or writes `max`', () => {
  /* `max` is the renderer's business. If a press ever starts un-maximising
     things, the picture stops matching what beckon actually does — beckon does
     not touch window geometry at all. */
  let d = scene('5a');
  d = deskToggleMax(d, d.wins[0].id);
  const before = d.wins.map(w => [w.id, w.max]);
  for (const k of ['C', 'C', 'V', 'T', 'C']) d = deskPress(d, k).desk;
  const after = new Map(d.wins.map(w => [w.id, w.max]));
  for (const [id, max] of before) {
    if (after.has(id)) assert.equal(after.get(id), max, `window ${id} had its max flag rewritten`);
  }
});

test('a launched window is never born maximised', () => {
  const r = deskPress(scene('4'), 'F');
  assert.equal(r.desk.wins.find(w => w.app === 'Files').max, false);
});

test('every mouse gesture has a sentence, and each names beckon or a key', () => {
  /* The mouse is on the desk to make it feel like a desk; every one of these
     sentences has to point back at the thing the page is actually selling. */
  for (const kind of ['focus', 'min', 'max', 'unmax', 'close', 'move', 'size']) {
    const said = deskSayWindow(kind, 'Chrome');
    assert.match(said, /\S/, `${kind} has no sentence`);
    assert.match(said, /beckon|key|press/i, `${kind} never gets back to the point: ${said}`);
  }
  assert.equal(deskSayWindow('nonsense', 'Chrome'), '');
});

/* --- the five branches have names, not numbers ---------------------------- */

test('every branch has a name, and no two share one', () => {
  /* The page shows these instead of 4 / 5 / 5a / 5b / 5c. Two branches sharing
     a name would make the readout point at the wrong row of the table. */
  const names = ['4', '5', '5a', '5b', '5c'].map(deskStepName);
  for (const [i, n] of names.entries()) {
    assert.match(n, /^[A-Z][a-z]+$/, `branch ${i} has no usable name: ${n}`);
  }
  assert.equal(new Set(names).size, names.length, `names collide: ${names}`);
});

test('the name is one word, because it is a label and not a sentence', () => {
  /* `deskSay` is where the explanation goes; this is what the readout prints
     in its heading slot and what the table prints in its second column. */
  for (const step of ['4', '5', '5a', '5b', '5c']) {
    assert.doesNotMatch(deskStepName(step), /\s/, `${step}: ${deskStepName(step)}`);
  }
});

test('an unknown step names nothing rather than inventing a label', () => {
  assert.equal(deskStepName('7'), '');
  assert.equal(deskStepName(null), '');
});

test('no readout says "front", which is meaningless on a tiling compositor', () => {
  /* sway does not stack windows, so "comes to the front" is not a thing that
     happens there. The old page's hero transcript said exactly that about all
     three machines at once. Every sentence here says "focus" instead. */
  for (const name of ['4', '5', '5a', '5b', '5c']) {
    const said = deskSay(deskPress(scene(name), 'C'));
    assert.doesNotMatch(said, /\bfront\b/i, `step ${name}: ${said}`);
  }
});
