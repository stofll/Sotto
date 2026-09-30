import type { ScreenId } from './product';

/** Regions in the dedicated 1088×736 captures, shared by both themes/locales. */
export const tourSteps = [
  { id: 'shortcut', screen: 'settings', rect: [280, 135, 246, 62] },
  { id: 'recording', screen: 'settings', rect: [676, 135, 254, 62] },
  { id: 'microphone', screen: 'settings', rect: [280, 319, 767, 64] },
  { id: 'languages', screen: 'models', rect: [268, 120, 792, 43] },
  { id: 'resources', screen: 'models', rect: [268, 239, 391, 120] },
  { id: 'streaming', screen: 'models', rect: [268, 407, 391, 144] },
  { id: 'search', screen: 'history', rect: [268, 145, 792, 106] },
  { id: 'copy', screen: 'history', rect: [974, 306, 37, 35] },
  { id: 'formatting', screen: 'history', rect: [309, 350, 196, 32] },
] as const satisfies readonly { id: string; screen: ScreenId; rect: readonly number[] }[];

export type TourStepId = typeof tourSteps[number]['id'];
