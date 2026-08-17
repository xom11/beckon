#!/usr/bin/env node
/* Render site/og.png — the 1200x630 card Facebook, Zalo, X, Slack and iMessage
 * show when someone pastes https://xom11.github.io/beckon/.
 *
 *   node tools/make-og-card.mjs
 *   node tools/make-og-card.mjs --out /tmp/probe.png --chrome <path>
 *
 * WHY THIS EXISTS AT ALL. `og:image` used to be `icon-512.png`, the 512x512 app
 * icon — so every share rendered as a small square of the letter `b` beside the
 * text. Two separate things made it small: a square image is not the 1.91:1 a
 * feed lays out edge to edge, and `twitter:card` was `summary` rather than
 * `summary_large_image`. Both are fixed in index.html; this file supplies the
 * picture that fix points at.
 *
 * WHY A GENERATOR AND NOT A HAND-DRAWN PNG. The card repeats three strings that
 * live elsewhere in the repo — the `<h1>`, the `beckon Chrome` example, and the
 * three OS names. A binary blob cannot be diffed against them, so it goes stale
 * in silence, which is exactly what CLAUDE.md records about
 * `assets/five-answers.gif`. Here the strings are constants in this file and
 * tools/check-site.sh compares HEADLINE against index.html's own `<h1>`, so a
 * reworded headline fails CI instead of shipping a card that contradicts the
 * page it advertises.
 *
 * WHY THE CARD HTML IS A TEMPLATE LITERAL AND NOT A FILE IN site/. Everything
 * under site/ is published by .github/workflows/pages.yml. A card.html sitting
 * there would be a real, reachable, indexable URL that renders as a headline
 * with no navigation — a second front page nobody meant to publish. The PNG is
 * the artifact; the source that made it belongs with the tool.
 *
 * THE TYPE IS BAKED, WHICH IS THE POINT. system-ui resolves to SF Pro on this
 * machine, so the card carries macOS letterforms everywhere it is shown. That
 * is a feature of rendering to a raster once rather than a caveat: unlike the
 * live page, a viewer's font stack cannot reflow it.
 */

import { writeFile, mkdir, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');

/* ---- the copy on the card ------------------------------------------------
 * HEADLINE is pinned against site/index.html's `<h1>` by check-site.sh. The
 * other two are pinned by eye only — COMMAND appears in the hero's lead
 * paragraph and OSES in the install section, but both are short enough that a
 * check would be asserting a substring of a substring. */
const HEADLINE = ['One key per app.', 'Every machine you own.'];
const COMMAND = 'beckon Chrome';
const OSES = [
  { name: 'macOS',   tint: '#7C5CC4' },   /* --os-tint, beckon.css §1 */
  { name: 'Windows', tint: '#2A5FC8' },
  { name: 'Linux',   tint: '#1E8794' },
];

/* The card is 1200x630 because that is the size both Facebook's and Zalo's
 * scrapers lay out edge to edge (1.91:1). Zalo's floor is 600x315; rendering at
 * double that costs ~40 KB and survives a retina feed. */
const W = 1200;
const H = 630;

const args = parseArgs(process.argv.slice(2));
const OUT = resolve(ROOT, args.out ?? 'site/og.png');
const KEEP = !!args.keep;

const CHROME =
  args.chrome ??
  ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
   '/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',
   '/Applications/Chromium.app/Contents/MacOS/Chromium'].find(p => existsSync(p));

if (!CHROME) die('no Chromium-family browser found; pass --chrome <path>');

main().catch(err => die(err.stack ?? String(err)));

async function main() {
  const workDir = join(tmpdir(), `beckon-og-card-${process.pid}`);
  await mkdir(workDir, { recursive: true });
  await mkdir(dirname(OUT), { recursive: true });

  const htmlPath = join(workDir, 'card.html');
  await writeFile(htmlPath, cardHtml(), 'utf8');

  const chrome = await launchChrome();
  try {
    const cdp = await connect(chrome.wsUrl);
    await cdp.send('Page.enable');
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: W, height: H, deviceScaleFactor: 1, mobile: false
    });

    const loaded = cdp.once('Page.loadEventFired');
    await cdp.send('Page.navigate', { url: `file://${htmlPath}` });
    await loaded;

    /* A screenshot taken before the webfont-free system stack has settled still
     * renders — with fallback metrics — so the failure is a subtly wrong card
     * rather than a blank one. Wait for the font engine to say it is done. */
    await cdp.send('Runtime.evaluate', {
      expression: 'document.fonts.ready.then(() => true)',
      awaitPromise: true
    });

    const { data } = await cdp.send('Page.captureScreenshot', {
      format: 'png',
      clip: { x: 0, y: 0, width: W, height: H, scale: 1 },
      captureBeyondViewport: true
    });
    await writeFile(OUT, Buffer.from(data, 'base64'));
  } finally {
    chrome.proc.kill('SIGKILL');
    if (!KEEP) await rm(workDir, { recursive: true, force: true });
  }

  console.log(`wrote ${OUT} (${W}x${H})`);
}

/* ---- the card ------------------------------------------------------------ */

