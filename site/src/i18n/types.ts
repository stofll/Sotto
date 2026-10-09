import type { CapabilityId } from '../data/capabilities';
import type { LanguageGroup } from '../data/product';

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

/** A label and its value in a scene's detail list. */
export type Detail = [string, string];

export type CleanupKind = 'term' | 'rule' | 'tic' | 'finish';

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
    | 'openMenu' | 'closeMenu' | 'darkTheme' | 'footerNav' | 'skipToContent' | 'noscript' | 'language',
    string
  >;
  hero: {
    eyebrow: string;
    titleLine1: string;
    titleLine2: string;
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
    /** The messenger on the hero's desktop. */
    demo: {
      app: string;
      /** The app's menus in the macOS menu bar, after its name. */
      menus: string[];
      /** The clock in the Windows taskbar, above `date`. */
      clock: string;
      date: string;
      /** The day and time in the macOS menu bar. */
      menuClock: string;
      search: string;
      /** Name, last message, time and unread count; the first chat is the open one. */
      chats: [string, string, string, number?][];
      status: string;
      /** The divider above today's messages. */
      today: string;
      messages: { text: string; time: string; mine?: boolean }[];
      placeholder: string;
      /** Raw words, as the overlay's live draft shows them. */
      spoken: string;
      /** The text pasted into the field. */
      pasted: string;
      /** One caption per step: before, during and after speaking. */
      steps: [string, string, string];
      /** `{n}` of `{total}` steps. */
      stepCount: string;
      processing: string;
      /** The overlay's note after a paste, `{n}` characters; worded as the app words it. */
      inserted: string;
      /** The choice under the desktop; look and palette names as the app names them. */
      tune: {
        label: string;
        auto: string;
        looks: Record<'pill' | 'glow' | 'caps' | 'scope' | 'mini' | 'bead' | 'term', string>;
        textLabel: string;
        /** Why both: words while speaking depend on the model, not on the overlay. */
        textHint: string;
        streaming: string;
        after: string;
        paletteLabel: string;
        palettes: Record<'graphite' | 'copper' | 'lagoon' | 'violet', string>;
      };
    };
  };
  apps: {
    eyebrow: string;
    title: string;
    text: string;
  };
  cleanup: {
    eyebrow: string;
    title: string;
    text: string;
    replay: string;
    beforeLabel: string;
    afterLabel: string;
    /** Plain text, or what was recognised, what came out (`''` when removed) and the step that did it. */
    sample: (string | [string, string, CleanupKind])[];
    rows: Record<CleanupKind, Detail>;
  };
  features: {
    eyebrow: string;
    title: string;
    listLabel: string;
    items: Record<CapabilityId, { name: string; text: string }>;
    /** The words the overlay streams in the voice card. */
    draft: string;
    models: { title: string };
    profile: {
      name: string;
      model: string;
      address: string;
      prompt: string;
    };
    history: { title: string; entries: [string, string][] };
    file: { title: string; name: string; done: string; result: string; note: string };
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
    providers: {
      title: string;
      local: string;
      llm: string;
      cloud: string;
      /** Closes the LLM and cloud lists. */
      compatible: string;
    };
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
