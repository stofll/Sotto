import type { FeatureStepId, OverlayPalette, OverlayPhase, OverlayTemplate, ScreenRegion } from '../data/feature-tour';
import type { LanguageGroup, ScreenId } from '../data/product';

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

export interface FeatureStep {
  /** Short name in the step list. */
  name: string;
  title: string;
  text: string;
}

/** A screenshot step: the capture's description and the regions it can zoom into. */
export interface ScreenScene<S extends ScreenId> {
  alt: string;
  regions: Record<ScreenRegion<S>, Detail>;
}

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
    stepsLabel: string;
    /** `{n}` of `{total}` steps. */
    stepCount: string;
    steps: Record<FeatureStepId, FeatureStep>;
    catalog: ScreenScene<'models'>;
    history: ScreenScene<'history'>;
    cleanup: {
      beforeLabel: string;
      afterLabel: string;
      /** Plain text, or what was recognised, what came out (`''` when removed) and the step that did it. */
      sample: (string | [string, string, CleanupKind])[];
      rows: Record<CleanupKind, Detail>;
    };
    profile: {
      name: string;
      fields: { name: string; provider: string; model: string; address: string; prompt: string };
      model: string;
      address: string;
      prompt: string;
      rows: Detail[];
    };
    overlay: {
      templateLabel: string;
      paletteLabel: string;
      phaseLabel: string;
      templates: Record<OverlayTemplate, string>;
      palettes: Record<OverlayPalette, string>;
      phases: Record<OverlayPhase, string>;
      processing: string;
      /** The words a streaming model shows while you speak. */
      draft: string;
      /** Shown under the bead and the orb while streaming, which hide the draft. */
      hiddenDraft: string;
      rowLabels: string[];
      /** One value per row label, for each template. */
      rowValues: Record<OverlayTemplate, string[]>;
    };
    file: {
      title: string;
      dragNote: string;
      pick: string;
      cancel: string;
      reading: string;
      transcribing: string;
      local: string;
      name: string;
      result: string;
      rows: Detail[];
    };
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
