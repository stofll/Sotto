/**
 * Verifiable product facts. Everything here is language-independent: sizes,
 * identifiers and links. Anything a translator would touch lives in src/i18n.
 *
 * Model figures are taken from docs/models.md in the Sotto repository. Change
 * them there first, then here.
 */

export const REPO = 'https://github.com/stofll/Sotto';
export const AUTHOR = { name: 'Stofl', url: 'https://github.com/stofll' } as const;

/** The social card. Dimensions mirror src/assets/og.svg, which renders it. */
export const OG_IMAGE = { path: '/og/sotto.png', width: 1200, height: 630 } as const;
export const RELEASES = `${REPO}/releases/latest`;
const DOCS = `${REPO}/blob/main/docs`;

export const LINKS = {
  github: REPO,
  releases: RELEASES,
  docs: `${DOCS}/README.md`,
  models: `${DOCS}/models.md`,
  privacy: `${DOCS}/privacy.md`,
  platforms: `${DOCS}/platforms.md`,
  install: `${DOCS}/verifying-downloads.md`,
  contributing: `${REPO}/blob/main/CONTRIBUTING.md`,
  license: `${REPO}/blob/main/LICENSE`,
  issues: `${REPO}/issues`,
} as const;

/** Release assets the download dialog is allowed to link to directly. */
export const ASSET_PREFIX = '/stofll/sotto/releases/download/';
export const RELEASES_API = 'https://api.github.com/repos/stofll/Sotto/releases/latest';

export type ModelId = 'nemotron' | 'whisper' | 'gigaam' | 'parakeet';

export interface Model {
  id: ModelId;
  /** Product name. Not translated: it is what the app's catalog shows. */
  name: string;
  /** Monogram in the catalog row. */
  letter: string;
  /** Download size in megabytes; omitted where the family ships several sizes. */
  sizeMb?: number;
  /** Which language key the dictionary should use for this row. */
  languages: 'multilingual' | 'russian' | 'sizes';
  /** Produces text while you speak; see docs/models.md. */
  streaming: boolean;
}

/** Non-empty by contract: the catalog markup takes the first row as selected. */
export const models: [Model, ...Model[]] = [
  { id: 'nemotron', name: 'Nemotron 3.5', letter: 'N', sizeMb: 651, languages: 'multilingual', streaming: true },
  { id: 'whisper', name: 'Whisper', letter: 'W', languages: 'sizes', streaming: false },
  { id: 'gigaam', name: 'GigaAM v3', letter: 'G', sizeMb: 214, languages: 'russian', streaming: false },
  { id: 'parakeet', name: 'Parakeet TDT v3', letter: 'P', sizeMb: 639, languages: 'multilingual', streaming: false },
];

export type WorkflowId = 'messages' | 'notes' | 'code';

export interface Workflow {
  id: WorkflowId;
  icon: 'message' | 'note' | 'code';
}

/** Non-empty by contract: the first tab is the one selected on load. */
export const workflows: [Workflow, ...Workflow[]] = [
  { id: 'messages', icon: 'message' },
  { id: 'notes', icon: 'note' },
  { id: 'code', icon: 'code' },
];

/**
 * Waveform bars. Generated rather than written out so the hero and the compact
 * workflow overlay cannot drift apart, as they had in the single-file draft.
 */
export const waveformBars = Array.from({ length: 26 }, (_, index) => ({
  height: [9, 16, 24, 13, 35, 47, 26, 56, 38, 64, 43, 29, 58, 73, 45, 61, 33, 50, 27, 41, 19, 30, 14, 22, 11, 6][index],
  speed: 660 + (index % 6) * 97 + Math.floor(index / 6) * 12,
  delay: -137 * index,
}));
