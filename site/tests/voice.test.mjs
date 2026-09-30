import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../src/scripts/voice.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText;
const { initVoice } = await import(`data:text/javascript,${encodeURIComponent(compiled)}`);

test('voice animation draws only visible canvases and repaints on entry or paused resize', () => {
  const original = {
    document: globalThis.document,
    window: globalThis.window,
    IntersectionObserver: globalThis.IntersectionObserver,
    requestAnimationFrame: globalThis.requestAnimationFrame,
    cancelAnimationFrame: globalThis.cancelAnimationFrame,
  };
  const paints = [0, 0];
  const canvases = paints.map((_, index) => {
    const context = {
      setTransform() {},
      clearRect() { paints[index] += 1; },
      beginPath() {}, rect() {}, roundRect() {}, fill() {},
    };
    return {
      clientWidth: 90, clientHeight: 30, width: 0, height: 0,
      dataset: { voiceWave: 'bars' },
      getContext: () => context,
    };
  });
  const text = { textContent: '' };
  const frames = new Map();
  const listeners = new Map();
  let observer;
  let reduced = false;
  let nextFrame = 1;
  try {
    globalThis.document = { hidden: false };
    globalThis.window = {
      devicePixelRatio: 1,
      addEventListener: (name, callback) => listeners.set(name, callback),
    };
    globalThis.IntersectionObserver = class {
      constructor(callback) { this.callback = callback; observer = this; }
      observe() {}
    };
    globalThis.requestAnimationFrame = (callback) => {
      const id = nextFrame++;
      frames.set(id, callback);
      return id;
    };
    globalThis.cancelAnimationFrame = (id) => frames.delete(id);

    const voice = initVoice({
      all: (selector) => selector.startsWith('canvas') ? canvases : [text],
      signal: new AbortController().signal,
      observe: (value) => value,
      reduced: () => reduced,
      strings: { phrases: ['Test phrase'] },
    });
    const enter = (index) => observer.callback([{ target: canvases[index], isIntersecting: true }]);
    const leave = (index) => observer.callback([{ target: canvases[index], isIntersecting: false }]);
    const tick = () => {
      const [id, callback] = frames.entries().next().value;
      frames.delete(id);
      callback(1000);
    };

    enter(0);
    assert.deepEqual(paints, [1, 0]);
    tick();
    assert.deepEqual(paints, [2, 0]);
    enter(1);
    assert.deepEqual(paints, [2, 1]);
    tick();
    assert.deepEqual(paints, [3, 2]);
    leave(0);
    tick();
    assert.deepEqual(paints, [3, 3]);

    voice.onVisibilityChange(true);
    assert.equal(frames.size, 0);
    voice.onVisibilityChange(false);
    assert.equal(frames.size, 1);

    reduced = true;
    voice.onMotionChange(true);
    assert.equal(frames.size, 0);
    assert.deepEqual(paints, [3, 4]);
    listeners.get('resize')();
    assert.deepEqual(paints, [3, 5]);
    enter(0);
    assert.deepEqual(paints, [4, 5]);
    voice.destroy();
  } finally {
    for (const [key, value] of Object.entries(original)) globalThis[key] = value;
  }
});
