import { ASSET_PREFIX, LINKS, RELEASES_API } from '../data/product';
import { createRuntime, type Behaviour } from './runtime';
import { initDownload } from './download';
import { initFaq } from './faq';
import { initHeader } from './header';
import { initModels } from './models';
import { initModes } from './modes';
import { initPlatform } from './platform';
import { initReveal } from './reveal';
import { initScreens } from './screens';
import { initVoice } from './voice';

/**
 * Progressive enhancement for markup Astro already rendered. Nothing here is
 * required to read the page: no microphone, no analytics, no third-party fonts.
 * The GitHub API is contacted only after an explicit download click.
 */
export const initLanding = (): (() => void) => {
  const abort = new AbortController();
  const { runtime, motion, setReduced, teardown } = createRuntime(abort);
  const { signal } = runtime;

  const behaviours: Behaviour[] = [
    initHeader(runtime),
    initFaq(runtime),
    initReveal(runtime),
    initVoice(runtime),
    initModes(runtime),
    initScreens(runtime),
    initModels(runtime),
    // Before the dialog, which binds the links this retargets.
    initPlatform(runtime),
    initDownload(runtime, { api: RELEASES_API, assetPrefix: ASSET_PREFIX, releases: LINKS.releases }),
  ];

  // The two page-wide signals live here so each module does not register its
  // own listener for them.
  document.addEventListener(
    'visibilitychange',
    () => {
      document.documentElement.dataset.pageHidden = String(document.hidden);
      behaviours.forEach((behaviour) => behaviour.onVisibilityChange?.(document.hidden));
    },
    { signal },
  );

  motion.addEventListener(
    'change',
    (event) => {
      setReduced(event.matches);
      behaviours.forEach((behaviour) => behaviour.onMotionChange?.(event.matches));
    },
    { signal },
  );

  return () => {
    behaviours.forEach((behaviour) => behaviour.destroy?.());
    teardown();
    delete document.documentElement.dataset.pageHidden;
  };
};
