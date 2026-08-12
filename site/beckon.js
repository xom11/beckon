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
