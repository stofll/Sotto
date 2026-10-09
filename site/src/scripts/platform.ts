import type { Behaviour, Runtime } from './runtime';

type Platform = 'windows' | 'macos' | 'mobile';

interface UserAgentData {
  platform?: string;
  mobile?: boolean;
}

/**
 * Best guess from what the browser reports locally; nothing is sent anywhere.
 * An iPad asks for the desktop site and reports itself as a Mac, which its
 * touch points give away. Apple Silicon and Intel Macs cannot be told apart
 * reliably (Safari reports Intel on both), so a Mac is offered the only build
 * there is and the setup dialog says which one it is.
 */
const detect = (): Platform | null => {
  const data = (navigator as Navigator & { userAgentData?: UserAgentData }).userAgentData;
  const platform = data?.platform || navigator.platform || '';
  const agent = navigator.userAgent;
  if (data?.mobile || /Android|iPhone|iPad|iPod/i.test(agent)) return 'mobile';
  if (/Mac/i.test(platform) && navigator.maxTouchPoints > 1) return 'mobile';
  if (/Win/i.test(platform)) return 'windows';
  if (/Mac/i.test(platform)) return 'macos';
  return null;
};

/**
 * Points the download buttons at the reader's own system. Every button starts
 * out leading to the section with both platforms, which is also what a reader
 * on Linux, on a phone, or without JavaScript gets.
 *
 * Must run before the download dialog binds its `[data-download]` links.
 */
export const initPlatform = ({ query, all, strings }: Runtime): Behaviour => {
  const platform = detect();

  if (platform === 'mobile') {
    query('[data-mobile-note]')?.classList.add('is-shown');
    return {};
  }
  if (!platform) return {};

  // The hero's desktop wears the reader's own system.
  const desk = query('[data-desk]');
  if (desk) desk.dataset.os = platform;

  // The platform cards: the reader's own goes first and gets the fill.
  const cards = query('[data-platforms]');
  const own = cards?.querySelector<HTMLElement>(`[data-download="${platform}"]`);
  if (cards && own) {
    own.classList.add('platform-primary');
    cards.prepend(own);
  }

  // The dialog lives on the landing page only; elsewhere the buttons keep
  // leading to its download section.
  if (!query('[data-download-dialog]')) return {};
  const release = query<HTMLAnchorElement>('[data-download]')?.href;
  all<HTMLAnchorElement>('[data-platform-cta]').forEach((button) => {
    button.dataset.download = platform;
    if (release) button.href = release;
    if (button.dataset.platformCta === 'long') {
      button.textContent = platform === 'windows' ? strings.downloadWindows : strings.downloadMac;
    }
  });
  return {};
};
