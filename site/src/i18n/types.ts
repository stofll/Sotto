import type { LanguageGroup, ScreenId } from '../data/product';
import type { TourStepId } from '../data/screen-tour';

/**
 * The shape every locale must satisfy. Adding a string here makes TypeScript
 * fail on any locale that has not translated it yet, which is what keeps the
 * two languages from drifting apart.
 */

export type Locale = 'en' | 'ru';

export interface Card {
  title: string;
  text: string;
}

export interface Dictionary {
  meta: {
    locale: Locale;
    htmlLang: string;
    title: string;
    description: string;
    ogTitle: string;
    ogDescription: string;
    ogImageAlt: string;
  };
  nav: Record<
    | 'home' | 'main' | 'mobile' | 'features' | 'models' | 'privacy' | 'faq' | 'start' | 'download'
    | 'openMenu' | 'closeMenu' | 'footerNav' | 'skipToContent' | 'noscript' | 'language',
    string
  >;
  hero: {
    eyebrow: string;
    titleLine1: string;
    titleLine2: string;
    /** Typed out one by one in the demo. */
    phrases: string[];
    subtitle: string;
    /** Shown when the reader's system is unknown; leads to both platforms. */
    download: string;
    downloadWindows: string;
    downloadMac: string;
    /** Shown instead of a download on phones and tablets. */
    mobileNote: string;
    meta: string;
    /** `{version}` of the latest release, resolved at build time. */
    version: string;
  };
  features: {
    eyebrow: string;
    title: string;
    modesLabel: string;
    toggle: string;
    pushToTalk: string;
    toggleHint: string;
    pushToTalkHint: string;
    /** Misheard word, then the replacement that fixes it. */
    replacements: [string, string][];
    /** Time and text of sample history entries. */
    historyRows: [string, string][];
    anywhere: Card;
    offline: Card;
    live: Card;
    custom: Card;
    files: Card;
    history: Card;
  };
  screens: {
    eyebrow: string;
    title: string;
    tablistLabel: string;
    enlarge: string;
    close: string;
    captions: Record<ScreenId, string>;
    tourPause: string;
    tourPlay: string;
    themeLabel: string;
    themes: { dark: string; light: string };
    previous: string;
    next: string;
    stepLabel: string;
    imageError: string;
    steps: Record<TourStepId, Card>;
    tabs: Record<ScreenId, string>;
    alt: Record<ScreenId, string>;
  };
  models: {
    eyebrow: string;
    title: string;
    description: string;
    purposes: string[];
    allModels: string;
    filterLabel: string;
    filters: Record<'all' | LanguageGroup, string>;
    streamingOnly: string;
    /** `{shown}` of `{total}` models. */
    count: string;
    empty: string;
    streaming: string;
    punctuation: string;
    note: string;
  };
  privacy: {
    link: string;
  };
  start: {
    eyebrow: string;
    title: string;
    steps: Card[];
    windows: string;
    windowsFormats: string;
    mac: string;
    note: string;
  };
  faq: {
    eyebrow: string;
    title: string;
    factsLabel: string;
    /** Short label and value; `{total}` is the model count. */
    facts: [string, string][];
    /** Questions and answers, also published as FAQPage structured data. */
    items: { id: string; question: string; answer: string }[];
  };
  final: { title: string };
  footer: Record<'docs' | 'privacy', string>;
  dialog: {
    eyebrow: string;
    closeAria: string;
    titleMac: string;
    titleWindows: string;
    descriptionMac: string;
    descriptionWindows: string;
    stepsMac: string[];
    stepsWindows: string[];
    notice: string;
    noticeLink: string;
    openRelease: string;
    directMac: string;
    directWindows: string;
    hostedOnGitHub: string;
    checking: string;
    resolved: string;
    choosePlatform: string;
    lookupFailed: string;
    latestRelease: string;
    allReleases: string;
  };
  notFound: {
    title: string;
    lede: string;
    backHome: string;
    readDocs: string;
    reportIssue: string;
    /** Caption under the number, and the code itself for anyone who cannot see it. */
    caption: string;
    markAlt: string;
  };
}
