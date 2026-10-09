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
    ogImageAlt: 'The name Sotto above the headline “Speak. Get more done.”',
  },

  nav: {
    home: 'Sotto home',
    main: 'Main navigation',
    mobile: 'Mobile navigation',
    features: 'Features',
    models: 'Models',
    faq: 'FAQ',
    start: 'Start',
    download: 'Download',
    openMenu: 'Open navigation',
    darkTheme: 'Dark theme',
    closeMenu: 'Close navigation',
    footerNav: 'Footer navigation',
    language: 'Language',
    skipToContent: 'Skip to content',
    noscript: 'Interactive previews need JavaScript. Downloads and documentation still work.',
  },

  hero: {
    titleLine1: 'Speak.',
    titleLine2: 'Get more done.',
    subtitle:
      'Sotto turns speech into text and drops it into whatever app you’re using.',
    download: 'Download free',
    downloadWindows: 'Download for Windows',
    downloadMac: 'Download for macOS',
    mobileNote: 'Sotto runs on computers with Windows or macOS.',
    meta: 'Windows and macOS, open source, MIT',
    version: 'Version {version}',
    demo: {
      app: 'Messenger',
      menus: ['File', 'Edit', 'View', 'Window', 'Help'],
      clock: '2:53 PM',
      date: '10/8/2026',
      menuClock: 'Thu Oct 8 2:53 PM',
      search: 'Search',
      chats: [
        ['Max Orlov', 'Are you joining the call at 3?', '2:52 PM'],
        ['Product team', 'Anna: mockups are in the folder', '2:40 PM', 3],
        ['Anna Sokolova', 'Thanks, got the report', '1:15 PM'],
        ['Design review', 'You: let’s do Thursday', '12:02 PM'],
        ['Lena', 'Weekend photos', 'Yesterday'],
      ],
      status: 'last seen just now',
      today: 'Today',
      messages: [
        { text: 'Hi! What time is the call today?', time: '2:20 PM' },
        { text: 'The call moves to 3 PM, same link', time: '2:31 PM', mine: true },
        { text: 'OK. Are you joining the call at 3?', time: '2:52 PM' },
        { text: 'And send me the notes afterwards, please', time: '2:52 PM' },
      ],
      placeholder: 'Message',
      spoken: 'um running ten minutes late start without me I’ll send the notes after',
      pasted: 'Running ten minutes late, start without me. I’ll send the notes after.',
      steps: [
        'Put the cursor in a field and press the hotkey.',
        'Speak as you normally would. The words show up while you talk.',
        'Press it again: Sotto cleans the text and pastes it.',
      ],
      stepCount: 'Step {n} of {total}',
      processing: 'Processing',
      inserted: '{n} characters inserted',
      tune: {
        label: 'Overlay style',
        auto: 'In turn',
        looks: { pill: 'Pill', glow: 'Glow', caps: 'Captions', scope: 'Oscilloscope', mini: 'Mini', bead: 'Bead', term: 'Terminal' },
        textLabel: 'When the words show',
        textHint: 'Streaming models show the words while you speak',
        streaming: 'While speaking',
        after: 'After',
        paletteLabel: 'Colour',
        palettes: { graphite: 'Graphite', copper: 'Copper', lagoon: 'Lagoon', violet: 'Purple' },
        note: 'Each dictation in the demo shows another overlay style. Pick one to keep it.',
      },
    },
  },

  apps: {
    title: 'Works wherever you type',
    text: 'Sotto pastes into the field with the cursor: a messenger, mail, a document, a code editor or a browser. There are no plugins or integrations to set up.',
  },

  cleanup: {
    eyebrow: 'Cleanup',
    title: 'Say it as it comes. Get it clean.',
    text: 'Dictionaries fix terms and names, your rules replace words, and the ums and uhs disappear. All on your computer, no LLM involved.',
    replay: 'Play again',
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

  features: {
    eyebrow: 'Features',
    title: 'What Sotto does on your computer',
    listLabel: 'Sotto features',
    items: {
      voice: { name: 'Voice', text: 'Press the hotkey and speak. The words show up in the overlay while you talk.' },
      cleanup: { name: 'Cleanup', text: 'Dictionaries, your replacements and no more ums. All on your computer, without an LLM.' },
      models: { name: 'Models', text: 'Pick a local model for your language. Size and languages are visible before you download.' },
      profile: { name: 'LLM', text: 'A profile with your own prompt: a cloud model, or a local one in Ollama or LM Studio.' },
      history: { name: 'History', text: 'Everything you said stays on your computer. Find it, copy it or process it again.' },
      file: { name: 'File', text: 'Open a recording and a local model turns it into text. It is not added to history.' },
    },
    draft: 'the meeting moves to Thursday at three',
    models: { title: 'Speech models' },
    profile: {
      name: 'Local Ollama',
      model: 'llama3.1',
      address: 'http://localhost:11434/v1',
      prompt: 'Add punctuation and split long text into paragraphs. Do not add anything of your own.',
    },
    history: {
      title: 'History',
      entries: [
        ['Running ten minutes late, start without me.', '2:53 PM'],
        ['Hi Anna, sending the Q3 report.', '1:10 PM'],
        ['Add a retry to the upload function and cover it with a test.', '11:42 AM'],
      ],
    },
    file: {
      title: 'Transcribe an audio file',
      name: 'interview.m4a',
      done: 'done',
      result: 'At first we only needed fast input for notes during calls. Cloud services were ruled out by our data requirements, so everything had to run locally.',
      note: 'Stays in the panel: not added to history or pasted into a window.',
    },
  },

  models: {
    eyebrow: 'Models',
    title: 'A model for your language and your machine',
    description:
      'Russian, multiple languages or words as you speak. Pick a model and download it once.',
    filterLabel: 'Filter models',
    allModels: 'All models · {total}',
    filters: { all: 'All', ru: 'Русский', en: 'English', cjk: '中文, 日本語, 한국어' },
    streamingOnly: 'Streaming only',
    count: '{shown} of {total}',
    empty: 'No models match these filters.',
    streaming: 'streaming',
    punctuation: 'punctuation',
    note: 'Whisper sizes are approximate. Details:',
    providers: {
      title: 'Engines and providers',
      local: 'Recognition on your computer',
      llm: 'LLM formatting, optional',
      cloud: 'Cloud recognition, optional',
      compatible: 'any OpenAI-compatible API',
    },
  },

  privacy: {
    link: 'Privacy summary',
  },

  start: {
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
        answer: 'By default it stays on your computer. Audio is sent to the cloud only if you connect a cloud recognition provider yourself. De-identified usage statistics stay off until you allow them, and never include audio or text.',
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
      'If macOS will not open Sotto: System Settings → Privacy & Security → Open Anyway.',
      'Grant microphone and Accessibility permissions when prompted.',
      'Download a model, select it, and set your dictation shortcut.',
    ],
    stepsWindows: [
      'Run the x64 .exe installer, or unpack the portable ZIP.',
      'If “Windows protected your PC” appears: More info → Run anyway.',
      'Allow microphone access when prompted.',
      'Download a model, select it, and set your dictation shortcut.',
    ],
    notice:
      'Builds lack a publisher certificate yet, so the system warns about an unknown publisher. You can check the file against SHA256SUMS.txt from the same release.',
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
