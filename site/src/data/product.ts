/**
 * Verifiable product facts. Everything here is language-independent: sizes,
 * identifiers and links. Anything a translator would touch lives in src/i18n.
 *
 * Languages and download sizes follow desktop/src-tauri/src/model.rs;
 * docs/models.md explains the engines and their constraints.
 */

export const REPO = 'https://github.com/stofll/Sotto';
export const AUTHOR = { name: 'Stofl', url: 'https://github.com/stofll' } as const;

/** The social card, one per locale, rendered by scripts/generate-og.mjs at this size. */
export const OG_IMAGE = { path: (locale: string) => `/og/sotto-${locale}.png`, width: 1200, height: 630 } as const;
export const RELEASES = `${REPO}/releases/latest`;
const DOCS = `${REPO}/blob/main/docs`;

export const LINKS = {
  github: REPO,
  releases: RELEASES,
  docs: `${DOCS}/README.md`,
  models: `${DOCS}/models.md`,
  privacy: `${DOCS}/privacy.md`,
  install: `${DOCS}/verifying-downloads.md`,
  license: `${REPO}/blob/main/LICENSE`,
  issues: `${REPO}/issues`,
} as const;

/** Release assets the download dialog is allowed to link to directly. */
export const ASSET_PREFIX = '/stofll/sotto/releases/download/';
export const RELEASES_API = 'https://api.github.com/repos/stofll/Sotto/releases/latest';

/** Language groups the model filter offers. `cjk` covers Chinese, Japanese and Korean. */
export type LanguageGroup = 'ru' | 'en' | 'cjk';

export interface Model {
  /** Product name. Not translated: it is what the app's catalog shows. */
  name: string;
  engine: string;
  /** Language tags as the card shows them; `+` marks a longer list. */
  languages: string[];
  groups: LanguageGroup[];
  /** Download size as the catalog states it; Whisper sizes are approximate. */
  size: string;
  /** Produces text while you speak; see docs/models.md. */
  streaming?: boolean;
  /** Punctuates and capitalises without the LLM step. */
  punctuation?: boolean;
}

const WHISPER = 'whisper.cpp';
const multilingualWhisper = (name: string, size: string): Model => ({
  name, engine: WHISPER, languages: ['99+'], groups: ['ru', 'en', 'cjk'], size,
});
const englishWhisper = (name: string, size: string): Model => ({
  name, engine: WHISPER, languages: ['en'], groups: ['en'], size,
});

/** The built-in catalog, in the order the page lists it. */
export const models: Model[] = [
  { name: 'GigaAM v3', engine: 'sherpa-onnx · NeMo CTC', languages: ['ru'], groups: ['ru'], size: '214 MB', punctuation: true },
  { name: 'Nemotron 3.5', engine: 'sherpa-onnx · transducer', languages: ['ru', 'en', 'zh', 'ja', 'ko', '+'], groups: ['ru', 'en', 'cjk'], size: '651 MB', streaming: true, punctuation: true },
  { name: 'Parakeet TDT v3', engine: 'sherpa-onnx · transducer', languages: ['ru', 'en', 'de', 'fr', '+'], groups: ['ru', 'en'], size: '639 MB' },
  multilingualWhisper('Whisper large-v3', '≈3.1 GB'),
  multilingualWhisper('Whisper turbo', '≈874 MB'),
  { name: 'Omnilingual 300M', engine: 'sherpa-onnx · Omnilingual CTC', languages: ['1600+'], groups: ['ru', 'en', 'cjk'], size: '348 MB' },
  { name: 'Zipformer', engine: 'sherpa-onnx · transducer', languages: ['ru'], groups: ['ru'], size: '70 MB' },
  { name: 'Zipformer small', engine: 'sherpa-onnx · transducer', languages: ['ru'], groups: ['ru'], size: '27 MB', streaming: true },
  { name: 'SenseVoice small', engine: 'sherpa-onnx · SenseVoice', languages: ['zh', 'en', 'ja', 'ko', 'yue'], groups: ['en', 'cjk'], size: '229 MB' },
  { name: 'Parakeet unified', engine: 'sherpa-onnx · transducer', languages: ['en'], groups: ['en'], size: '632 MB', streaming: true },
  { name: 'Parakeet TDT v2', engine: 'sherpa-onnx · transducer', languages: ['en'], groups: ['en'], size: '631 MB' },
  { name: 'Canary 180M Flash', engine: 'sherpa-onnx · Canary', languages: ['en'], groups: ['en'], size: '198 MB' },
  { name: 'Moonshine base', engine: 'sherpa-onnx · Moonshine', languages: ['en'], groups: ['en'], size: '274 MB' },
  multilingualWhisper('Whisper medium', '≈823 MB'),
  multilingualWhisper('Whisper small', '≈488 MB'),
  multilingualWhisper('Whisper base', '≈148 MB'),
  multilingualWhisper('Whisper tiny', '≈78 MB'),
  englishWhisper('Whisper medium.en', '≈823 MB'),
  englishWhisper('Whisper small.en', '≈264 MB'),
  englishWhisper('Whisper base.en', '≈82 MB'),
  englishWhisper('Whisper tiny.en', '≈44 MB'),
];

/** LLM providers the formatting step ships presets for. */
export const llmProviders = ['OpenAI', 'Anthropic', 'Gemini', 'Ollama', 'LM Studio'];

/** Screens of the app's settings window, captured for the site in both locales and themes. */
export type ScreenId = 'settings' | 'models' | 'history';
export const screens: ScreenId[] = ['settings', 'models', 'history'];
