import { h, render } from 'preact';
import { OverlayScene } from '../../../../desktop/src/overlay/OverlayScene';
import { paletteTone, paletteVariables } from '../../../../desktop/src/overlay/overlayPalette';
import type { Recipe, ScenePhase } from '../../../../desktop/src/overlay/overlayRecipe';
import { SYSTEM_TEMPLATES } from '../../../../desktop/src/pages/overlayEditor/templates';
import { silentVoice, simulatedVoice } from '../../../../desktop/src/pages/overlayEditor/simulatedVoice';

/**
 * The app's own overlay on the hero's desktop, built from desktop/src/overlay
 * with Preact standing in for React (astro.config.mjs). The page keeps the
 * clock; this only draws the frame it is given.
 */

/** The page shows every look with a bare shell, as the app's defaults are, including those whose template draws a hairline. */
const bare = (recipe: Recipe): Recipe => ({ ...recipe, style: { ...recipe.style, stroke: 'none' } });
/** "Mini" with the oscilloscope: a look people run, and one without a draft. */
const miniScope: Recipe = { ...SYSTEM_TEMPLATES.mini, draw: { ...SYSTEM_TEMPLATES.mini.draw, level: 'scope' } };
export const LOOKS = {
  pill: bare(SYSTEM_TEMPLATES.pill),
  glow: bare(SYSTEM_TEMPLATES.glow),
  caps: bare(SYSTEM_TEMPLATES.caps),
  scope: bare(SYSTEM_TEMPLATES.scope),
  mini: bare(miniScope),
  bead: bare(SYSTEM_TEMPLATES.bead),
  term: bare(SYSTEM_TEMPLATES.term),
} satisfies Record<string, Recipe>;
export type Look = keyof typeof LOOKS;
export type Palette = 'graphite' | 'copper' | 'lagoon' | 'violet';

export interface OverlayFrame {
  phase: ScenePhase;
  shown: boolean;
  /** Words so far, when the model streams. */
  draft: string;
  timer: string;
  /** The processing or pasted note. */
  status: string;
  streaming: boolean;
  look: Look;
  palette: Palette;
  /** Counts dictations, so each starts from a fresh overlay as in the app. */
  take: number;
  /** False holds a still picture: the level rests. */
  live: boolean;
}

/** The desk is a scaled-down screen, so the overlay takes the app's smallest size, S. */
const SIZE = 's';
/** The streaming card's width at size S; a narrower desk scales the overlay down further. */
const WIDEST = 520;

export const mountOverlay = (host: HTMLElement) => {
  const desk = host.parentElement;
  const resize = new ResizeObserver(() => {
    const width = desk?.clientWidth ?? WIDEST;
    host.style.setProperty('--ovs-scale', String(Math.min(1, (width - 24) / WIDEST)));
  });
  if (desk) resize.observe(desk);

  const draw = (frame: OverlayFrame) => {
    const { palette_hue, palette_chroma } = paletteTone({ palette: frame.palette, palette_hue: 0, palette_chroma: 0 });
    render(
      h('div', { class: 'desk-ovs-tone', style: paletteVariables(palette_hue, palette_chroma) as Record<string, string> },
        h(OverlayScene, {
          key: `${frame.look}-${frame.take}`,
          recipe: LOOKS[frame.look],
          size: SIZE,
          phase: frame.phase,
          shown: frame.shown,
          streaming: frame.streaming,
          draft: frame.streaming ? frame.draft : '',
          draftPlaceholder: '',
          timer: frame.timer,
          limited: false,
          status: frame.status,
          mode: { full: '', short: '' },
          source: frame.live && frame.phase === 'recording' ? simulatedVoice : silentVoice,
          close: { label: '', text: '' },
        })),
      host,
    );
  };

  return {
    draw,
    /** Unmounts the scene, so a look with its own loop (Glow) stops drawing off-screen. */
    clear: () => render(null, host),
    destroy: () => {
      resize.disconnect();
      render(null, host);
    },
  };
};
