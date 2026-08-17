#!/usr/bin/env node
/*
 * Record #how — "One key, five answers." — from site/ into an animated GIF for
 * README.md.
 *
 * The section is not a video and never was: it is DOM built by desk.js and
 * choreographed by a setTimeout loop in beckon.js. So the only way to get a
 * clip is to run the real page and photograph it, which is what this does.
 *
 * WHY VIRTUAL TIME AND NOT A REAL-TIME LOOP. `Page.captureScreenshot` of a
 * 2240px-wide clip costs tens of milliseconds and the cost is not constant, so
 * a wall-clock capture loop cannot hold a frame interval: every frame lands
 * late by a different amount and the GIF's constant frame delay then lies
 * about when each one happened. `Emulation.setVirtualTimePolicy` freezes the
 * page's clock between frames, so the shutter costs the page nothing —
 * timers, CSS transitions and the loop's own beats all advance by exactly the
 * interval asked for. The frames are evenly spaced by construction rather than
 * by hoping.
 *
 * WHY THE LOOP IS PHASE-LOCKED BEFORE THE FIRST FRAME. beckon.js drives
 * STEPS = ['4','5','5a','5b','5c'] with per-branch turns of 4500/4750/5250/
 * 5250/5500 ms — 25,250 ms in total. A GIF that starts anywhere else in that
 * cycle still loops, but it opens mid-sentence. `cue(step)` calls
 * `mark('is-on', step)`, so the rising edge of `#how-table tr.is-on` reading
 * `data-step="4"` is the top of the cycle, and that is where recording starts.
 *
 * That edge also skips the first pass for free, which is wanted rather than
 * tolerated: the tour opens at `at=0, phase='press'` on a scene `onOs` has
 * already built, so the first turn fires a press with no seam and no cue
 * before it. `is-on` is never '4' on that pass, so waiting for the edge lands
 * on the second, steady-state cycle.
 *
 * WHY 1120px. `.how-demo` carries
 * `margin-right: calc(-1 * max(0px, min(176px, (100vw - var(--maxw)) / 2)))`,
 * i.e. past --maxw (1120px) the desk deliberately bleeds out of the wrap. At
 * exactly 1120 that term is 0, so the grid is a plain rectangle and the clip
 * is the union of the two columns with nothing hanging outside it.
 *
 * WHY prefers-reduced-motion IS FORCED. This machine has Reduce Motion ON and
 * headless Chrome inherits it. beckon.css §8 turns every beat that moves or
 * resizes into a cut under `reduce` — so recording without the override
 * produces a clip that is technically correct and shows none of the motion the
 * section exists to show.
 *
 * Regenerate whenever #how changes shape — the clip is a photograph of the
 * page and goes stale silently:
 *
 *   node tools/record-five-answers.mjs
 *
 * It lands in assets/ beside beckon.ico rather than in docs/, which holds
 * internal specs and measurements this repository deliberately does not
 * publish.
 */

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { extname, join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');
const SITE = join(ROOT, 'site');

/* The loop's own arithmetic, from beckon.js. Kept here as a constant rather
   than scraped out of the page because a wrong value fails loudly (the GIF
   stutters at the seam) instead of quietly recording 24 seconds of a 25 second
   cycle. */
const TURNS = { '4': 4500, '5': 4750, '5a': 5250, '5b': 5250, '5c': 5500 };
const CYCLE_MS = Object.values(TURNS).reduce((a, b) => a + b, 0); /* 25250 */

const args = parseArgs(process.argv.slice(2));
const OUT = resolve(ROOT, args.out ?? 'assets/five-answers.gif');
const VIEW_W = num(args.width, 1120);
const VIEW_H = num(args.height, 1000);
const DSF = num(args.scale, 2);
/* Frame interval in centiseconds: GIF stores delays in hundredths of a second,
   so an interval that is not a whole cs cannot be represented and ffmpeg would
   round it silently. 8cs = 80ms = 12.5fps. */
const DELAY_CS = num(args.delay, 8);
const STEP_MS = DELAY_CS * 10;
const FRAMES = num(args.frames, Math.ceil(CYCLE_MS / STEP_MS));
const OUT_W = num(args.outWidth, 900);
const COLORS = num(args.colors, 128);
const KEEP = !!args.keep;

const CHROME =
  args.chrome ??
  ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
   '/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',
   '/Applications/Chromium.app/Contents/MacOS/Chromium'].find(p => existsSync(p));

if (!CHROME) die('no Chromium-family browser found; pass --chrome <path>');

const MIME = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript',
               '.png': 'image/png', '.svg': 'image/svg+xml', '.ico': 'image/x-icon' };

