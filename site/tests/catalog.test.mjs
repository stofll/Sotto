import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { models } from '../src/data/product.ts';

const native = await readFile(new URL('../../desktop/src-tauri/src/model.rs', import.meta.url), 'utf8');

test('Whisper download sizes match the artifacts the application downloads', () => {
  const manifest = native.split('pub const MODEL_MANIFEST:')[1]?.split('\n];')[0];
  assert.ok(manifest, 'Native Whisper manifest is missing');
  const artifacts = new Map([...manifest.matchAll(/public_id: "([^"]+)"[\s\S]*?expected_bytes: ([\d_]+)/g)]
    .map(([, id, bytes]) => [id, Number(bytes.replaceAll('_', ''))]));
  const whisper = models.filter((model) => model.engine === 'whisper.cpp');
  assert.deepEqual(whisper.map((model) => model.name.replace('Whisper ', '')).sort(), [...artifacts.keys()].sort());
  for (const model of whisper) {
    const id = model.name.replace('Whisper ', '');
    const bytes = artifacts.get(id);
    assert.ok(bytes > 0, `Missing native artifact for ${id}`);
    const expected = bytes >= 1e9 ? `≈${(bytes / 1e9).toFixed(1)} GB` : `≈${Math.round(bytes / 1e6)} MB`;
    assert.equal(model.size, expected, `${id}: use the shipped artifact, including its quantization`);
  }
});

test('Bundle models match the native catalog: names, sizes, languages and streaming', () => {
  // The first manifest is the Windows/macOS one; the other platforms ship none.
  const manifest = native.split('pub const BUNDLE_MODEL_MANIFEST:')[1]?.split('\n];')[0];
  assert.ok(manifest, 'Native bundle manifest is missing');
  const constant = (name) => native.match(new RegExp(`const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\];`))?.[1];
  const codes = (list) => [...list.matchAll(/"([^"]+)"/g)].map(([, code]) => code);
  const entries = manifest.split('BundleModelManifestEntry {').slice(1).map((entry) => {
    const field = (name) => entry.match(new RegExp(`\\b${name}: ([^,\\n]+),`))?.[1];
    const languages = entry.match(/\blanguages: (None|Some\((.*)\)),/)?.[2];
    const list = languages === undefined ? null : languages.match(/^&\[(.*)\]$/)?.[1] ?? constant(languages);
    return {
      label: field('label').slice(1, -1),
      size: field('size').slice(1, -1),
      streaming: field('engine') === 'ModelEngine::SherpaStreamingTransducer',
      languages: list === null ? null : codes(list),
    };
  });
  const bundles = models.filter((model) => model.engine !== 'whisper.cpp');
  assert.deepEqual(bundles.map((model) => model.name).sort(), entries.map((entry) => entry.label).sort());
  for (const entry of entries) {
    const model = bundles.find((item) => item.name === entry.label);
    assert.equal(model.size, entry.size, `${entry.label}: size`);
    assert.equal(Boolean(model.streaming), entry.streaming, `${entry.label}: streaming`);
    if (entry.languages === null) {
      assert.match(model.languages.join(), /^\d+\+$/, `${entry.label}: an open language list shows a count`);
      continue;
    }
    // A trailing "+" stands for the languages the card leaves out.
    const shown = model.languages.filter((code) => code !== '+');
    for (const code of shown) assert.ok(entry.languages.includes(code), `${entry.label}: ${code} is not in the native list`);
    assert.equal(model.languages.includes('+'), shown.length < entry.languages.length, `${entry.label}: languages`);
  }
});

test('CJK streaming search includes Nemotron, consistent with its native language list', () => {
  const languages = native.match(/const NEMOTRON_LANGUAGES[^=]*= &\[([\s\S]*?)\];/)?.[1];
  assert.ok(languages, 'Native Nemotron language list is missing');
  const cjkLanguages = ['zh', 'ja', 'ko'].filter((language) => languages.includes(`"${language}"`));
  assert.ok(cjkLanguages.length > 0);
  const matching = models.filter((model) => model.groups.includes('cjk') && model.streaming);
  const nemotron = matching.find((model) => model.name === 'Nemotron 3.5');
  assert.ok(nemotron, 'CJK + streaming must not hide Nemotron');
  for (const language of cjkLanguages) assert.ok(nemotron.languages.includes(language));
});
