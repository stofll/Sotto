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
    title: 'What Sotto does',
    stepsLabel: 'Sotto features',
    stepCount: '{n} / {total}',
    steps: {
      catalog: { name: 'Catalog', title: 'A model for your language', text: 'Languages, size and speed are visible before you download. A model downloads once and then works offline.' },
      cleanup: { name: 'Cleanup', title: 'Text without the clutter', text: 'Dictionaries fix terms and names, your rules replace words, and the ums and uhs disappear. All on your computer, no LLM involved.' },
      profile: { name: 'Profile', title: 'An LLM, if you want one', text: 'A profile ties together a provider, a key, a model and your prompt. A cloud model works, and so does a local one in Ollama or LM Studio.' },
      overlay: { name: 'Overlay', title: 'An overlay to your taste', text: 'Recording, live text, processing and pasting show in a small window above your apps. Its shape, colour and position are up to you.' },
      history: { name: 'History', title: 'Everything you said, at hand', text: 'History stays on your computer. Find an entry, copy it or process it again with the LLM.' },
      file: { name: 'File', title: 'Transcribe an audio file', text: 'Open a recording and a local model turns it into text. The result stays in the panel: it is not added to history or pasted into a window.' },
    },
    catalog: {
      alt: 'The Sotto model catalog: search, a language filter and model cards',
      regions: {
        languages: ['Search and language', 'by name or language'],
        resources: ['Size and memory', 'before downloading'],
        streaming: ['Streaming', 'words as you speak'],
      },
    },
    history: {
      alt: 'Sotto dictation history: search, entries and the Process with the LLM action',
      regions: {
        search: ['Search', 'by text and date'],
        copy: ['Copy', 'in one click'],
        formatting: ['Process with the LLM', 'with a diff of the changes'],
      },
    },
    cleanup: {
      beforeLabel: 'Recognised',
      afterLabel: 'Pasted',
      sample: [
        ['uh ', '', 'tic'], ['c', 'C', 'finish'], 'ommit this to ', ['git hub', 'GitHub', 'term'], ' and open a ',
        ['pull request', 'PR', 'rule'], ' so ', ['claude code', 'Claude Code', 'term'], ' can review it', ['', '.', 'finish'],
      ],
      rows: {
        term: ['Dictionaries', 'GitHub, Claude Code'],
        rule: ['Your replacements', 'pull request → PR'],
        tic: ['Filler sounds', 'uh, um'],
        finish: ['Finishing', 'capital and full stop'],
      },
    },
    profile: {
      name: 'Local Ollama',
      fields: { name: 'Profile name', provider: 'Provider', model: 'Model', address: 'Address', prompt: 'Prompt' },
      model: 'llama3.1',
      address: 'http://localhost:11434/v1',
      prompt: 'Add punctuation and split long text into paragraphs. Do not add anything of your own.',
      rows: [
        ['Providers', 'cloud and local'],
        ['Key', 'not needed for local ones'],
        ['Prompt', 'one per profile'],
        ['Profiles', 'for dictation and for the panel'],
      ],
    },
    overlay: {
      templateLabel: 'Template',
      paletteLabel: 'Colour',
      phaseLabel: 'State',
      templates: { pill: 'Pill', bead: 'Bead', glow: 'Glow', orb: 'Orb' },
      palettes: { graphite: 'Graphite', copper: 'Copper', lagoon: 'Lagoon', violet: 'Purple' },
      phases: { rec: 'Recording', stream: 'Streaming', proc: 'Processing', done: 'Pasted' },
      processing: 'Processing',
      draft: 'The meeting moves to Thursday, 3 pm',
      hiddenDraft: 'Live transcription text is hidden in the bead.',
      rowLabels: ['Shell', 'Level', 'Timer', 'Draft', 'Cancel', 'Position'],
      rowValues: {
        pill: ['pill', 'bars', 'in a capsule', 'below the row', 'on hover', 'bottom centre'],
        bead: ['bead', 'ring', 'none', 'hidden', 'on hover', 'bottom centre'],
        glow: ['card', 'edge glow', 'digits', 'in the card', 'on hover', 'bottom centre'],
        orb: ['bead without outline', 'orb', 'none', 'hidden', 'on hover', 'bottom centre'],
      },
    },
    file: {
      title: 'Transcribe an audio file',
      dragNote: 'drag-and-drop supported',
      pick: 'Pick a file',
      cancel: 'Cancel',
      reading: 'Reading the file…',
      transcribing: 'Transcribing audio…',
      local: 'The file is being transcribed by the local model',
      name: 'interview.m4a',
      result: 'At first we only needed fast input for notes during calls. Cloud services were ruled out by our data requirements, so everything had to run locally.',
      rows: [
        ['Formats', 'wav, mp3, m4a and more'],
        ['Model', 'your local one'],
        ['Internet', 'not needed'],
        ['History', 'kept separate'],
      ],
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
        answer: 'That depends on the model. Whisper covers about a hundred languages and Omnilingual more than 1600. GigaAM v3 is built for Russian. Nemotron 3.5 supports 28 languages, including Russian, English, Chinese, Japanese and Korean; Parakeet TDT v3 supports 25 European languages. Parakeet TDT v2, Parakeet unified, Canary and Moonshine are English-only in Sotto. SenseVoice supports Chinese, English, Japanese, Korean and Cantonese.',
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