main().catch(err => die(err.stack ?? String(err)));

async function main() {
  const frameDir = join(tmpdir(), `beckon-five-answers-${process.pid}`);
  await mkdir(frameDir, { recursive: true });
  await mkdir(dirname(OUT), { recursive: true });

  /* Served over http rather than opened as file:// so the page runs under the
     same origin rules it ships under. */
  const { server, port } = await serve(SITE);
  const chrome = await launchChrome(port);

  try {
    const cdp = await connect(chrome.wsUrl);
    await record(cdp, port, frameDir);
    await encode(frameDir, OUT);
  } finally {
    chrome.proc.kill('SIGKILL');
    server.close();
    if (!KEEP) await rm(frameDir, { recursive: true, force: true });
    else console.log(`frames kept in ${frameDir}`);
  }
}

/* ------------------------------------------------------------------ record */

async function record(cdp, port, frameDir) {
  await cdp.send('Page.enable');
  await cdp.send('Runtime.enable');

  await cdp.send('Emulation.setDeviceMetricsOverride', {
    width: VIEW_W, height: VIEW_H, deviceScaleFactor: DSF, mobile: false
  });

  /* Both features are load-bearing. `dark` is the look the clip was chosen to
     have; `no-preference` is what keeps beckon.css §8 from flattening the
     animation into stills on a machine with Reduce Motion on. */
  await cdp.send('Emulation.setEmulatedMedia', {
    features: [
      { name: 'prefers-color-scheme', value: 'dark' },
      { name: 'prefers-reduced-motion', value: 'no-preference' }
    ]
  });

  /* Freeze the clock before anything loads, so no beat of the tour is spent
     while the recorder is still setting up. */
  await cdp.send('Emulation.setVirtualTimePolicy', { policy: 'pause' });
  await cdp.send('Page.navigate', { url: `http://127.0.0.1:${port}/index.html` });
  await budget(cdp, 4000, true);   /* load + fonts + first paint */

  /* Put #how in the middle of the viewport. `block: 'center'` rather than
     'start' keeps the grid clear of anything pinned to the top of the page. */
  /* `scrollBehavior: auto` is set BEFORE the scroll, not after: the page asks
     for smooth scrolling, and a smooth scroll under virtual time lands
     whenever the budget happens to run out — which is how the first pass
     measured its box mid-flight. */
  await evaluate(cdp, `
    document.documentElement.style.scrollBehavior = 'auto';
    document.getElementById('how').scrollIntoView({ block: 'center' });
  `);
  await budget(cdp, 2000);

  /* Phase-lock: advance in fine steps until the Launch row lights up. */
  const lock = await phaseLock(cdp);
  console.log(`phase-locked on step 4 after ${lock}ms of virtual time`);

  /* MEASURED AFTER THE LOCK, NOT BEFORE, and the order is the whole point.
     The readout under the desk is empty until the tour writes its first line,
     so a box measured on arrival is short by the height of that strip — and
     the clip then cuts the last table row and the readout off at the same
     edge, which reads as a layout bug in the page rather than as a mistake in
     the recorder. */
  const clip = await measureClip(cdp);
  console.log(`clip ${clip.width}x${clip.height} at (${clip.x}, ${clip.y}), dsf ${DSF}`);

  console.log(`capturing ${FRAMES} frames at ${STEP_MS}ms (${CYCLE_MS}ms cycle)`);
  for (let i = 0; i < FRAMES; i++) {
    /* `scale: 1`, NOT `DSF`. The device metrics override already renders at
       `deviceScaleFactor`, and clip.scale multiplies on top of it — passing
       the same number twice records at 4x and quadruples every frame for no
       extra detail. */
    const { data } = await cdp.send('Page.captureScreenshot', {
      format: 'png', clip: { ...clip, scale: 1 }, captureBeyondViewport: true
    });
    await writeFile(join(frameDir, `f${String(i).padStart(4, '0')}.png`),
                    Buffer.from(data, 'base64'));
    if (i < FRAMES - 1) await budget(cdp, STEP_MS);
    if ((i + 1) % 50 === 0) console.log(`  ${i + 1}/${FRAMES}`);
  }
}

/* The clip is the union of the grid and the demo. Taking the grid alone is
   wrong at any viewport wider than --maxw, where the desk is deliberately
   outside it — and a union that happens to equal the grid at 1120px costs
   nothing, while a bare `.how-grid` box silently crops the desk if anyone
   records at a different width. */
