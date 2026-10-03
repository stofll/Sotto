import type { ScreenId } from './product';

/** The steps of the feature tour, in the order a new user meets them. */
export const featureSteps = ['catalog', 'cleanup', 'profile', 'overlay', 'history', 'file'] as const;
export type FeatureStepId = typeof featureSteps[number];

/**
 * Regions of the 1088×736 captures that the screenshot steps zoom into.
 * Review them whenever the app's layout or `test_site_screenshots.py` changes.
 */
export const screenRegions = {
  models: { languages: [268, 120, 792, 43], resources: [268, 239, 391, 120], streaming: [268, 407, 391, 144] },
  history: { search: [268, 145, 792, 106], copy: [974, 306, 37, 35], formatting: [309, 350, 196, 32] },
} as const satisfies Record<ScreenId, Record<string, readonly [number, number, number, number]>>;
export type ScreenRegion<S extends ScreenId> = keyof typeof screenRegions[S];
export const CAPTURE = { width: 1088, height: 736 } as const;

/** The overlay card's quick templates, from `desktop/src/pages/overlayEditor/templates.ts`. */
export const overlayTemplates = ['pill', 'bead', 'glow', 'orb'] as const;
export type OverlayTemplate = typeof overlayTemplates[number];

/** Hue and OKLCH chroma of each preset, mirroring `desktop/src/overlay/overlayPalette.ts`. */
export const overlayPalettes = {
  graphite: [0, 0],
  copper: [55, 0.14],
  lagoon: [195, 0.1],
  violet: [295, 0.14],
} as const;
export type OverlayPalette = keyof typeof overlayPalettes;

/** States the demo walks through; the app's constructor previews the same phases. */
export const overlayPhases = ['rec', 'stream', 'proc', 'done'] as const;
export type OverlayPhase = typeof overlayPhases[number];

/** Formats the file panel accepts, mirroring `AUDIO_EXTENSIONS` in `desktop/src/pages/AiPage.tsx`. */
export const audioExtensions = ['wav', 'mp3', 'm4a', 'mp4', 'ogg', 'oga', 'opus', 'flac'];
