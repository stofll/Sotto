/** Icon paths, drawn on a 24 × 24 grid with a 1.6 stroke. */

export const icons = {
  'arrow-right': '<path d="M4 12h15M13 6l6 6-6 6"></path>',
  'close': '<path d="m6 6 12 12M18 6 6 18"></path>',
  'external': '<path d="M6 18 18 6M6 6h12v12"></path>',
  'folder': '<path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2.5h9A1.5 1.5 0 0 1 21 10v7.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5Z"></path>',
  'mic': '<rect x="9" y="3" width="6" height="11" rx="3"></rect><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"></path>',
  'menu': '<path d="M4 7h16M4 12h16M4 17h16"></path>',
  'shield-check': '<path d="m12 3 8 3v6c0 4.5-8 9-8 9s-8-4.5-8-9V6l8-3Z"></path><path d="m8 12 3 3 5-6"></path>',
} as const satisfies Record<string, string>;

export type IconName = keyof typeof icons;