async function measureClip(cdp) {
  const box = await evaluate(cdp, `(() => {
    /* SCROLL OFFSET ADDED HERE, and it is not optional. Page.captureScreenshot
       takes its clip in DOCUMENT coordinates while getBoundingClientRect
       reports VIEWPORT ones, so the two agree only while scrollY is 0 - and
       this runs after the page has been scrolled to put #how on screen.
       Without the offset the clip is short by exactly the scroll distance and
       lands on the hero section, which photographs perfectly and is the wrong
       part of the page.
       (No backticks in here: this comment lives inside a template literal,
       and one would end the string mid-sentence.) */
    const sx = window.scrollX, sy = window.scrollY;
    const r = el => { const b = el.getBoundingClientRect();
                      return { l: b.left + sx, t: b.top + sy,
                               r: b.right + sx, b: b.bottom + sy }; };
    const grid = r(document.querySelector('.how-grid'));
    const demo = r(document.querySelector('#how-demo'));
    /* The table is pulled left by its own cell padding so its ink lines up
       with the heading above it, which puts a lit row's fill hard against the
       grid's left edge. 22px gives that fill somewhere to end. */
    const pad = 22;
    const l = Math.min(grid.l, demo.l) - pad, t = Math.min(grid.t, demo.t) - pad;
    const rr = Math.max(grid.r, demo.r) + pad, bb = Math.max(grid.b, demo.b) + pad;
    return JSON.stringify({ x: l, y: t, width: rr - l, height: bb - t });
  })()`);
  const b = JSON.parse(box);
  /* Even dimensions: several encoders reject odd ones, and rounding here is
     cheaper than discovering it 300 frames later. */
  return {
    x: Math.round(b.x), y: Math.round(b.y),
    width: Math.round(b.width / 2) * 2, height: Math.round(b.height / 2) * 2
  };
}

/* Advance until `#how-table tr.is-on` reads step 4 having previously read
   something else — the rising edge, not merely the state, so a recorder that
   arrives mid-Launch waits for the next one rather than opening two thirds of
   the way through the branch. */
async function phaseLock(cdp) {
  const PROBE = `(() => {
    const on = document.querySelector('#how-table tr.is-on');
    return on ? on.getAttribute('data-step') : '';
  })()`;
  const GRAIN = 20;
  const LIMIT = CYCLE_MS * 3;
  let waited = 0, sawOther = false;
  while (waited < LIMIT) {
    const step = await evaluate(cdp, PROBE);
    if (step && step !== '4') sawOther = true;
    if (step === '4' && sawOther) return waited;
    await budget(cdp, GRAIN);
    waited += GRAIN;
  }
  die(`never saw the Launch row light up within ${LIMIT}ms of virtual time — ` +
      `the tour did not start (is #how on screen?)`);
}

/* ------------------------------------------------------------------ encode */

async function encode(frameDir, out) {
  /* Two passes. palettegen over EVERY frame (stats_mode=diff weights pixels
     that actually change, which is most of the desk and none of the table),
     then paletteuse with Bayer dithering — ordered dither costs far fewer
     bytes than error-diffusion on a flat UI, because it does not scatter
     fresh noise into regions that were identical between frames and would
     otherwise compress to nothing. */
  const pat = join(frameDir, 'f%04d.png');
  const palette = join(frameDir, 'palette.png');
  const fps = 100 / DELAY_CS;
  const scale = `scale=${OUT_W}:-2:flags=lanczos`;

  await run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y',
    '-framerate', String(fps), '-i', pat,
    '-vf', `${scale},palettegen=max_colors=${COLORS}:stats_mode=diff`,
    palette]);

  await run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y',
    '-framerate', String(fps), '-i', pat, '-i', palette,
    '-lavfi', `${scale}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle`,
    '-loop', '0', out]);

  const { size } = await import('node:fs').then(m => m.promises.stat(out));
  console.log(`\n${out}\n${(size / 1048576).toFixed(2)} MB, ${FRAMES} frames, ` +
              `${fps}fps, ${OUT_W}px wide, ${COLORS} colors`);
}

/* --------------------------------------------------------------- plumbing */

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--')) continue;
    const key = argv[i].slice(2).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    const next = argv[i + 1];
    if (next === undefined || next.startsWith('--')) out[key] = true;
    else { out[key] = next; i++; }
  }
  return out;
}

/* Function declarations, not `const` arrows: the option block at the top of
   the file calls both before this point in source order, which a `const` puts
   in the temporal dead zone. */
function num(v, d) { return v === undefined || v === true ? d : Number(v); }

function die(msg) { console.error(`record-five-answers: ${msg}`); process.exit(1); }

