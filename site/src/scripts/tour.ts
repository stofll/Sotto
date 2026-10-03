import { CAPTURE } from '../data/feature-tour';
import { createFileDemo } from './file-demo';
import { createOverlayDemo } from './overlay-demo';
import type { Behaviour, Runtime } from './runtime';

/** A scene that animates only while it is selected and can be seen. */
export interface SceneDemo {
  run: (on: boolean) => void;
  destroy: () => void;
}

/** The largest zoom into a screenshot region; the 2× capture stays sharp up to here. */
const MAX_ZOOM = 2;
/** Room left around a zoomed region, in capture pixels. */
const ZOOM_PADDING = 64;

/**
 * The feature tour: a vertical tab list beside one scene at a time. Steps
 * change only on request. Animated scenes run while selected, on screen, in a
 * visible tab and with motion allowed; otherwise they hold a still frame.
 */
export const initTour = (runtime: Runtime): Behaviour => {
  const { query, all, signal, observe, reduced } = runtime;
  const root = query('[data-tour]');
  const tabs = all<HTMLButtonElement>('[data-tour] [role="tab"]');
  const scenes = all<HTMLElement>('[data-tour] [role="tabpanel"]');
  if (!root || !tabs.length) return {};

  const demos = new Map<HTMLElement, SceneDemo>();
  const overlay = scenes.find((scene) => scene.dataset.scene === 'overlay');
  const file = scenes.find((scene) => scene.dataset.scene === 'file');
  if (overlay) demos.set(overlay, createOverlayDemo(overlay, runtime));
  if (file) demos.set(file, createFileDemo(file, runtime));

  let current = 0;
  let visible = false;
  let hidden = document.hidden;

  const update = () => {
    const live = visible && !hidden && !reduced();
    demos.forEach((demo, scene) => demo.run(live && scene === scenes[current]));
  };

  const select = (index: number, focus = false) => {
    current = index;
    tabs.forEach((tab, position) => {
      const selected = position === index;
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
    });
    scenes.forEach((scene, position) => {
      const selected = position === index;
      scene.inert = !selected;
      scene.classList.toggle('is-active', selected);
      if (selected) scene.removeAttribute('aria-hidden');
      else scene.setAttribute('aria-hidden', 'true');
    });
    if (focus) tabs[index]!.focus();
    update();
  };

  tabs.forEach((tab, index) => tab.addEventListener('click', () => select(index), { signal }));
  query('[data-tour] [role="tablist"]')?.addEventListener('keydown', (event) => {
    // The list stands vertically beside the scenes and lies flat above them on phones.
    const moves: Record<string, number> = {
      ArrowDown: current + 1, ArrowRight: current + 1, ArrowUp: current - 1, ArrowLeft: current - 1, Home: 0, End: tabs.length - 1,
    };
    const target = moves[event.key];
    if (target === undefined) return;
    event.preventDefault();
    select((target + tabs.length) % tabs.length, true);
  }, { signal });

  // Screenshot scenes zoom into the region named by a pressed detail.
  const shots = scenes.filter((scene) => scene.querySelector('[data-shot]'));
  const zoom = (scene: HTMLElement) => {
    const stage = scene.querySelector<HTMLElement>('[data-shot]')!;
    const layer = scene.querySelector<HTMLElement>('[data-shot-layer]')!;
    const pressed = scene.querySelector<HTMLButtonElement>('[data-region][aria-pressed="true"]');
    scene.querySelectorAll<HTMLElement>('[data-shot-ring]').forEach((ring) => {
      ring.classList.toggle('is-on', ring.dataset.shotRing === pressed?.dataset.region);
    });
    stage.classList.toggle('is-zoomed', Boolean(pressed));
    const ring = pressed && scene.querySelector<HTMLElement>(`[data-shot-ring="${pressed.dataset.region}"]`);
    if (!ring || !stage.clientWidth) { layer.style.transform = ''; return; }

    const [x, y, width, height] = ring.dataset.rect!.split(' ').map(Number) as [number, number, number, number];
    const unit = stage.clientWidth / CAPTURE.width;
    const scale = Math.max(1, Math.min(MAX_ZOOM,
      stage.clientWidth / ((width + ZOOM_PADDING * 2) * unit),
      stage.clientHeight / ((height + ZOOM_PADDING * 2) * unit)));
    // Centre the region, but never pull the capture's edge inside the frame.
    const clamp = (value: number, size: number) => Math.min(0, Math.max(size - size * scale, value));
    const left = clamp(stage.clientWidth / 2 - (x + width / 2) * unit * scale, stage.clientWidth);
    const top = clamp(stage.clientHeight / 2 - (y + height / 2) * unit * scale, stage.clientHeight);
    layer.style.transform = `translate(${left}px, ${top}px) scale(${scale})`;
  };
  shots.forEach((scene) => {
    const details = Array.from(scene.querySelectorAll<HTMLButtonElement>('[data-region]'));
    details.forEach((detail) => detail.addEventListener('click', () => {
      const on = detail.getAttribute('aria-pressed') !== 'true';
      details.forEach((other) => other.setAttribute('aria-pressed', String(on && other === detail)));
      zoom(scene);
    }, { signal }));
  });
  window.addEventListener('resize', () => shots.forEach(zoom), { passive: true, signal });

  observe(new IntersectionObserver(([entry]) => { visible = Boolean(entry?.isIntersecting); update(); }, { threshold: 0.2 })).observe(root);
  root.classList.add('is-enhanced');
  select(0);

  return {
    onVisibilityChange: (value) => { hidden = value; update(); },
    onMotionChange: update,
    destroy: () => {
      demos.forEach((demo) => demo.destroy());
      root.classList.remove('is-enhanced');
      scenes.forEach((scene) => { scene.inert = false; scene.removeAttribute('aria-hidden'); scene.classList.remove('is-active'); });
    },
  };
};
