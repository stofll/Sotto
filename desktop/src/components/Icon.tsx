type IconProps = { name: string; size?: number } & React.SVGProps<SVGSVGElement>;

export function Icon({ name, size = 16, ...rest }: IconProps) {
  const props = { width: size, height: size, viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: 1.5, strokeLinecap: "round", strokeLinejoin: "round", ...rest } as const;
  switch (name) {
    case "mic": return <svg {...props}><rect x="6" y="2" width="4" height="8" rx="2"/><path d="M3.5 7.5a4.5 4.5 0 0 0 9 0M8 12v2M5.5 14h5"/></svg>;
    // Headphones for echo monitoring: a headband arc and two cups. The mic on
    // the neighbouring button needs a visually dissimilar partner — otherwise
    // two buttons in a row read as one on/off pair for a single mode.
    case "headphones": return <svg {...props}><path d="M3 10.5V8a5 5 0 0 1 10 0v2.5"/><rect x="2" y="9.5" width="3" height="4.5" rx="1.5"/><rect x="11" y="9.5" width="3" height="4.5" rx="1.5"/></svg>;
    case "sliders": return <svg {...props}><path d="M2 4h7M11 4h3M2 8h3M7 8h7M2 12h9M13 12h1"/><circle cx="10" cy="4" r="1.4"/><circle cx="6" cy="8" r="1.4"/><circle cx="12" cy="12" r="1.4"/></svg>;
    case "replace": return <svg {...props}><path d="M3 5h7l-2-2M3 5l2 2M13 11H6l2 2M13 11l-2-2"/></svg>;
    // The «Текст» section used to be marked with a magic wand: five sparks of
    // assorted sizes around a diagonal read as clutter and said nothing about text.
    case "text": return <svg {...props}><path d="M3 4.9V3.4h10v1.5M8 3.4v9.2M6 12.6h4"/></svg>;
    case "wand": return <svg {...props}><path d="M11 3l1 1M14 6l-1-1M9 5l1 1M3 13l8-8 1 1-8 8z"/><path d="M5 3v2M4 4h2M12 9v2M11 10h2"/></svg>;
    case "chart": return <svg {...props}><path d="M2 14h12"/><path d="M4 10v3M7 6v7M10 8v5M13 4v9"/></svg>;
    case "info": return <svg {...props}><circle cx="8" cy="8" r="6"/><path d="M8 7v4M8 5v.01"/></svg>;
    case "cpu": return <svg {...props}><rect x="4" y="4" width="8" height="8" rx="1"/><path d="M6 6h4v4H6zM6 1.5v1.5M10 1.5v1.5M6 13v1.5M10 13v1.5M1.5 6h1.5M1.5 10h1.5M13 6h1.5M13 10h1.5"/></svg>;
    case "server": return <svg {...props}><rect x="2.5" y="2.5" width="11" height="4" rx="1"/><rect x="2.5" y="9.5" width="11" height="4" rx="1"/><path d="M5 4.5h.01M5 11.5h.01M8 6.5v3"/></svg>;
    case "gpu": return <svg {...props}><rect x="1.5" y="4" width="13" height="7" rx="1"/><circle cx="5" cy="7.5" r="1.5"/><circle cx="10" cy="7.5" r="1.5"/><path d="M3 11v2M12 11v2"/></svg>;
    case "globe": return <svg {...props}><circle cx="8" cy="8" r="6"/><path d="M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12"/></svg>;
    case "play": return <svg {...props}><path d="M5 3l8 5-8 5z" fill="currentColor"/></svg>;
    case "pause": return <svg {...props}><rect x="4" y="3" width="3" height="10" fill="currentColor"/><rect x="9" y="3" width="3" height="10" fill="currentColor"/></svg>;
    case "check": return <svg {...props}><path d="M3 8l3.5 3.5L13 5"/></svg>;
    case "x": return <svg {...props}><path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/></svg>;
    case "plus": return <svg {...props}><path d="M8 3v10M3 8h10"/></svg>;
    case "search": return <svg {...props}><circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5L14 14"/></svg>;
    case "chev": return <svg {...props}><path d="M5 4l4 4-4 4"/></svg>;
    case "chev-down": return <svg {...props}><path d="M4 6l4 4 4-4"/></svg>;
    case "chev-left": return <svg {...props}><path d="M11 4l-4 4 4 4"/></svg>;
    case "chev-right": return <svg {...props}><path d="M5 4l4 4-4 4"/></svg>;
    case "chev-up": return <svg {...props}><path d="M4 10l4-4 4 4"/></svg>;
    case "test": return <svg {...props}><path d="M6 2v3.5L3 11.5a2 2 0 0 0 1.7 3h6.6a2 2 0 0 0 1.7-3L10 5.5V2"/><path d="M5 2h6"/></svg>;
    case "eye": return <svg {...props}><path d="M1.5 8c1.5-3 3.8-4.5 6.5-4.5S13 5 14.5 8C13 11 10.7 12.5 8 12.5S3 11 1.5 8z"/><circle cx="8" cy="8" r="2"/></svg>;
    case "eye-off": return <svg {...props}><path d="M3 3l10 10M5.5 5.5C3.8 6.3 2.5 7.5 1.5 8c1.5 3 3.8 4.5 6.5 4.5 1.2 0 2.3-.3 3.2-.8M9.8 4.3A6.6 6.6 0 0 1 14.5 8c-.5 1-1.1 1.8-1.9 2.4"/><path d="M6.8 6.8a2 2 0 0 0 2.4 2.4"/></svg>;
    case "chip": return <svg {...props}><rect x="4" y="4" width="8" height="8" rx="1"/><path d="M6.5 4V2.5M9.5 4V2.5M6.5 13.5V12M9.5 13.5V12M2.5 6.5H4M2.5 9.5H4M12 6.5h1.5M12 9.5h1.5"/></svg>;
    case "kbd": return <svg {...props}><rect x="1.5" y="3.5" width="13" height="9" rx="1.5"/><path d="M4 6.5h.01M7 6.5h.01M10 6.5h.01M4 9.5h6"/></svg>;
    case "spark": return <svg {...props}><path d="M8 2v3M8 11v3M2 8h3M11 8h3M3.8 3.8l2 2M10.2 10.2l2 2M3.8 12.2l2-2M10.2 5.8l2-2"/></svg>;
    case "shield": return <svg {...props}><path d="M8 1.5l5.5 2v4.5C13.5 11 11 13.5 8 14.5C5 13.5 2.5 11 2.5 8V3.5z"/><path d="M5.5 8l2 2 3-4"/></svg>;
    // Trash can: the lid spans the full width, the body is a trapezoid from 4.5
    // to 13.4 in height. The previous version read as squashed because the
    // handle took only 1.5 units and the body started almost at the lid.
    case "trash": return <svg {...props}><path d="M2.5 4.5h11"/><path d="M6.25 4.5V3.4c0-.5.4-.9.9-.9h1.7c.5 0 .9.4.9.9v1.1"/><path d="M12.15 4.5l-.55 8.05c-.04.5-.45.9-.95.9H5.35c-.5 0-.91-.4-.95-.9L3.85 4.5"/></svg>;
    case "clock": return <svg {...props}><circle cx="8" cy="8" r="6"/><path d="M8 4.5V8l2.5 1.5"/></svg>;
    case "copy": return <svg {...props}><rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M2.5 10V3a.5.5 0 0 1 .5-.5h7"/></svg>;
    case "folder": return <svg {...props}><path d="M2 12.5v-9a.5.5 0 0 1 .5-.5h3.6l1.4 1.8h5a.5.5 0 0 1 .5.5v7.2a.5.5 0 0 1-.5.5h-10a.5.5 0 0 1-.5-.5Z"/></svg>;
    case "pencil": return <svg {...props}><path d="M3 13l1.5-3 7-7 2 2-7 7zM10 4l2 2"/></svg>;
    case "key": return <svg {...props}><circle cx="5" cy="11" r="2.5"/><path d="M7 9l5-5 1 1-1 1 1 1-1 1 1 1-2 2"/></svg>;
    case "arrow-right": return <svg {...props}><path d="M3 8h10M9 4l4 4-4 4"/></svg>;
    case "download": return <svg {...props}><path d="M8 2v8M4.5 6.5L8 10l3.5-3.5M3 13h10"/></svg>;
    case "more": return <svg {...props}><circle cx="4" cy="8" r="1"/><circle cx="8" cy="8" r="1"/><circle cx="12" cy="8" r="1"/></svg>;
    case "compare": return <svg {...props}><path d="M5 2v12M11 2v12M2 5l3-3 3 3M14 11l-3 3-3-3"/></svg>;
    case "filter": return <svg {...props}><path d="M2 3h12l-4.5 5.5V13L6.5 11.5V8.5z"/></svg>;
    case "refresh": return <svg {...props}><path d="M13 5a5.5 5.5 0 0 0-9.4-1.9L2.5 4.2M3 11a5.5 5.5 0 0 0 9.4 1.9l1.1-1.1"/><path d="M2.5 1.8v2.4h2.4M13.5 14.2v-2.4h-2.4"/></svg>;
    case "sun": return <svg {...props}><circle cx="8" cy="8" r="2.5"/><path d="M8 1.5v1.2M8 13.3v1.2M1.5 8h1.2M13.3 8h1.2M3.4 3.4l.9.9M11.7 11.7l.9.9M3.4 12.6l.9-.9M11.7 4.3l.9-.9"/></svg>;
    case "moon": return <svg {...props}><path d="M12.8 10.4A5.5 5.5 0 0 1 5.6 3.2 5.7 5.7 0 1 0 12.8 10.4z"/></svg>;
    // Sidebar: a window with a filled left rail. Collapsing used to be drawn as
    // a lone chevron pointing left — the same glyph that means "back" everywhere
    // else, and that is exactly how people read it.
    case "panel": return <svg {...props}><rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2"/><path d="M6.25 2.75v10.5"/><path d="M1.75 4.75a2 2 0 0 1 2-2h2.5v10.5h-2.5a2 2 0 0 1-2-2z" fill="currentColor" stroke="none"/></svg>;
    case "brand-openai": return <svg {...props} viewBox="0 0 16 16"><path d="M8 1.8 10 3l2.2.1 1.2 2-.3 2.2.9 2-1.3 1.8-2.1.6L9.2 13.4H6.8l-1.4-1.7-2.1-.6L2 9.3l.9-2-.3-2.2 1.2-2L6 3z"/><path d="M5.1 6.2 8 4.5l2.9 1.7v3.6L8 11.5 5.1 9.8zM8 4.5v7M5.1 6.2l5.8 3.6M10.9 6.2 5.1 9.8"/></svg>;
    case "brand-anthropic": return <svg {...props} viewBox="0 0 16 16"><path d="M3 13 7.1 3h1.8L13 13h-2l-.8-2.2H5.8L5 13z"/><path d="M6.4 9.1h3.2L8 4.8z"/></svg>;
    case "brand-gemini": return <svg {...props} viewBox="0 0 16 16"><path d="M8 1.8c.7 3 2.2 4.5 5.2 5.2C10.2 7.8 8.7 9.3 8 14.2 7.3 9.3 5.8 7.8 2.8 7 5.8 6.3 7.3 4.8 8 1.8z"/><path d="M12.2 1.8c.2 1 .7 1.5 1.7 1.8-1 .2-1.5.8-1.7 1.8-.3-1-.8-1.6-1.8-1.8 1-.3 1.5-.8 1.8-1.8z"/></svg>;
    case "brand-opencode": return <svg {...props} viewBox="0 0 16 16"><path d="M6.2 4 2.6 8l3.6 4M9.8 4l3.6 4-3.6 4"/><path d="M8.8 3.2 7.2 12.8"/></svg>;
    case "brand-compatible": return <svg {...props} viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.8"/><path d="M2.6 8h10.8M8 2.2c1.4 1.5 2.1 3.5 2.1 5.8s-.7 4.3-2.1 5.8C6.6 12.3 5.9 10.3 5.9 8S6.6 3.7 8 2.2z"/><path d="M4.1 4.3c1 .7 2.4 1.1 3.9 1.1s2.9-.4 3.9-1.1M4.1 11.7c1-.7 2.4-1.1 3.9-1.1s2.9.4 3.9 1.1"/></svg>;
    case "flag-ru": return <svg width={size} height={(size / 16) * 12} viewBox="0 0 16 12" fill="none"><rect width="16" height="4" fill="#fff"/><rect y="4" width="16" height="4" fill="#0039A6"/><rect y="8" width="16" height="4" fill="#D52B1E"/></svg>;
    case "flag-en": return <svg width={size} height={(size / 16) * 12} viewBox="0 0 16 12" fill="none"><rect width="16" height="12" fill="#012169"/><path d="M0 0l16 12M16 0L0 12" stroke="#fff" strokeWidth="2"/><path d="M8 0v12M0 6h16" stroke="#fff" strokeWidth="3"/><path d="M8 0v12M0 6h16" stroke="#C8102E" strokeWidth="1.5"/></svg>;
    default: return null;
  }
}
