import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { createServer } from 'node:http';
import { once } from 'node:events';
import test from 'node:test';
import { createSandbox, marker, sandboxXml } from './package-sandbox.mjs';
import { assertMainSnapshot, requireSandbox, verifyPayload } from './probe.mjs';
import { WebDriver } from './webdriver.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'sotto-native-test-'));
  t.after(() => {
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    assert.ok(root.includes('sotto-native-test-'));
    rmSync(root, { recursive: true, force: true });
  });
  const binaryDir = join(root, 'bin');
  const webviewDir = join(root, 'runtime');
  const crtDir = join(root, 'crt');
  mkdirSync(binaryDir);
  mkdirSync(webviewDir);
  mkdirSync(crtDir);
  for (const file of ['msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll']) writeFileSync(join(crtDir, file), 'synthetic');
  writeFileSync(join(binaryDir, 'Sotto.exe'), marker);
  for (const file of ['onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll']) writeFileSync(join(binaryDir, file), 'synthetic');
  writeFileSync(join(webviewDir, 'msedgewebview2.exe'), 'synthetic');
  const nodeExe = join(root, 'node.exe');
  writeFileSync(nodeExe, 'synthetic');
  return { binaryDir, webviewDir, crtDir, nodeExe, output: join(root, 'new-run') };
}

test('sandbox mappings expose only staged input and a dedicated report directory', () => {
  const xml = sandboxXml('D:\\test & build\\input', 'D:\\test & build\\reports');
  assert.ok(xml.includes('D:\\test &amp; build\\input'));
  assert.equal((xml.match(/<MappedFolder>/g) ?? []).length, 2);
  assert.equal((xml.match(/<ReadOnly>true<\/ReadOnly>/g) ?? []).length, 1);
  for (const setting of ['Networking', 'ClipboardRedirection', 'AudioInput', 'VideoInput', 'PrinterRedirection']) assert.ok(xml.includes(`<${setting}>Disable</${setting}>`));
});

test('packaging hashes its staged files and refuses to overwrite a previous run', (t) => {
  const options = fixture(t);
  const { manifest, configuration } = createSandbox(options);
  verifyPayload(join(options.output, 'input'), manifest);
  assert.ok(readFileSync(configuration, 'utf8').includes('<LogonCommand>'));
  assert.throws(() => createSandbox(options), /never overwritten/);
});

test('a production executable cannot be packaged as an instrumented experiment', (t) => {
  const options = fixture(t);
  writeFileSync(join(options.binaryDir, 'Sotto.exe'), 'ordinary build');
  assert.throws(() => createSandbox(options), /not an instrumented/);
});

test('portable inputs are refused instead of redirecting configuration unexpectedly', (t) => {
  const options = fixture(t);
  writeFileSync(join(options.binaryDir, 'portable.flag'), '');
  assert.throws(() => createSandbox(options), /non-portable/);
});

test('output inside the runtime is refused before a recursive copy', (t) => {
  const options = fixture(t);
  options.output = join(options.webviewDir, 'nested-output');
  assert.throws(() => createSandbox(options), /outside the runtime/);
});

test('changed runtime files fail verification before app startup', (t) => {
  const options = fixture(t);
  const { manifest } = createSandbox(options);
  const root = join(options.output, 'input');
  writeFileSync(join(root, 'webview/msedgewebview2.exe'), 'changed');
  assert.throws(() => verifyPayload(root, manifest), /Payload changed/);
});

test('manifest paths cannot escape the staged input', (t) => {
  const options = fixture(t);
  const { manifest } = createSandbox(options);
  manifest.files['../outside'] = '0'.repeat(64);
  assert.throws(() => verifyPayload(join(options.output, 'input'), manifest), /Invalid manifest path/);
});

test('launching from an ordinary host account is refused', () => {
  assert.throws(() => requireSandbox('win32', 'developer'), /never in the host account/);
  assert.throws(() => requireSandbox('darwin', 'WDAGUtilityAccount'), /only runs in Windows Sandbox/);
  assert.doesNotThrow(() => requireSandbox('win32', 'WDAGUtilityAccount'));
});

test('blank settings, wrong theme and relaxed CSP fail even if the process is alive', () => {
  const healthy = { settingsVisible: true, theme: 'light', csp: ["default-src 'self'; script-src 'self'"] };
  assert.doesNotThrow(() => assertMainSnapshot(healthy));
  assert.throws(() => assertMainSnapshot({ ...healthy, settingsVisible: false }), /blank/);
  assert.throws(() => assertMainSnapshot({ ...healthy, theme: 'dark' }), /light theme/);
  assert.throws(() => assertMainSnapshot({ ...healthy, csp: [] }), /CSP is missing/);
  assert.throws(() => assertMainSnapshot({ ...healthy, csp: ["script-src 'self' 'unsafe-eval'"] }), /must not be relaxed/);
});

test('driver protocol errors cannot be treated as successful observations', async (t) => {
  const server = createServer((request, response) => {
    response.setHeader('Content-Type', 'application/json');
    response.end(JSON.stringify({ value: { error: 'javascript error', message: 'Blocked by CSP' } }));
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => server.close());
  const driver = new WebDriver(server.address().port);
  await assert.rejects(driver.start(), /Blocked by CSP/);
  assert.equal(driver.session, null);
});
