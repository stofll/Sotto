import type { AppId, ModelId, WorkflowId } from '../data/product';

/**
 * The shape every locale must satisfy. Adding a string here makes TypeScript
 * fail on any locale that has not translated it yet, which is what keeps the
 * two languages from drifting apart.
 */

export type Locale = 'en' | 'ru';

export interface Scenario {
  title: string;
  sub: string;
  app: string;
  destination: string;
  destinationSub: string;
  context: string;
  /** Spoken fragments, revealed one by one in the typing animation. */
  chunks: string[];
}

export interface ListItem {
  icon: string;
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
  };
  nav: Record<
    | 'home' | 'main' | 'mobile' | 'features' | 'models' | 'privacy' | 'docs' | 'docsLong'
    | 'github' | 'githubAria' | 'download' | 'openMenu' | 'closeMenu' | 'backToTop' | 'footerNav'
    | 'skipToContent' | 'noscript' | 'language',
    string
  >;
  hero: {
    eyebrow: string[];
    titleLine1: string;
    titleLine2: string;
    subtitleLine1: string;
    subtitleLine2: string;
    downloadMac: string;
    downloadWindows: string;
    noSubscription: string;
    viewOnGitHub: string;
    demoAria: string;
    demoDisclaimer: string;
    caption: string;
    captionExtra: string;
    replay: string;
    replayAria: string;
    steps: { number: string; label: string }[];
    editor: {
      app: string;
      workspace: string;
      personal: string;
      allNotes: string;
      currentNote: string;
      quickThoughts: string;
      ideas: string;
      breadcrumbRoot: string;
      intro: string;
      tasks: string[];
      savedLocally: string;
      format: string;
    };
    overlay: { local: string; exploreModels: string; hide: string; model: string; stopHint: string };
    sideNoteLine1: string;
    sideNoteLine2: string;
  };
  benefits: { label: string; items: ListItem[] };
  workflow: {
    eyebrow: string;
    titleLine1: string;
    titleLine2: string;
    description: string;
    tablistLabel: string;
    caption: string;
    overlayLabel: string;
    scenarios: Record<WorkflowId, Scenario>;
  };
  works: { title: string; subtitle: string; listLabel: string; note: string; apps: Record<AppId, string> };
  models: {
    eyebrow: string;
    titleLine1: string;
    titleLine2: string;
    descriptionLine1: string;
    descriptionLine2: string;
    principles: string[];
    guideLink: string;
    windowTitle: string;
    panelTitle: string;
    panelSubtitle: string;
    tablistLabel: string;
    localTab: string;
    cloudTab: string;
    cloudTabBadge: string;
    radiogroupLabel: string;
    streamingBadge: string;
    languageLabels: Record<'multilingual' | 'russian' | 'sizes', string>;
    megabytes: string;
    details: Record<ModelId, string>;
    cloud: { title: string; text: string; note: string; link: string };
    footnote: string;
  };
  privacy: {
    eyebrow: string;
    titleLine1: string;
    titleLine2: string;
    descriptionLine1: string;
    descriptionLine2: string;
    diagramAlt: string;
    boundary: string;
    nodes: Record<'microphone' | 'app' | 'model' | 'text', string>;
    noCloud: string;
    points: { number: string; title: string; text: string }[];
    footnote: string;
    footnoteLink: string;
  };
  features: {
    eyebrow: string;
    title: string;
    subtitleLine1: string;
    subtitleLine2: string;
    items: (ListItem & { overline: string })[];
    footnote: string;
  };
  openSource: Record<
    | 'eyebrow' | 'titleLine1' | 'titleLine2' | 'manifesto' | 'descriptionLine1' | 'descriptionLine2'
    | 'viewOnGitHub' | 'repoPublic' | 'repoOpenAria' | 'readmeName' | 'readmeTitle' | 'readmeText'
    | 'tagLocal' | 'tagLicense' | 'copyAria' | 'copied' | 'copiedTitle' | 'copyManual'
    | 'copyFallbackTitle' | 'copiedAria',
    string
  >;
  download: Record<'eyebrow' | 'title' | 'subtitle' | 'mac' | 'windows' | 'linux' | 'setupNotes', string>;
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
  footer: Record<'tagline' | 'docs' | 'privacy' | 'license' | 'freeSoftware' | 'builtInTheOpen', string>;
  demo: {
    phases: Record<'idle' | 'ready' | 'listening' | 'processing' | 'done' | 'reset', string>;
    prompt: string;
    transcript: string[];
    pause: string;
    resume: string;
    motionDisabled: string;
  };
}