function cardHtml() {
  const chips = OSES.map(o => `
      <li class="os" style="--tint:${o.tint}">
        <span class="dot"></span>${o.name}
      </li>`).join('');

  return `<!doctype html>
<meta charset="utf-8">
<title>beckon — share card</title>
<style>
  /* Palette lifted from site/beckon.css §1. The ground is the dark canvas the
     page uses (#131316) with all three --os-tint values washed across it, so
     the card carries the same "one line, three machines" idea in colour before
     a single word is read. */
  * { margin: 0; padding: 0; box-sizing: border-box; }

  html, body { width: ${W}px; height: ${H}px; }

  body {
    display: flex;
    flex-direction: column;
    justify-content: center;   /* the block is shorter than the card; centre it
                                  rather than pinning the chips to the floor and
                                  leaving a hole in the middle */
    padding: 64px;
    background:
      radial-gradient(115% 135% at 4% -10%,  rgba(124, 92, 196, .52), transparent 62%),
      radial-gradient(105% 125% at 96% 8%,   rgba(42, 95, 200, .46),  transparent 58%),
      radial-gradient(130% 120% at 58% 112%, rgba(30, 135, 148, .38), transparent 64%),
      #131316;
    color: #F4F3F0;
    font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    -webkit-font-smoothing: antialiased;
  }

  /* ---- wordmark ---- */
  .brand { display: flex; align-items: center; gap: 16px; }
  .mark { width: 44px; height: 44px; display: block; }
  .brand b {
    font-size: 30px; font-weight: 600; letter-spacing: -.015em;
    color: rgba(244, 243, 240, .92);
  }

  /* ---- headline ----
     The one element that has to survive being shrunk to a thumbnail in a phone
     feed, so it takes the largest size the two lines fit at. */
  h1 {
    margin-top: 40px;
    font-size: 88px;
    line-height: 1.06;
    font-weight: 700;
    letter-spacing: -.03em;
  }
  h1 span { display: block; }
  h1 span + span { color: rgba(244, 243, 240, .70); }

  /* ---- the command ----
     Drawn as a terminal chip rather than bare text: the claim is that one typed
     line does this, and a chip says "typed" without a word of explanation. */
  .cmd {
    margin-top: 38px;
    align-self: flex-start;
    display: flex; align-items: center; gap: 14px;
    padding: 16px 26px;
    border-radius: 14px;
    background: rgba(255, 255, 255, .07);
    border: 1px solid rgba(255, 255, 255, .14);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 30px;
    letter-spacing: -.01em;
  }
  .cmd .prompt { color: rgba(244, 243, 240, .42); }
  .cmd .verb { color: #F4F3F0; }
  .cmd .arg { color: #9DC4FF; }

  /* ---- the three machines ---- */
  ul { list-style: none; display: flex; gap: 14px; margin-top: 40px; }
  .os {
    display: flex; align-items: center; gap: 11px;
    padding: 13px 24px;
    border-radius: 999px;
    font-size: 24px; font-weight: 500;
    color: rgba(244, 243, 240, .88);
    background: color-mix(in oklab, var(--tint) 22%, transparent);
    border: 1px solid color-mix(in oklab, var(--tint) 55%, transparent);
  }
  .dot {
    width: 11px; height: 11px; border-radius: 50%;
    background: var(--tint);
    box-shadow: 0 0 0 4px color-mix(in oklab, var(--tint) 24%, transparent);
  }
</style>

<div class="brand">
  <!-- Same artwork as assets/beckon.ico and site/favicon.png: the #3B82F6 ->
       #2563EB tile with a LOWER-CASE b. Upper case is wrong here — CLAUDE.md
       records a capital B shipping to the macOS menu bar for exactly as long as
       nobody looked at it. -->
  <svg class="mark" viewBox="0 0 34 34" aria-hidden="true">
    <defs>
      <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#3B82F6"/>
        <stop offset="1" stop-color="#2563EB"/>
      </linearGradient>
    </defs>
    <rect width="34" height="34" rx="8" fill="url(#tile)"/>
    <text x="17" y="17" fill="#EDEFF4" font-size="23" font-weight="600"
          font-family="system-ui, -apple-system, sans-serif"
          text-anchor="middle" dominant-baseline="central">b</text>
  </svg>
  <b>beckon</b>
</div>

<h1>${HEADLINE.map(l => `<span>${esc(l)}</span>`).join('\n  ')}</h1>

<div class="cmd">
  <span class="prompt">$</span><span><span class="verb">${esc(COMMAND.split(' ')[0])}</span> <span class="arg">${esc(COMMAND.split(' ').slice(1).join(' '))}</span></span>
</div>

<ul>${chips}
</ul>
`;
}

function esc(s) {
  return s.replace(/[&<>]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));
}

/* ---- chrome plumbing -----------------------------------------------------
 * Same shape as tools/record-five-answers.mjs. Kept as a copy rather than
 * factored into a shared module: the two scripts have no other overlap, and a
 * lib/ shared by exactly two callers is a third file to keep in step. */

async function launchChrome() {
  const userDir = join(tmpdir(), `beckon-og-chrome-${process.pid}`);
  const proc = spawn(CHROME, [
    '--headless=new', '--remote-debugging-port=0',
    `--user-data-dir=${userDir}`,
    '--no-first-run', '--no-default-browser-check', '--disable-extensions',
    '--hide-scrollbars', '--force-device-scale-factor=1',
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
  let page = list.find(t => t.type === 'page');
  if (!page) {
    await fetch(`http://${base}/json/new?about:blank`, { method: 'PUT' });
    page = (await fetch(`http://${base}/json/list`).then(r => r.json()))
      .find(t => t.type === 'page');
  }
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

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith('--')) continue;
    const k = a.slice(2);
    const next = argv[i + 1];
    if (next === undefined || next.startsWith('--')) out[k] = true;
    else { out[k] = next; i++; }
  }
  return out;
}

function die(msg) {
  console.error(`make-og-card: ${msg}`);
  process.exit(1);
}
