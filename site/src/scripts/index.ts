import { ASSET_PREFIX, LINKS, RELEASES_API, REPO } from '../data/product';
import { createRuntime, type Behaviour } from './runtime';
import { initClipboard } from './clipboard';
import { initDownload } from './download';
import { initHeader } from './header';
import { initHero } from './hero';
import { initModels } from './models';
import { initPrivacyDiagram } from './privacy';
import { initReveal } from './reveal';
import { initWorkflow } from './workflow';

/**
 * Progressive enhancement for markup Astro already rendered. Nothing here is
 * required to read the page: no microphone, no analytics, no third-party fonts.
 * The GitHub API is contacted only after an explicit download click.
 */
export const initLanding = (): (() => void) => {
  const abort = new AbortController();
  const { runtime, motion, setReduced, teardown } = createRuntime(abort);
  const { signal } = runtime;

  document.documentElement.classList.add('js-enabled');

  const behaviours: Behaviour[] = [
    initHeader(runtime),
    initReveal(runtime),
    initHero(runtime),
    initWorkflow(runtime),
    initModels(runtime),
    initPrivacyDiagram(runtime),
    initClipboard(runtime, REPO),
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
    document.documentElement.classList.remove('js-enabled');
    delete document.documentElement.dataset.pageHidden;
  };
};
