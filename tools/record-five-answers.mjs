#!/usr/bin/env node
/*
 * Record #how — "One key, five answers." — from site/ into an animated GIF for
 * README.md.
 *
 * The section is not a video and never was: it is DOM built by desk.js and
 * choreographed by a setTimeout loop in beckon.js. So the only way to get a
 * clip is to run the real page and photograph it, which is what this does.
 *
 *   node tools/record-five-answers.mjs
 *
 * It lands in assets/ beside beckon.ico rather than in docs/, which holds
 * internal specs and measurements this repository deliberately does not
 * publish. Regenerate whenever #how changes shape — the clip is a photograph
 * of a live page and goes stale in silence.
 *
 * ── VIRTUAL TIME IS REJECTED, AND THIS IS THE MEASUREMENT ──────────────────
 *
 * The obvious way to record deterministically is
 * `Emulation.setVirtualTimePolicy`: freeze the page's clock, advance it by
 * exactly one frame interval, shoot, repeat. Frames come out evenly spaced by
 * construction instead of by hoping, and `Page.captureScreenshot` of a 2216px
 * clip costs tens of milliseconds and not a constant number of them, so the
 * wall-clock alternative cannot hold an interval the GIF's constant frame
 * delay then claims it held.
 *
 * It is still wrong, and it fails in the worst available way. Measured on
 * Chrome 151.0.7922.138, counting DISTINCT frames:
 *
 *     virtual time                       5 unique / 316   (a full 25s cycle)
 *     virtual time + begin-frame-control 2 unique /  40
 *     real time                         26 unique /  60   (a 4.8s window)
 *
 * Changing the DOM forces a layout and paint commit, so the five scene swaps
 * come through. Every CSS transition in this section animates transform and
 * opacity, which live on the COMPOSITOR — and no compositor frame is produced
 * while virtual time sits paused between budgets. The result is a five-state
 * slideshow in which every individual frame is perfectly correct.
 *
 * `--enable-begin-frame-control` is the documented answer and makes it worse
 * unless you also drive `HeadlessExperimental.beginFrame` yourself, which is
 * deprecated in new headless. Hence: real time, and pay for the jitter by
 * recording each frame's true timestamp (below).
 *
 * THE TRAP THAT MAKES THIS WORTH THIS MANY WORDS: sampling one frame per
 * branch and eyeballing it CANNOT tell a smooth clip from the slideshow — the
 * five sampled frames are exactly the five states the broken run produces, and
 * they look right. Only counting distinct frames separates them, which is why
 * `assertMotion` runs on every recording and refuses rather than warns.
 *
 * ── The rest of the decisions ──────────────────────────────────────────────
 *
 * PHASE LOCK. beckon.js drives STEPS = ['4','5','5a','5b','5c'] with turns of
 * 4500/4750/5250/5250/5500 ms — 25,250 ms total. `cue(step)` calls
 * `mark('is-on', step)`, so the rising edge of `#how-table tr.is-on` reading
 * `data-step="4"` is the top of the cycle, and that is where recording starts;
 * anywhere else still loops but opens mid-sentence. That edge also skips the
 * tour's first pass for free, which is wanted: it opens at `at=0,
 * phase='press'` on a scene `onOs` has already built, so no `cue()` runs and
 * `is-on` is never '4' on that pass.
 *
 * WIDTH. `.how-demo` carries
 * `margin-right: calc(-1 * max(0px, min(176px, (100vw - var(--maxw)) / 2)))`,
 * i.e. past --maxw (1120px) the desk deliberately bleeds out of the wrap. At
 * exactly 1120 that term is 0, so the grid is a plain rectangle.
 *
 * RESOLUTION, which is the whole reason this file was revised. The clip is
 * 1108 CSS px wide, captured at deviceScaleFactor 2 (2216px) and shipped at
 * 1800px. GitHub renders a README image at roughly 900 CSS px, so a reader on
 * any HiDPI display needs ~1800 device pixels for it — hence the number.
 * The first version shipped 900px, which is 0.81x of the design size, and the
 * browser then scaled that back up ~2x on screen: a soft source, stretched.
 * It was reported as "mờ" and it was. Do not "save space" here.
 *
 * COLOUR DEPTH IS NOT A FREE KNOB EITHER. 64 colours saves 0.7 MB and turns
 * the accent blue on `Launch`/`Focus` PURPLE — measured, visible at a glance
 * side by side. 128 stays true. Dither is `bayer_scale=5`: at the default 3 a
 * crosshatch is plainly visible across the dark window bodies AND the file is
 * 0.9 MB larger, while `dither=none` saves a further 0.3 MB and puts faint
 * contour bands in the desk's gradient. Sizes at 1800px/128 colours, measured:
 * bayer3 2.7 MB, bayer4 2.0 MB, bayer5 1.8 MB, none 1.5 MB, sierra2_4a 5.1 MB.
 *
 * prefers-reduced-motion IS FORCED to no-preference. This machine has Reduce
 * Motion ON and headless Chrome inherits it; beckon.css section 8 then turns
 * every beat that moves into a cut. The override is asserted from inside the
 * page rather than assumed, because its failure mode is the same silent
 * slideshow described above.
 */

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { extname, join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');
const SITE = join(ROOT, 'site');

/* The loop's own arithmetic, from beckon.js. A constant rather than something
   scraped out of the page: a wrong value fails loudly (the GIF stutters at the
   seam) instead of quietly recording 24 seconds of a 25 second cycle. */
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
/* See RESOLUTION above. 0 ships at the captured 2216px, which is more than
   anything needs; 1800 is 2x what GitHub gives a README image and is where the
   size/sharpness curve sits. Never set this near 900 again. */
const OUT_W = num(args.outWidth, 1800);
const COLORS = num(args.colors, 128);
/* CAPTURE FORMAT IS JPEG, AND IT IS NOT A QUALITY COMPROMISE. Measured on this
   clip: `Page.captureScreenshot` costs 117ms median as png and 51ms as jpeg
   q100, and the png figure caps the recorder at 8.5fps — below the 12.5fps the
   GIF asks for, so frames arrive late and motion goes choppy. Against a png of
   the same settled frame, jpeg q100 measures PSNR y=62.0dB over the whole
   picture and y=60.9dB over the text; ~45dB is the usual visually-lossless
   line. Chrome writes yuvj420p even at q100, so chroma is halved — which is
   why the numbers above are quoted per plane, and why this is still fine here:
   the text is near-white on near-black (luma), and the one broad chroma
   feature is the desk's gradient, which is exactly what subsampling costs
   nothing on. The 128-colour quantisation below is orders of magnitude more
   destructive than any of this. */
const SHOT = { format: 'jpeg', quality: 100, ext: 'jpg' };
/* The slideshow guard. A healthy cycle measures ~200 distinct frames; a broken
   one measures 5, one per scene. Anything in between means some layer stopped
   animating and is worth a human looking. */
const MIN_UNIQUE = num(args.minUnique, 60);
const KEEP = !!args.keep;

const CHROME =
  args.chrome ??
  ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
   '/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',
   '/Applications/Chromium.app/Contents/MacOS/Chromium'].find(p => existsSync(p));

