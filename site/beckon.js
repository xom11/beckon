// Progressive enhancement only. Everything below is optional: with JS off the
// page still reads, every link works, and the theme follows the OS.
//
// The rule this file keeps, and it is the same one stated on .copy in
// beckon.css: nothing renders a control that silently does nothing. The theme
// button and the install tablist both ship with the `hidden` attribute and are
// revealed here; the copy buttons are created here and only when the clipboard
// API exists.
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
  // Android reports `Linux` in its user agent ("Linux; Android 14; Pixel 8"),
  // so testing Linux before Android opened every Android reader on the Nix
  // flake tab. Nothing is unreachable either way — the panels all ship visible
  // and every tab is one click away — this only picks the first one shown.
  const p = navigator.userAgent;
  const i = /Android/i.test(p) ? 0
    : /Mac/i.test(p) ? 0
    : /Win/i.test(p) ? 1
    : /Linux|X11/i.test(p) ? 3
    : 0;
  show(i);
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
