import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync, spawn } from 'node:child_process';
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { userInfo, release } from 'node:os';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { WebDriver } from './webdriver.mjs';

export function requireSandbox(platform, username) {
  if (platform !== 'win32' || username !== 'WDAGUtilityAccount') {
    throw new Error('This launcher only runs in Windows Sandbox, never in the host account');
  }
}

export function verifyPayload(root, manifest) {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.kind, 'instrumented-native-experiment');
  for (const required of ['app/Sotto.exe', 'node.exe', 'webview/msedgewebview2.exe', 'probe.mjs', 'webdriver.mjs', 'bootstrap.ps1']) {
    assert.ok(manifest.files?.[required], `Missing manifest entry: ${required}`);
  }
  for (const [name, hash] of Object.entries(manifest.files)) {
    const path = resolve(root, name);
    const rel = relative(resolve(root), path);
    assert.ok(!isAbsolute(name) && rel && !rel.startsWith('..') && !isAbsolute(rel), 'Invalid manifest path');
    assert.match(hash, /^[a-f0-9]{64}$/);
    assert.equal(createHash('sha256').update(readFileSync(path)).digest('hex'), hash, `Payload changed: ${name}`);
  }
}

export function assertMainSnapshot(snapshot) {
  assert.equal(snapshot.settingsVisible, true, 'The settings UI is missing or blank');
  assert.equal(snapshot.theme, 'light', 'The seeded light theme was not loaded');
  assert.ok(snapshot.csp.some(policy => /script-src[^;]*'self'/.test(policy)), 'The app CSP is missing');
  assert.ok(snapshot.csp.every(policy => !policy.includes("'unsafe-eval'")), 'CSP must not be relaxed for the driver');
}

const snapshotScript = `
  const page = document.querySelector('[data-testid="page-settings"]');
  const rect = page?.getBoundingClientRect();
  return {
    settingsVisible: !!rect && rect.width > 0 && rect.height > 0 && getComputedStyle(page).visibility !== 'hidden',
    theme: document.documentElement.dataset.theme,
    csp: Array.from(document.querySelectorAll('meta[http-equiv]')).filter(x => x.httpEquiv.toLowerCase() === 'content-security-policy').map(x => x.content),
    runtime: navigator.userAgent
  };`;

async function until(action, milliseconds, child) {
  const deadline = Date.now() + milliseconds;
  let last;
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode !== null) throw new Error('Sotto exited before the probe finished');
    try { const value = await action(); if (value) return value; } catch (error) { last = error; }
    await delay(250);
  }
  throw last ?? new Error('Timed out waiting for application state');
}

