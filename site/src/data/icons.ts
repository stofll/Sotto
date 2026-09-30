/** Icon paths, drawn on a 24 × 24 grid with a 1.6 stroke. */

export const icons = {
  'arrow-right': '<path d="M4 12h15M13 6l6 6-6 6"></path>',
  'close': '<path d="m6 6 12 12M18 6 6 18"></path>',
  'external': '<path d="M6 18 18 6M6 6h12v12"></path>',
  'keyboard': '<rect x="2.5" y="6" width="19" height="12" rx="2"></rect><path d="M6.5 10h.01M10.5 10h.01M14.5 10h.01M17.5 10h.01M8 14h8"></path>',
  'menu': '<path d="M4 7h16M4 12h16M4 17h16"></path>',
  'shield-check': '<path d="m12 3 8 3v6c0 4.5-8 9-8 9s-8-4.5-8-9V6l8-3Z"></path><path d="m8 12 3 3 5-6"></path>',
} as const satisfies Record<string, string>;

export type IconName = keyof typeof icons;
