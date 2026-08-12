// Progressive enhancement only. Everything below is optional: with JS off the
// page still reads, every link works, and the theme follows the OS.
(() => {
  const root = document.documentElement;
  const btn = document.getElementById('theme');
  if (!btn) return;
  const saved = localStorage.getItem('beckon-theme');
  if (saved) root.dataset.theme = saved;
  const label = () =>
    (btn.setAttribute('aria-label',
      'Switch to ' + (root.dataset.theme === 'dark' ? 'light' : 'dark') + ' theme'));
  label();
  btn.addEventListener('click', () => {
    const dark = root.dataset.theme
      ? root.dataset.theme === 'dark'
      : matchMedia('(prefers-color-scheme: dark)').matches;
    root.dataset.theme = dark ? 'light' : 'dark';
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
  const p = navigator.userAgent;
  show(/Mac/i.test(p) ? 0 : /Win/i.test(p) ? 1 : /Linux|X11/i.test(p) ? 3 : 0);
})();

// Copy buttons. Hidden entirely when the clipboard API is absent, rather than
// rendering a button that silently does nothing.
(() => {
  if (!navigator.clipboard) return;
  document.querySelectorAll('#install pre').forEach(pre => {
    const b = document.createElement('button');
    b.className = 'copy'; b.type = 'button'; b.textContent = 'Copy';
    b.addEventListener('click', async () => {
      await navigator.clipboard.writeText(pre.querySelector('code').textContent.trim());
      b.textContent = 'Copied'; setTimeout(() => (b.textContent = 'Copy'), 1400);
    });
    pre.appendChild(b);
  });
})();

// Native <details> already works. This only closes siblings.
document.querySelectorAll('#faq details').forEach(d =>
  d.addEventListener('toggle', () => {
    if (!d.open) return;
    d.parentElement.querySelectorAll('details[open]').forEach(o => { if (o !== d) o.open = false; });
  }));
