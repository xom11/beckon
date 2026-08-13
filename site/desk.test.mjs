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
const { DESK_APPS, DESK_SCENES, deskMake, deskPress, deskAppOf, deskSay } =
  require('./desk.js');

const scene = (name, os = 'macos') => deskMake(os, DESK_SCENES[name]);
const front = d => (d.focused === null ? null : d.wins.find(w => w.id === d.focused));

test('the letter map is exactly README.md’s table', () => {
  assert.deepEqual(
    DESK_APPS.map(a => [a.key, a.name]),
    [['T', 'terminal'], ['C', 'Claude'], ['B', 'Brave'], ['E', 'Cursor'], ['D', 'Discord']]
  );
});

test('letters resolve case-insensitively, like every beckon resolver', () => {
  assert.equal(deskAppOf('c').name, 'Claude');
  assert.equal(deskAppOf('C').name, 'Claude');
  assert.equal(deskAppOf('t').name, 'terminal');
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
  assert.equal(front(r.desk).app, 'Claude');
  assert.equal(r.desk.wins.filter(w => w.app === 'Claude').length, 1);
});

test('step 5 — running but not focused, so focus it', () => {
  const r = deskPress(scene('5'), 'C');
  assert.equal(r.step, '5');
  assert.equal(front(r.desk).app, 'Claude');
});

test('step 5a — focused with another window, so walk the ring', () => {
  const r = deskPress(scene('5a'), 'C');
  assert.equal(r.step, '5a');
  assert.equal(front(r.desk).app, 'Claude');
});

test('step 5b — focused, one window, something else open, so go back', () => {
  const r = deskPress(scene('5b'), 'C');
  assert.equal(r.step, '5b');
  assert.equal(front(r.desk).app, 'Brave');
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
     its MRU index, so focusing Claude slid it into the spot Brave had been in
     while Brave slid out of it. Two similar windows swapping places does not
     read as "Claude came forward" — it reads as the Brave window having been
     renamed, which is the opposite of what the demo is for. */
  const before = scene('5');                       // Brave in front, Claude behind
  const claude = before.wins.find(w => w.app === 'Claude');
  const brave = before.wins.find(w => w.app === 'Brave');

  const after = deskPress(before, 'C').desk;
  const claude2 = after.wins.find(w => w.id === claude.id);
  const brave2 = after.wins.find(w => w.id === brave.id);

  assert.equal(after.focused, claude.id, 'Claude took focus');
  assert.equal(claude2.slot, claude.slot, 'and did not move');
  assert.equal(brave2.slot, brave.slot, 'and Brave did not move either');
});

test('no press of any letter ever changes a slot', () => {
  let d = scene('hero');
  const before = d.wins.map(w => [w.id, w.slot]);
  for (const k of ['C', 'B', 'C', 'T', 'B', 'C', 'C']) d = deskPress(d, k).desk;
  const after = d.wins.filter(w => before.some(b => b[0] === w.id)).map(w => [w.id, w.slot]);
  assert.deepEqual(after.sort(), before.sort());
});

test('a launched window takes a new place, not somebody else’s', () => {
  const before = scene('hero');
  const taken = before.wins.map(w => w.slot);
  const after = deskPress(before, 'E').desk;      // step 4, Cursor is not running
  const born = after.wins.find(w => w.app === 'Cursor');
  assert.ok(born, 'Cursor launched');
  assert.ok(!taken.includes(born.slot), `slot ${born.slot} was already occupied`);
});

/* --- the two properties the ring exists to have --------------------------- */

test('the ring visits every window exactly once per lap, and wraps', () => {
  /* Three windows of one app. This is the case a recency-ordered "focus the
     least-recent other window" gets wrong: it 2-cycles between the newest pair
     and window 3 is never reachable. Ids are addresses, so the lap is
     1 -> 2 -> 3 -> 1, which is what sway measured. */
  let d = deskMake('linux', [{ app: 'Claude' }, { app: 'Claude' }, { app: 'Claude' }]);
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
  assert.equal(deskPress(h, 'C').step, '5', 'Claude is running, behind Brave');
  assert.equal(deskPress(h, 'T').step, '5', 'the terminal is running too');
  assert.equal(deskPress(h, 'E').step, '4', 'Cursor is not running');
  assert.equal(deskPress(h, 'D').step, '4', 'Discord is not running');
  assert.equal(deskPress(h, 'B').step, '5b', 'Brave is already in front');
});

test('one press moves all three desks to the same app', () => {
  const out = ['macos', 'windows', 'linux'].map(os => {
    const r = deskPress(deskMake(os, DESK_SCENES.hero), 'C');
    return [r.step, front(r.desk).app];
  });
  assert.deepEqual(out, [['5', 'Claude'], ['5', 'Claude'], ['5', 'Claude']],
    'the hero’s whole argument: same letter, three machines, same outcome');
});

/* --- pressing is pure ----------------------------------------------------- */

test('press never mutates the desk it was given', () => {
  const before = scene('5');
  const snapshot = JSON.stringify(before);
  deskPress(before, 'C');
  deskPress(before, 'E');
  deskPress(before, 'B');
  assert.equal(JSON.stringify(before), snapshot);
});

test('every step that fires has a sentence to show for it', () => {
  for (const name of ['4', '5', '5a', '5b', '5c']) {
    const r = deskPress(scene(name), 'C');
    assert.equal(r.step, name);
    assert.match(deskSay(r), /\S/, `step ${name} has no readout text`);
  }
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
