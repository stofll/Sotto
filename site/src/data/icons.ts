/**
 * Icon shapes, extracted from the single-file draft where the same 30 shapes
 * were inlined 78 times. Each entry keeps its own viewBox and stroke width so
 * the brand marks and the waveform render exactly as they did before.
 */

export interface IconShape {
  body: string;
  viewBox?: string;
  strokeWidth?: string;
}

export const icons = {
  'apple': { body: '<path fill="currentColor" stroke="none" d="M16.5 2.1c.1 1.1-.4 2.2-1.1 3-.7.8-1.8 1.4-2.9 1.3-.2-1 .4-2.2 1-2.9.8-.9 1.9-1.4 3-1.4Zm3.4 15.3c-.5 1.1-.8 1.6-1.5 2.6-.9 1.2-2.1 2.7-3.6 2.7-1.3 0-1.7-.9-3.5-.9s-2.3.9-3.6.9c-1.4 0-2.6-1.4-3.5-2.6-2.4-3.4-2.7-7.4-1.3-9.5 1-1.5 2.6-2.4 4.2-2.4 1.6 0 2.7.9 4.1.9 1.4 0 2.3-.9 4.1-.9 1.4 0 2.9.8 3.9 2.1-3.4 1.8-2.9 6.5.7 7.1Z"></path>' },
  'arrow-right': { body: '<path d="M4 12h15M13 6l6 6-6 6"></path>' },
  'bolt': { body: '<path d="m13 2-9 12h7l-1 8 10-13h-7l0-7Z"></path>' },
  'book': { body: '<path d="M12 5c-3-2-6-2-10-1v15c4-1 7-1 10 1 3-2 6-2 10-1V4c-4-1-7-1-10 1v15M5 8h3M5 12h3m8-4h3m-3 4h3"></path>' },
  'check': { body: '<path d="m5 12 4 4L19 6"></path>' },
  'chip': { body: '<rect x="6" y="6" width="12" height="12" rx="2"></rect><path d="M9 2v4m6-4v4M9 18v4m6-4v4M2 9h4m-4 6h4m12-6h4m-4 6h4"></path><rect x="9" y="9" width="6" height="6" rx="1"></rect>' },
  'clock': { body: '<circle cx="12" cy="12" r="9"></circle><path d="M12 7v5l3 2"></path>' },
  'close': { body: '<path d="m6 6 12 12M18 6 6 18"></path>' },
  'code': { body: '<path d="m7 7-5 5 5 5m10-10 5 5-5 5M14 3l-4 18"></path>' },
  'copy': { body: '<rect x="8" y="8" width="12" height="13" rx="2"></rect><path d="M15 8V3H3v13h5"></path>' },
  'external': { body: '<path d="M6 18 18 6M6 6h12v12"></path>' },
  'file-bars': { body: '<path d="M14 2H5v20h14V7l-5-5Zm0 0v5h5M8 13v4m4-7v10m4-7v4"></path>' },
  'file-text': { body: '<path d="M14 3H5v18h14V8l-5-5Zm0 0v5h5M8 12h8M8 16h6"></path>' },
  'github': { body: '<path d="M9 19c-4.3 1.3-4.3-2.2-6-2.7m12 5v-3.7c0-1.1.1-1.6-.5-2.2 3.3-.4 6.7-1.6 6.7-7.3a5.7 5.7 0 0 0-1.5-4c.2-.9.2-2.2-.2-3.1 0 0-1.2-.4-4 1.5a13.5 13.5 0 0 0-7 0c-2.8-1.9-4-1.5-4-1.5-.4.9-.4 2.2-.2 3.1a5.7 5.7 0 0 0-1.5 4c0 5.7 3.4 6.9 6.7 7.3-.5.5-.6 1.2-.5 2.2v3.7"></path>' },
  'globe': { body: '<circle cx="12" cy="12" r="9"></circle><ellipse cx="12" cy="12" rx="4" ry="9"></ellipse><path d="M3 12h18"></path>' },
  'menu': { body: '<path d="M4 7h16M4 12h16M4 17h16"></path>' },
  'message': { body: '<path d="M20 15a3 3 0 0 1-3 3H9l-5 4V6a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v9Z"></path>' },
  'mic': { body: '<rect x="9" y="2" width="6" height="12" rx="3"></rect><path d="M5 10v2a7 7 0 0 0 14 0v-2m-7 9v3m-4 0h8"></path>' },
  'note': { body: '<path d="M4 5h16M4 10h16M4 15h11M4 20h7"></path>' },
  'obsidian': { body: '<path d="m11 1 7 7-2 11-8 4-6-8 3-9 6-5Zm0 0-3 10 8 8M8 11 2 15m6-4v12"></path>', viewBox: '0 0 20 24', strokeWidth: '1.5' },
  'pause': { body: '<path d="M8 4v16M16 4v16"></path>' },
  'play': { body: '<path d="m8 4 12 8-12 8V4Z"></path>' },
  'replay': { body: '<path d="M3 10a9 9 0 1 1 2 8M3 3v7h7"></path>' },
  'send': { body: '<path d="m2 10 20-7-5 19-6-7-5 3 2-6 10-6-7 9"></path>', strokeWidth: '1.5' },
  'share': { body: '<circle cx="6" cy="4" r="2"></circle><circle cx="6" cy="20" r="2"></circle><circle cx="18" cy="6" r="2"></circle><path d="M6 6v12m12-10c0 5-12 2-12 8"></path>' },
  'shield-check': { body: '<path d="m12 3 8 3v6c0 4.5-8 9-8 9s-8-4.5-8-9V6l8-3Z"></path><path d="m8 12 3 3 5-6"></path>' },
  'sliders': { body: '<path d="M4 6h16M4 12h16M4 18h16"></path><circle cx="8" cy="6" r="2"></circle><circle cx="16" cy="12" r="2"></circle><circle cx="10" cy="18" r="2"></circle>' },
  'waveform-mark': { body: '<path d="M5 19v6m8-13v20m8-28v36m8-32v28m8-21v14m8-10v6" stroke="currentColor" stroke-width="3" stroke-linecap="round"></path>', viewBox: '0 0 50 44' },
  'wifi-off': { body: '<path d="m3 3 18 18M7 7a13 13 0 0 1 14 2M3 9l1-1m3 5 2-1m5 0 3 1m-7 4 2-1M12 21h.01"></path>' },
  'windows': { body: '<path fill="currentColor" stroke="none" d="m2 4 8-1.1v8H2V4Zm10-1.4L22 1v9.9H12V2.6ZM2 13h8v8L2 20v-7Zm10 0h10v10l-10-1.5V13Z"></path>' },
} as const satisfies Record<string, IconShape>;

export type IconName = keyof typeof icons;