async function serve(dir) {
  const server = createServer(async (req, res) => {
    const path = decodeURIComponent(req.url.split('?')[0]);
    const file = join(dir, path === '/' ? 'index.html' : path);
    if (!file.startsWith(dir)) { res.writeHead(403).end(); return; }
    try {
      const body = await readFile(file);
      res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' });
      res.end(body);
    } catch { res.writeHead(404).end('not found'); }
  });
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  return { server, port: server.address().port };
}

async function launchChrome(sitePort) {
  const userDir = join(tmpdir(), `beckon-chrome-${process.pid}`);
  const proc = spawn(CHROME, [
    '--headless=new', '--remote-debugging-port=0',
    `--user-data-dir=${userDir}`,
    '--no-first-run', '--no-default-browser-check', '--disable-extensions',
    '--hide-scrollbars', '--force-device-scale-factor=1',
    '--disable-lcd-text',           /* subpixel AA turns into colour fringes in a 128-colour palette */
    '--font-render-hinting=none',
    '--enable-begin-frame-control',  /* virtual time drives the compositor, not the wall clock */
    '--disable-gpu', '--disable-dev-shm-usage',
    'about:blank'
  ], { stdio: ['ignore', 'ignore', 'pipe'] });

  /* Chrome prints the DevTools endpoint to stderr once it is listening. */
  const wsUrl = await new Promise((res, rej) => {
    let buf = '';
    const t = setTimeout(() => rej(new Error('chrome did not report a debugging port')), 20000);
    proc.stderr.on('data', d => {
      buf += d;
      const m = buf.match(/ws:\/\/[^\s]+/);
      if (m) { clearTimeout(t); res(m[0]); }
    });
    proc.on('exit', c => { clearTimeout(t); rej(new Error(`chrome exited ${c}`)); });
  });

  /* The browser endpoint cannot drive a page; open a tab and target that. */
  const base = wsUrl.match(/ws:\/\/([^/]+)/)[1];
  const list = await fetch(`http://${base}/json/list`).then(r => r.json());
  let page = list.find(t => t.type === 'page');
  if (!page) {
    await fetch(`http://${base}/json/new?about:blank`, { method: 'PUT' });
    page = (await fetch(`http://${base}/json/list`).then(r => r.json()))
      .find(t => t.type === 'page');
  }
  if (!page) throw new Error('chrome exposed no page target');
  return { proc, wsUrl: page.webSocketDebuggerUrl, sitePort };
}

async function connect(url) {
  const ws = new WebSocket(url);
  await new Promise((res, rej) => {
    ws.addEventListener('open', res, { once: true });
    ws.addEventListener('error', () => rej(new Error(`cannot reach ${url}`)), { once: true });
  });
  let id = 0;
  const pending = new Map();
  const waiters = new Map();
  ws.addEventListener('message', ev => {
    const msg = JSON.parse(ev.data);
    if (msg.id !== undefined) {
      const p = pending.get(msg.id);
      if (!p) return;
      pending.delete(msg.id);
      msg.error ? p.rej(new Error(`${msg.error.message} (${p.method})`)) : p.res(msg.result);
    } else if (waiters.has(msg.method)) {
      const list = waiters.get(msg.method);
      waiters.delete(msg.method);
      list.forEach(fn => fn(msg.params));
    }
  });
  return {
    send(method, params = {}) {
      const mid = ++id;
      return new Promise((res, rej) => {
        pending.set(mid, { res, rej, method });
        ws.send(JSON.stringify({ id: mid, method, params }));
      });
    },
    once(method) {
      return new Promise(res => {
        if (!waiters.has(method)) waiters.set(method, []);
        waiters.get(method).push(res);
      });
    }
  };
}

/* Grant the page `ms` of clock and wait for it to be spent. The listener is
   armed BEFORE the grant: the budget for a small step can expire before the
   reply to setVirtualTimePolicy is even read, and a listener attached after
   that would wait for the next expiry that never comes. */
async function budget(cdp, ms, waitForNavigation = false) {
  const expired = cdp.once('Emulation.virtualTimeBudgetExpired');
  await cdp.send('Emulation.setVirtualTimePolicy', {
    policy: 'pauseIfNetworkFetchesPending', budget: ms,
    maxVirtualTimeTaskStarvationCount: 10000,
    ...(waitForNavigation ? { waitForNavigation: true } : {})
  });
  await expired;
}

async function evaluate(cdp, expression) {
  const r = await cdp.send('Runtime.evaluate', { expression, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.text + ' :: ' + expression);
  return r.result.value;
}

function run(cmd, argv) {
  return new Promise((res, rej) => {
    const p = spawn(cmd, argv, { stdio: ['ignore', 'inherit', 'inherit'] });
    p.on('exit', c => (c === 0 ? res() : rej(new Error(`${cmd} exited ${c}`))));
    p.on('error', rej);
  });
}
