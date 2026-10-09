/** Icon paths, drawn on a 24 × 24 grid with a 1.6 stroke. */

export const icons = {
  'arrow-right': '<path d="M4 12h15M13 6l6 6-6 6"></path>',
  'close': '<path d="m6 6 12 12M18 6 6 18"></path>',
  'external': '<path d="M6 18 18 6M6 6h12v12"></path>',
  'clock': '<circle cx="12" cy="12" r="9"></circle><path d="M12 7v5l3 2"></path>',
  'cube': '<path d="M12 3 4 7.5v9L12 21l8-4.5v-9L12 3Z"></path><path d="M4 7.5 12 12l8-4.5M12 12v9"></path>',
  'file-audio': '<path d="M14 3H6v18h12V7l-4-4Z"></path><path d="M14 3v4h4M9 13v3M12 11v7M15 13v3"></path>',
  'folder': '<path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2.5h9A1.5 1.5 0 0 1 21 10v7.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5Z"></path>',
  'mic': '<rect x="9" y="3" width="6" height="11" rx="3"></rect><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"></path>',
  'menu': '<path d="M4 7h16M4 12h16M4 17h16"></path>',
  'moon': '<path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z"></path>',
  'sun': '<circle cx="12" cy="12" r="4"></circle><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"></path>',
  'shield-check': '<path d="m12 3 8 3v6c0 4.5-8 9-8 9s-8-4.5-8-9V6l8-3Z"></path><path d="m8 12 3 3 5-6"></path>',
  'sparkle': '<path d="M12 3v4M12 17v4M3 12h4M17 12h4M6.5 6.5l2.5 2.5M15 15l2.5 2.5M6.5 17.5 9 15M15 9l2.5-2.5"></path>',
  'text-check': '<path d="M4 7h11M4 12h8M4 17h6M14 16l3 3 5-6"></path>',
  // The hero's desktop: OS chrome and the messenger window.
  'chat': '<path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v8a2.5 2.5 0 0 1-2.5 2.5H10l-4 3.5V17h0a2 2 0 0 1-2-2Z"></path>',
  'checks': '<path d="m2.5 12.5 4 4 8-9M11.5 15.5l1 1 8-9"></path>',
  'globe': '<circle cx="12" cy="12" r="9"></circle><path d="M3 12h18M12 3c2.5 2.6 3.7 5.6 3.7 9s-1.2 6.4-3.7 9c-2.5-2.6-3.7-5.6-3.7-9S9.5 5.6 12 3Z"></path>',
  'mail': '<rect x="3" y="5.5" width="18" height="13" rx="2"></rect><path d="m3.5 7 8.5 6 8.5-6"></path>',
  'minus': '<path d="M5 12h14"></path>',
  'more': '<circle cx="12" cy="5.5" r=".9"></circle><circle cx="12" cy="12" r=".9"></circle><circle cx="12" cy="18.5" r=".9"></circle>',
  'note': '<path d="M6 3h12v18H6Z"></path><path d="M9 8h6M9 12h6M9 16h3"></path>',
  'paperclip': '<path d="m20 11.5-8 8a5 5 0 0 1-7-7l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7L9.7 17.2a1.7 1.7 0 0 1-2.4-2.4l7.7-7.7"></path>',
  'phone': '<path d="M5 4h3.5l1.5 4.5-2 1.5a11 11 0 0 0 6 6l1.5-2 4.5 1.5V19a1.5 1.5 0 0 1-1.5 1.5A16.5 16.5 0 0 1 3.5 5.5 1.5 1.5 0 0 1 5 4Z"></path>',
  'search': '<circle cx="11" cy="11" r="6.5"></circle><path d="m16 16 4.5 4.5"></path>',
  'smile': '<circle cx="12" cy="12" r="9"></circle><path d="M8.5 14.5a4.5 4.5 0 0 0 7 0M9 9.5h.01M15 9.5h.01"></path>',
  'square': '<rect x="5.5" y="5.5" width="13" height="13" rx="1"></rect>',
  'volume': '<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4Z"></path><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"></path>',
  'wifi': '<path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.5 16a5 5 0 0 1 7 0"></path><circle cx="12" cy="19" r=".6"></circle>',
} as const satisfies Record<string, string>;

export type IconName = keyof typeof icons;