if (!CHROME) die('no Chromium-family browser found; pass --chrome <path>');

const MIME = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript',
               '.png': 'image/png', '.svg': 'image/svg+xml', '.ico': 'image/x-icon' };

const sleep = ms => new Promise(r => setTimeout(r, ms));

main().catch(err => die(err.stack ?? String(err)));

async function main() {
  const frameDir = join(tmpdir(), `beckon-five-answers-${process.pid}`);
  await mkdir(frameDir, { recursive: true });
  await mkdir(dirname(OUT), { recursive: true });

  /* Served over http rather than opened as file:// so the page runs under the
     same origin rules it ships under. */
  const { server, port } = await serve(SITE);
  const chrome = await launchChrome();

  try {
    const cdp = await connect(chrome.wsUrl);
    const shot = await record(cdp, port, frameDir);
    assertMotion(shot);
    await encode(frameDir, shot, OUT);
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

  await cdp.send('Emulation.setEmulatedMedia', {
    features: [
      { name: 'prefers-color-scheme', value: 'dark' },
      { name: 'prefers-reduced-motion', value: 'no-preference' }
    ]
  });

  const loaded = cdp.once('Page.loadEventFired');
  await cdp.send('Page.navigate', { url: `http://127.0.0.1:${port}/index.html` });
  await loaded;
  await sleep(1800);              /* fonts, first paint, the tour arming */

  /* ASK THE PAGE WHAT IT ACTUALLY GOT, and refuse rather than record. Under
     `reduce` the recording still walks all five scenes and every single frame
     still looks right; only the distinct-frame count betrays it. */
  const media = JSON.parse(await evaluate(cdp, `JSON.stringify({
    reduce: matchMedia('(prefers-reduced-motion: reduce)').matches,
    dark:   matchMedia('(prefers-color-scheme: dark)').matches
  })`));
  console.log(`media in page: reduced-motion=${media.reduce ? 'REDUCE' : 'no-preference'}, ` +
              `color-scheme=${media.dark ? 'dark' : 'light'}`);
  if (media.reduce) {
    die('the page reports prefers-reduced-motion: reduce despite the override.\n' +
        '  Every animated beat would be recorded as a cut. Refusing to record.');
  }
  if (!media.dark) die('the page is in light mode despite the override; refusing to record.');

  /* `scrollBehavior: auto` BEFORE the scroll, not after: the page asks for
     smooth scrolling and the box would otherwise be measured mid-flight. */
  await evaluate(cdp, `
    document.documentElement.style.scrollBehavior = 'auto';
    document.getElementById('how').scrollIntoView({ block: 'center' });
  `);
  await sleep(900);

  const lock = await phaseLock(cdp);
  console.log(`phase-locked on step 4 after ${(lock / 1000).toFixed(1)}s`);

  /* MEASURED AFTER THE LOCK, NOT BEFORE. The readout under the desk is empty
     until the tour writes its first line, so a box measured on arrival is
     short by that strip and crops the last table row — which reads as a layout
     bug in the page rather than as a mistake in the recorder. */
  const clip = await measureClip(cdp);
  console.log(`clip ${clip.width}x${clip.height} CSS at (${clip.x}, ${clip.y}) ` +
              `-> ${clip.width * DSF}x${clip.height * DSF} captured`);

  /* Real time, on an ABSOLUTE schedule so a slow shot costs one frame's
     lateness rather than shifting every frame after it. Each frame's true
     offset is kept and becomes its duration at encode time, so jitter shows up
     as correct timing rather than as drift. */
  console.log(`capturing ~${Math.round(CYCLE_MS / STEP_MS)} frames at ${STEP_MS}ms ` +
              `over ${(CYCLE_MS / 1000).toFixed(2)}s`);
  const stamps = [], hashes = new Set();
  const costs = [];
  const t0 = Date.now();
  for (let i = 0; ; i++) {
    const wait = t0 + i * STEP_MS - Date.now();
    if (wait > 0) await sleep(wait);
    const at = Date.now() - t0;
    if (at >= CYCLE_MS) break;

    const before = Date.now();
    const { data } = await cdp.send('Page.captureScreenshot', {
      format: SHOT.format, quality: SHOT.quality,
      clip: { ...clip, scale: 1 }, captureBeyondViewport: true
    });
    costs.push(Date.now() - before);

    await writeFile(frameFile(frameDir, i), Buffer.from(data, 'base64'));
    hashes.add(createHash('md5').update(data).digest('hex'));
    stamps.push(at);
    if ((i + 1) % 50 === 0) console.log(`  ${i + 1} frames, ${(at / 1000).toFixed(1)}s`);
  }

  costs.sort((a, b) => a - b);
  console.log(`shot cost: median ${costs[costs.length >> 1]}ms, ` +
              `p95 ${costs[Math.floor(costs.length * 0.95)]}ms, max ${costs[costs.length - 1]}ms`);
  return { stamps, unique: hashes.size, frames: stamps.length };
}

/* The guard the eyeball cannot be. See the header: a frozen compositor yields
   one distinct frame per scene and every one of them looks correct. */
function assertMotion({ unique, frames }) {
  const pct = ((unique / frames) * 100).toFixed(0);
  console.log(`distinct frames: ${unique} / ${frames} (${pct}%)`);
  if (unique < MIN_UNIQUE) {
    die(`only ${unique} distinct frames in ${frames}.\n` +
        `  That is a slideshow, not a recording — the compositor stopped\n` +
        `  producing frames and only DOM scene swaps were captured. Nothing\n` +
        `  was written. (Threshold --min-unique ${MIN_UNIQUE}.)`);
  }
}

/* The clip is the union of the grid and the demo. Taking the grid alone is
   wrong at any viewport wider than --maxw, where the desk is deliberately
   outside it. */
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
  /* Even dimensions after the device-scale multiply: several encoders reject
     odd ones, and rounding here is cheaper than finding out 300 frames later. */
  const even = n => Math.round(n / 2) * 2;
  return { x: Math.round(b.x), y: Math.round(b.y),
           width: even(b.width), height: even(b.height) };
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
  const GRAIN = 25;
  const LIMIT = CYCLE_MS * 3;
  const start = Date.now();
  let sawOther = false;
  while (Date.now() - start < LIMIT) {
    const step = await evaluate(cdp, PROBE);
    if (step && step !== '4') sawOther = true;
    if (step === '4' && sawOther) return Date.now() - start;
    await sleep(GRAIN);
  }
  die(`never saw the Launch row light up within ${LIMIT / 1000}s — the tour did ` +
      `not start (is #how on screen?)`);
}

