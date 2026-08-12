// Progressive enhancement only. Everything below is optional: with JS off the
// page still reads, every link works, and the theme follows the OS.
//
// The rule this file keeps, and it is the same one stated on .copy in
// beckon.css: nothing renders a control that silently does nothing. The theme
// button, the OS switcher and the install tablist all ship with the `hidden`
// attribute and are revealed here; the copy buttons are created here and only
// when the clipboard API exists.
//
// TWO pieces of shared state, both stamped on <html> by the inline script in
// <head> before first paint, both remembered in localStorage, both overridable
// by a control in the nav: `data-theme` and `data-os`. This file owns the
// controls and nothing else. `data-os` changes are announced on document as a
// `beckon:os` event so a consumer — today the install tablist, tomorrow
// whatever else follows the reader's OS — can subscribe without this file
// knowing about it.
(() => {
  const root = document.documentElement;
  const btn = document.getElementById('theme');
  if (!btn) return;

  // The saved theme is already applied by the inline script in <head> — before
  // first paint, which a `defer`red file cannot promise. This only owns the
  // button.
  //
  // `dataset.theme` is unset until the reader chooses, so it cannot be the only
  // input to the label: on a dark-by-default machine the ternary fell to the
  // `dark` branch and the button offered to switch to the theme already on
  // screen. aria-label is the button's only accessible name — the visible word
  // is the neutral "Theme" — so that is the whole announcement, with nothing to
  // correct it. Resolve the same way the click handler does.
  const isDark = () => root.dataset.theme
    ? root.dataset.theme === 'dark'
    : matchMedia('(prefers-color-scheme: dark)').matches;
  const label = () =>
    btn.setAttribute('aria-label', 'Switch to ' + (isDark() ? 'light' : 'dark') + ' theme');

  label();
  btn.hidden = false;
  btn.addEventListener('click', () => {
    root.dataset.theme = isDark() ? 'light' : 'dark';
    localStorage.setItem('beckon-theme', root.dataset.theme);
    label();
  });
})();

// The OS switcher, decision for decision the same shape as the theme button
// above: the head bootstrap owns the value and applies it before first paint,
// this owns the control.
//
// No label() equivalent is needed here and that is the point of choosing a
// <select>: its accessible name is the fixed word "OS" and its accessible
// VALUE is the current OS, so it announces the state the reader is in rather
// than the one they would move to. A cycling button can only name the next
// state, which is the bug the theme button's aria-label had before it was
// resolved against matchMedia.
(() => {
  const root = document.documentElement;
  const wrap = document.getElementById('os-switch');
  const sel = document.getElementById('os-select');
  if (!wrap || !sel) return;

  // Normally already stamped by <head>. Restamped here because the bootstrap
  // is inside a try/catch — a browser that throws on localStorage (Safari
  // private mode used to) leaves `data-os` unset, and a switcher showing
  // "macOS" over a page in no OS state at all is a control that lies. The
  // fallback matches the bootstrap's: see the <head> comment for why linux.
  const os = root.dataset.os || 'linux';
  root.dataset.os = os;
  sel.value = os;
  wrap.hidden = false;

  sel.addEventListener('change', () => {
    root.dataset.os = sel.value;
    // Wrapped, unlike the theme button's: a throw here would skip the dispatch
    // below and leave the install tabs on the previous OS while the marker in
    // #setups had already moved. Failing to remember is survivable; disagreeing
    // with itself on the same page is not.
    try { localStorage.setItem('beckon-os', sel.value); } catch (e) {}
    document.dispatchEvent(new CustomEvent('beckon:os'));
  });
})();

// Install tabs. The HTML ships every panel visible and the tablist hidden;
// this only ever *reduces* what is on screen, so JS-off degrades to
// "all four platforms listed", which is a fine page.
(() => {
  const list = document.querySelector('#install [role="tablist"]');
  if (!list) return;
  const tabs = [...list.querySelectorAll('[role="tab"]')];
  const panels = tabs.map(t => document.getElementById(t.getAttribute('aria-controls')));
  const show = i => {
    tabs.forEach((t, n) => {
      t.setAttribute('aria-selected', String(n === i));
      t.tabIndex = n === i ? 0 : -1;
      panels[n].hidden = n !== i;
    });
  };
  // The ARIA tab pattern is created here, with the widget. In the markup the
  // panels are plain labelled <section>s, so a JS-off reader is not told about
  // four tab panels belonging to a tablist that is display:none. tabindex="0"
  // because a panel whose only focusable content is a copy button has none at
  // all when navigator.clipboard is absent.
  panels.forEach(p => { p.setAttribute('role', 'tabpanel'); p.tabIndex = 0; });
  list.hidden = false;
  tabs.forEach((t, i) => {
    t.addEventListener('click', () => show(i));
    t.addEventListener('keydown', e => {
      const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
      if (!d) return;
      e.preventDefault();
      const n = (i + d + tabs.length) % tabs.length;
      show(n); tabs[n].focus();
    });
  });
  // Which tab opens first follows `data-os`, the same state the nav's switcher
  // writes and the head bootstrap detects. This used to sniff
  // navigator.userAgent right here, privately, which made the guess
  // uncorrectable: a reader on the wrong tab could click across but could not
  // tell the page it had guessed wrong, and nothing else on the page moved
  // with them. Cargo (index 2) is nobody's default — it is the from-source
  // channel and needs a toolchain — so it is reachable only by clicking.
  const order = { macos: 0, windows: 1, linux: 3 };
  const pick = () => {
    const i = order[document.documentElement.dataset.os];
    return i === undefined ? 0 : i;
  };
  show(pick());
  document.addEventListener('beckon:os', () => show(pick()));
})();

// Copy buttons. Hidden entirely when the clipboard API is absent, rather than
// rendering a button that silently does nothing.
(() => {
  if (!navigator.clipboard) return;
  document.querySelectorAll('#install pre').forEach(pre => {
    const code = pre.querySelector('code');
    const text = code.textContent.trim();
    const first = text.split('\n')[0];

    const b = document.createElement('button');
    b.className = 'copy'; b.type = 'button'; b.textContent = 'Copy';
    // Every panel holds one or two of these and "Copy" is the same word on all
    // of them, so a screen reader's button list could not tell `brew install
    // xom11/tap/beckon` from `brew services start beckon`. The command is the
    // name.
    b.setAttribute('aria-label', 'Copy ' + first);

    // The confirmation is a visible label swap, which assistive technology is
    // never told about — and it cannot be the button's own name, because the
    // aria-label above overrides its text. A sibling live region is the one
    // place the change can be announced.
    const say = document.createElement('span');
    say.className = 'sr-only'; say.setAttribute('role', 'status');

    b.addEventListener('click', async () => {
      await navigator.clipboard.writeText(text);
      b.textContent = 'Copied';
      say.textContent = 'Copied ' + first;
      setTimeout(() => { b.textContent = 'Copy'; say.textContent = ''; }, 1400);
    });
    pre.appendChild(b);
    pre.appendChild(say);
  });
})();

// Native <details> already works. This only closes siblings.
document.querySelectorAll('#faq details').forEach(d =>
  d.addEventListener('toggle', () => {
    if (!d.open) return;
    d.parentElement.querySelectorAll('details[open]').forEach(o => { if (o !== d) o.open = false; });
  }));
