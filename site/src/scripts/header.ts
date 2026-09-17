import type { Behaviour, Runtime } from './runtime';

/** Sticky-header state plus an accessible disclosure menu. */
export const initHeader = ({ query, all, signal, strings }: Runtime): Behaviour => {
  const header = query('[data-header]');
  const menu = query('#mobile-nav');
  const menuToggle = query('[data-menu-toggle]');
  let headerFrame = 0;

  const setMenu = (open: boolean, restoreFocus = false) => {
    if (!menu || !menuToggle) return;
    menu.hidden = !open;
    menuToggle.setAttribute('aria-expanded', String(open));
    menuToggle.setAttribute('aria-label', open ? strings.nav.closeMenu : strings.nav.openMenu);
    header?.classList.toggle('menu-open', open);
    if (restoreFocus) menuToggle.focus();
  };

  const updateHeader = () => {
    header?.classList.toggle('is-scrolled', window.scrollY > 40);
    headerFrame = 0;
  };

  updateHeader();
  window.addEventListener(
    'scroll',
    () => {
      if (!headerFrame) headerFrame = requestAnimationFrame(updateHeader);
    },
    { passive: true, signal },
  );

  menuToggle?.addEventListener('click', () => setMenu(menu?.hidden ?? true), { signal });
  all('#mobile-nav a').forEach((link) => link.addEventListener('click', () => setMenu(false), { signal }));

  document.addEventListener(
    'keydown',
    (event) => {
      if (event.key === 'Escape' && menu && !menu.hidden) setMenu(false, true);
    },
    { signal },
  );

  document.addEventListener(
    'click',
    (event) => {
      if (menu && !menu.hidden && event.target instanceof Node && !header?.contains(event.target)) setMenu(false);
    },
    { signal },
  );

  window.matchMedia('(min-width: 601px)').addEventListener(
    'change',
    (event) => {
      if (event.matches) setMenu(false);
    },
    { signal },
  );

  return { destroy: () => cancelAnimationFrame(headerFrame) };
};