/* ------------------------------------------------------------------ encode */

async function encode(frameDir, shot, out) {
  /* Each frame carries its measured offset, so the concat demuxer gets real
     durations and `fps` resamples from those to a constant rate. Feeding the
     numbered sequence at a fixed -framerate instead would assert an even
     spacing the real-time capture cannot guarantee. */
  const { stamps } = shot;
  const list = join(frameDir, 'frames.txt');
  let txt = '';
  for (let i = 0; i < stamps.length; i++) {
    const end = i + 1 < stamps.length ? stamps[i + 1] : CYCLE_MS;
    txt += `file '${frameFile(frameDir, i)}'\n`;
    txt += `duration ${((end - stamps[i]) / 1000).toFixed(4)}\n`;
  }
  /* The concat demuxer ignores the final entry's duration unless the last file
     is repeated. */
  txt += `file '${frameFile(frameDir, stamps.length - 1)}'\n`;
  await writeFile(list, txt);

  const palette = join(frameDir, 'palette.png');
  const fps = 100 / DELAY_CS;
  /* No scale filter at all unless asked: resampling is what softened the first
     version of this clip. */
  const chain = [`fps=${fps}`];
  if (OUT_W > 0) chain.push(`scale=${OUT_W}:-2:flags=lanczos`);
  const vf = chain.join(',');

  /* palettegen over every frame with stats_mode=diff, which weights the pixels
     that actually change — most of the desk, none of the table. paletteuse
     with Bayer: an ordered dither costs far fewer bytes than error diffusion on
     a flat UI, because it does not scatter fresh noise through regions that
     were identical between frames and would otherwise compress to nothing. */
  await run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y',
    '-f', 'concat', '-safe', '0', '-i', list,
    '-vf', `${vf},palettegen=max_colors=${COLORS}:stats_mode=diff`, palette]);

  await run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y',
    '-f', 'concat', '-safe', '0', '-i', list, '-i', palette,
    '-lavfi', `${vf}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle`,
    '-loop', '0', out]);

  const { size } = await import('node:fs').then(m => m.promises.stat(out));
  console.log(`\n${out}\n${(size / 1048576).toFixed(2)} MB, ${fps}fps, ` +
              `${COLORS} colors${OUT_W > 0 ? `, scaled to ${OUT_W}px` : ', native size'}`);
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

function frameFile(dir, i) {
  return join(dir, `f${String(i).padStart(4, '0')}.${SHOT.ext}`);
}

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

async function launchChrome() {
  const userDir = join(tmpdir(), `beckon-chrome-${process.pid}`);
  const proc = spawn(CHROME, [
    '--headless=new', '--remote-debugging-port=0',
    `--user-data-dir=${userDir}`,
    '--no-first-run', '--no-default-browser-check', '--disable-extensions',
    '--hide-scrollbars', '--force-device-scale-factor=1',
    '--disable-lcd-text',        /* subpixel AA becomes colour fringes in a 128-colour palette */
    '--font-render-hinting=none',
    /* NO --enable-begin-frame-control: it puts the compositor in a mode where
       frames appear only on an explicit HeadlessExperimental.beginFrame, which
       nothing here sends. Measured at 2 distinct frames out of 40. */
    '--disable-gpu', '--disable-dev-shm-usage',
    'about:blank'
  ], { stdio: ['ignore', 'ignore', 'pipe'] });

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
  const page = list.find(t => t.type === 'page');
  if (!page) throw new Error('chrome exposed no page target');
  return { proc, wsUrl: page.webSocketDebuggerUrl };
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
