import type { Dictionary } from './types';

export const en: Dictionary = {
  meta: {
    locale: 'en',
    htmlLang: 'en',
    title: 'Sotto: offline voice typing for Windows and macOS',
    description:
      'Free, open-source voice typing for Windows and macOS. Speech is recognized on your computer without the internet, and the text goes into any app.',
    ogTitle: 'Sotto — Speak. Get more done.',
    ogDescription: 'Free, open-source desktop dictation. Local by default.',
    ogImageAlt: 'The Sotto logo above the headline “Speak. Get more done.” and an orange voice line',
  },

  nav: {
    home: 'Sotto home',
    main: 'Main navigation',
    mobile: 'Mobile navigation',
    features: 'Features',
    models: 'Models',
    privacy: 'Privacy',
    faq: 'FAQ',
    start: 'Start',
    download: 'Download',
    openMenu: 'Open navigation',
    closeMenu: 'Close navigation',
    footerNav: 'Footer navigation',
    language: 'Language',
    skipToContent: 'Skip to content',
    noscript: 'Interactive previews need JavaScript. Downloads and documentation still work.',
  },

  hero: {
    eyebrow: 'Sotto: offline voice typing',
    titleLine1: 'Speak.',
    titleLine2: 'Get more done.',
    phrases: [
      'Remind me to buy bread and call mom at seven.',
      'The meeting moves to Thursday, 3 pm.',
      'Summarize this email for the team.',
    ],
    subtitle:
      'Sotto turns speech into text and drops it into whatever app you’re using.',
    download: 'Download free',
    downloadWindows: 'Download for Windows',
    downloadMac: 'Download for macOS',
    mobileNote: 'Sotto runs on computers with Windows or macOS.',
    meta: 'Windows and macOS, open source, MIT',
    version: 'Version {version}',
  },

  features: {
    eyebrow: 'Features',
    title: 'Speak instead of typing',
    modesLabel: 'Hotkey mode',
    toggle: 'Toggle',
    pushToTalk: 'Push-to-talk',
    toggleHint: 'press, speak, press again',
    pushToTalkHint: 'hold the key while you speak',
    replacements: [['chat gpt', 'ChatGPT'], ['pdf', 'PDF']],
    historyRows: [
      ['8:20 PM', 'Buy coffee and pick up the parcel'],
      ['7:18 PM', 'Idea: quick search across notes'],
    ],
    anywhere: {
      title: 'Dictate anywhere',
      text: 'One hotkey works in every app: chats, notes, AI prompts. Text lands in the active field or on the clipboard.',
    },
    offline: {
      title: 'Works offline',
      text: 'Download a model once and recognition runs on your computer. Free, with no per-minute limits.',
    },
    live: {
      title: 'Words as you speak',
      text: 'Streaming models show live text in a compact overlay. No need to open the main window.',
    },
    custom: {
      title: 'Make it yours',
      text: 'Replacements for names and terms. For punctuation and formatting, connect an LLM and save instructions as profiles.',
    },
    files: {
      title: 'Transcribe recordings',
      text: 'Turn an interview, lecture or voice memo into text.',
    },
    history: {
      title: 'History at hand',
      text: 'Find a past dictation, copy the text or process it again.',
    },
  },

  screens: {
    eyebrow: 'Interface',
    title: 'Make it your own',
    tablistLabel: 'App screens',
    enlarge: 'Take a closer look',
    close: 'Close',
    captions: {
      settings: 'Choose your shortcut, microphone and recording mode: press to toggle or hold while speaking.',
      models: 'See languages, size and capabilities before downloading. A loaded model is ready for your next dictation.',
      history: 'Keep your dictations at hand: search, copy or process the text again.',
    },
    themeLabel: 'App theme',
    themes: { dark: 'Dark', light: 'Light' },
    previous: 'Previous step',
    next: 'Next step',
    stepLabel: 'Step',
    imageError: 'Could not load this screen. Try selecting it again.',
    steps: {
      shortcut: { title: 'Your keyboard shortcut', text: 'Click the pencil beside the shortcut to set your own. It starts dictation from any application, without returning to Sotto.' },
      recording: { title: 'Press or hold', text: 'Toggle: press once to start recording and again to stop. Hold: speak while holding the shortcut, then release it to transcribe your speech.' },
      microphone: { title: 'Check your microphone', text: 'Choose an audio source from the list. The microphone button starts an input level check; the meter on the right helps you see whether you can be heard.' },
      languages: { title: 'Narrow your search', text: 'Search by model name or choose a language from the list. Downloaded only shows models already available on your computer.' },
      resources: { title: 'Compare before downloading', text: 'Each card shows languages, size, approximate memory requirements and speed. GigaAM v3, for example, is designed for Russian and takes 214 MB.' },
      streaming: { title: 'Words as you speak', text: 'Streaming models can show partial text in the overlay while you speak. The green In memory label marks the model that is already loaded.' },
      search: { title: 'Find a past dictation', text: 'Search for a phrase, choose a time range or filter entries by processing status. History helps you return to something you have already dictated.' },
      copy: { title: 'Copy the result', text: 'The two-sheet button copies an entry to your clipboard. Paste it into a document, message or another application.' },
      formatting: { title: 'Process the text again', text: 'Process with the LLM applies your configured language model to a saved entry. Connect a provider in Sotto first to use this feature.' },
    },
    tourPause: 'Pause the tour',
    tourPlay: 'Play the tour',
    tabs: { settings: 'Settings', models: 'Models', history: 'History' },
    alt: {
      settings: 'Sotto settings: shortcut, recording mode, languages, microphone and auto-paste',
      models: 'The Sotto model catalog with local speech recognition models',
      history: 'Sotto transcription history with recent dictations and a Process with the LLM action',
    },
  },

  models: {
    eyebrow: 'Models',
    title: 'A model for your language and your machine',
    description:
      'Russian, multiple languages or words as you speak. Pick a model and download it once.',
    filterLabel: 'Filter models',
    purposes: ['For Russian', 'Words as you speak', 'For multiple languages', 'The Whisper family'],
    allModels: 'All models · {total}',
    filters: { all: 'All', ru: 'Русский', en: 'English', cjk: '中文, 日本語, 한국어' },
    streamingOnly: 'Streaming only',
    count: '{shown} of {total}',
    empty: 'No models match these filters.',
    streaming: 'streaming',
    punctuation: 'punctuation',
    note: 'Whisper sizes are approximate. Details:',
  },

  privacy: {
    link: 'Privacy summary',
  },

  start: {
    eyebrow: 'Start',
    title: 'Three steps to your first phrase',
    steps: [
      { title: 'Download', text: 'Windows: .exe installer or portable ZIP. macOS: open the .dmg and drag Sotto into Applications.' },
      { title: 'Pick a model', text: 'Open Models and download one: GigaAM v3 for Russian, Whisper or Parakeet TDT v3 for many languages.' },
      { title: 'Say something', text: 'Put the cursor in a text field. Press the hotkey, speak, then press it again to stop and insert the text. In push-to-talk mode, hold the key while speaking and release it to finish.' },
    ],
    windows: 'Windows x64',
    windowsFormats: '.exe or portable .zip',
    mac: 'macOS, Apple Silicon',
    note: 'Builds don’t have a publisher certificate yet, so Windows or macOS may warn on first launch.',
  },

  faq: {
    eyebrow: 'Questions',
    title: 'What people ask about Sotto',
    factsLabel: 'Sotto in brief',
    facts: [
      ['Platforms', 'Windows x64, macOS on Apple Silicon'],
      ['Price', 'Free, no subscription or account'],
      ['License', 'Open source, MIT'],
      ['Recognition', 'On your computer, with whisper.cpp and sherpa-onnx'],
      ['Models', '{total} in the built-in catalog'],
      ['Cloud', 'Cloud recognition and LLM formatting, if you want them'],
    ],
    items: [
      {
        id: 'offline',
        question: 'Does Sotto work without the internet?',
        answer: 'Yes. Once a model is downloaded, speech is recognized on your computer. The internet is only needed to download the app and the models, and for cloud providers if you set one up.',
      },
      {
        id: 'price',
        question: 'How much does Sotto cost?',
        answer: 'Nothing. Sotto is free and open source under the MIT license, with no subscription, no account and no per-minute limits.',
      },
      {
        id: 'platforms',
        question: 'Which systems does it run on?',
        answer: 'Windows x64 and macOS on Apple Silicon. Windows requires a processor with AVX2, FMA and F16C support, including when using GPU acceleration. Intel Macs and Linux are not currently supported release targets.',
      },
      {
        id: 'languages',
        question: 'Which languages does it recognize?',
        answer: 'That depends on the model. Whisper covers about a hundred languages and Omnilingual more than 1600. GigaAM v3 is built for Russian; GigaAM Multilingual and GigaAM Multilingual Large support Russian, English, Kazakh, Kyrgyz and Uzbek. Nemotron 3.5 supports 28 languages, including Russian, English, Chinese, Japanese and Korean; Parakeet TDT v3 and Parakeet Ultra support 25 European languages. Qwen3 ASR 0.6B supports 30 languages, including Russian, English, Chinese, Japanese and Korean. Parakeet TDT v2, Parakeet unified, Canary and Moonshine are English-only in Sotto. SenseVoice supports Chinese, English, Japanese, Korean and Cantonese.',
      },
      {
        id: 'hardware',
        question: 'Do I need a powerful graphics card?',
        answer: 'No. Every model runs on the processor. Whisper can also use the graphics card where the build supports it.',
      },
      {
        id: 'privacy',
        question: 'Where does my voice go?',
        answer: 'By default it stays on your computer. Audio is sent to the cloud only if you connect a cloud recognition provider yourself. De-identified usage statistics never include audio or text, and can be turned off.',
      },
      {
        id: 'audio-files',
        question: 'Can I transcribe an audio file offline?',
        answer: 'Yes. Download and select a local model, then open an audio file in Sotto to get its transcript. File transcription stays separate from dictation history and does not paste text into the active window. Cloud recognition is optional and must be configured separately.',
      },
      {
        id: 'comparison',
        question: 'How is Sotto different from built-in voice typing?',
        answer: 'You choose the recognition model and it runs on your computer. Sotto works in any app with your own hotkey, keeps a local history, transcribes audio files, applies your replacements for names and terms, and can hand the text to an LLM for punctuation and formatting.',
      },
    ],
  },

  final: { title: 'Say the first word' },

  footer: {
    docs: 'Docs',
    privacy: 'Privacy',
  },

  dialog: {
    eyebrow: 'Setup',
    closeAria: 'Close download details',
    titleMac: 'Sotto for macOS',
    titleWindows: 'Sotto for Windows',
    descriptionMac: 'Choose the Apple Silicon .dmg from the latest GitHub release.',
    descriptionWindows: 'Choose the x64 .exe installer from the latest GitHub release. A portable ZIP is also available.',
    stepsMac: [
      'Open the .dmg and drag Sotto into Applications.',
      'Grant microphone and Accessibility permissions when prompted.',
      'Download a model, select it, and set your dictation shortcut.',
    ],
    stepsWindows: [
      'Run the x64 .exe installer, or unpack the portable ZIP.',
      'Allow microphone access when prompted.',
      'Download a model, select it, and set your dictation shortcut.',
    ],
    notice:
      'Builds currently lack a publisher certificate. Your operating system may warn on first launch.',
    noticeLink: 'Read the verification and first-launch guide.',
    openRelease: 'Open GitHub release',
    directMac: 'Download .dmg for Apple Silicon',
    directWindows: 'Download .exe for Windows x64',
    hostedOnGitHub: 'Official downloads are hosted on GitHub.',
    checking: 'Checking for the latest official installer…',
    resolved: '{version}, official release asset on GitHub',
    choosePlatform: 'Choose the installer for your platform on the GitHub release page.',
    lookupFailed: 'Direct-link lookup unavailable. The GitHub release page still works.',
    latestRelease: 'Latest release',
    allReleases: 'All releases and other installation options',
  },

  notFound: {
    title: 'This page isn’t here',
    lede: 'The link may be out of date, or the address may have a typo in it. Everything about Sotto lives on the home page.',
    backHome: 'Back to the home page',
    readDocs: 'Read the documentation',
    reportIssue: 'Report a broken link',
    caption: 'That’s all we could make out.',
    markAlt: 'Error 404',
  },
};
