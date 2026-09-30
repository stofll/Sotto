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
  assert.ok(whisper.length > 0);
  for (const model of whisper) {
    const id = model.name.replace('Whisper ', '');
    const bytes = artifacts.get(id);
    assert.ok(bytes > 0, `Missing native artifact for ${id}`);
    const expected = bytes >= 1e9 ? `≈${(bytes / 1e9).toFixed(1)} GB` : `≈${Math.round(bytes / 1e6)} MB`;
    assert.equal(model.size, expected, `${id}: use the shipped artifact, including its quantization`);
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