async function run() {
  requireSandbox(process.platform, userInfo().username);
  const input = dirname(fileURLToPath(import.meta.url));
  const reports = 'C:\\SottoReports';
  assert.ok(existsSync(reports), 'Missing report mapping');
  const manifest = JSON.parse(readFileSync(join(input, 'manifest.json'), 'utf8'));
  verifyPayload(input, manifest);
  const active = execFileSync('powershell.exe', ['-NoProfile', '-Command', '(Get-Process -Name Sotto -ErrorAction SilentlyContinue | Measure-Object).Count'], { encoding: 'utf8', windowsHide: true }).trim();
  assert.equal(active, '0', 'Another Sotto process exists; refusing to attach or stop it');
  const work = 'C:\\SottoNativeExperiment';
  const configDir = join(process.env.APPDATA, 'com.sotto.app');
  assert.ok(!existsSync(work) && !existsSync(configDir), 'Use a fresh Sandbox; no existing app data will be overwritten');
  mkdirSync(work);
  mkdirSync(configDir);
  for (const name of ['data', 'logs', 'models']) mkdirSync(join(work, name));
  writeFileSync(join(configDir, 'config.json'), JSON.stringify({ telemetry_enabled: false, theme: 'light', ui_language: 'en', device: 'cpu', debug_save_recordings: false }), { flag: 'wx' });
  const report = {
    schema: 1, kind: 'instrumented-native-experiment', status: 'running', os: release(),
    binarySha256: manifest.files['app/Sotto.exe'], webviewRuntime: manifest.webviewRuntime,
    startedAt: new Date().toISOString(), checks: {},
    limitations: ['No microphone, GPU, paste or model inference test', 'No frontend error stream has been established', 'Hidden overlay discovery does not prove native visibility or focus', 'macOS and unmodified release packages are not tested'],
  };
  const save = () => writeFileSync(join(reports, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  save();
  const stdout = openSync(join(reports, 'stdout.log'), 'wx');
  const stderr = openSync(join(reports, 'stderr.log'), 'wx');
  const child = spawn(join(input, 'app/Sotto.exe'), [], {
    // This GUI is the subject of the test; only helper consoles are hidden.
    cwd: work, windowsHide: false, stdio: ['ignore', stdout, stderr],
    env: { ...process.env, SOTTO_NATIVE_E2E_PORT: '4445', SOTTO_CONFIG_DIR: join(work, 'data'), SPEECH_TO_TEXT_LOG_DIR: join(work, 'logs'), SPEECH_TO_TEXT_MODELS_DIR: join(work, 'models'), WEBVIEW2_BROWSER_EXECUTABLE_FOLDER: join(input, 'webview') },
  });
  closeSync(stdout);
  closeSync(stderr);
  let spawnError;
  child.on('error', error => { spawnError = error; });
  report.pid = child.pid;
  const driver = new WebDriver(4445);
  try {
    await until(async () => { if (spawnError) throw spawnError; return (await driver.request('GET', '/status'))?.ready; }, 30000, child);
    await driver.start();
    report.checks.driver = 'passed';
    const snapshot = await until(async () => {
      const value = await driver.read(snapshotScript);
      return value.settingsVisible ? value : null;
    }, 20000, child);
    // Tauri serves CSP in a response header, not necessarily in a meta tag.
    const header = await driver.readAsync(`
      const done = arguments[arguments.length - 1];
      fetch(location.href).then(response => done(response.headers.get('Content-Security-Policy')))
        .catch(error => done({ error: String(error) }));`);
    if (typeof header === 'string') snapshot.csp.push(header);
    assertMainSnapshot(snapshot);
    report.checks.main = { status: 'passed', ...snapshot };
    writeFileSync(join(reports, 'main.png'), Buffer.from(await driver.screenshot(), 'base64'));
    await driver.click('[data-testid="overlay-disclosure"] > summary');
    assert.equal(await driver.read('return document.querySelector(\'[data-testid="overlay-disclosure"]\').open;'), true);
    await driver.click('[data-testid="overlay-disclosure"] > summary');
    report.checks.userInteraction = 'passed';
    report.checks.overlay = 'not-observed';
    report.checks.trayPopup = 'not-observed';
    report.checks.gracefulExit = 'not-observed';
    // Native tray input remains an operator action during this feasibility spike.
    // Do not replace it with show_tray_popup or a synthetic application event.
    report.operatorAction = 'Within 120 seconds, left-click the Sotto tray icon, then right-click it and choose Exit.';
    save();
    const deadline = Date.now() + 120000;
    while (Date.now() < deadline && child.exitCode === null && child.signalCode === null) {
      const handles = await driver.handles();
      if (handles.includes('overlay') && report.checks.overlay === 'not-observed') {
        await driver.switchTo('overlay');
        const root = await driver.read('return !!document.querySelector("#root") && document.readyState === "complete";');
        if (root) report.checks.overlay = 'hidden-webview-readable; native display unverified';
      }
      if (handles.includes('tray-popup') && report.checks.trayPopup === 'not-observed') {
        await driver.switchTo('tray-popup');
        if (await driver.read('return !!document.querySelector(\'[role="menu"]\');')) {
          report.checks.trayPopup = 'webview-readable-after-native-action';
          writeFileSync(join(reports, 'tray.png'), Buffer.from(await driver.screenshot(), 'base64'));
        }
      }
      await driver.switchTo('main');
      save();
      await delay(500);
    }
    if (child.exitCode === 0 && child.signalCode === null) report.checks.gracefulExit = 'observed-zero-exit; operator must confirm Exit was used';
    report.status = 'partial';
  } catch (error) {
    // Exiting from the native menu can terminate an in-flight driver request.
    if (child.exitCode === 0 && report.checks.userInteraction === 'passed') {
      report.checks.gracefulExit = 'observed-zero-exit; operator must confirm Exit was used';
      report.status = 'partial';
    } else {
      report.status = 'failed';
      report.error = String(error.stack ?? error);
    }
  } finally {
    if (report.status === 'failed' && driver.session && child.exitCode === null && child.signalCode === null) {
      try {
        writeFileSync(join(reports, 'failure.png'), Buffer.from(await driver.screenshot(), 'base64'));
      } catch (error) {
        report.screenshotError = String(error.message);
      }
    }
    if (child.pid && child.exitCode === null && child.signalCode === null) {
      report.forcedCleanup = true;
      child.kill();
      await Promise.race([new Promise(resolveExit => child.once('exit', resolveExit)), delay(5000)]);
    }
    const log = join(work, 'logs', 'app.log');
    if (existsSync(log)) {
      writeFileSync(join(reports, 'app.log'), readFileSync(log));
      report.checks.backendLog = 'collected';
    } else report.checks.backendLog = 'not-observed';
    report.finishedAt = new Date().toISOString();
    save();
  }
  // A partial feasibility experiment must never masquerade as an E2E pass.
  process.exitCode = report.status === 'failed' ? 1 : 2;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  run().catch(error => { console.error(error); process.exitCode = 1; });
}
