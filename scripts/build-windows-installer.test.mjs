import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// Test orchestration without compiling, downloading or reading signing keys.
for (const scenario of ['clean', 'missing-cached-dlls', 'failed-build']) {
  test(`installer prepares verified native runtime: ${scenario}`, { skip: process.platform !== 'win32' }, (t) => {
    const tempRoot = resolve(tmpdir());
    const fixture = mkdtempSync(join(tempRoot, 'sotto-installer-test-'));
    t.after(() => {
      assert.equal(dirname(fixture), tempRoot);
      rmSync(fixture, { recursive: true, force: true });
    });
    const scripts = join(fixture, 'scripts');
    const tauri = join(fixture, 'desktop', 'src-tauri');
    mkdirSync(scripts);
    mkdirSync(tauri, { recursive: true });
    copyFileSync(fileURLToPath(new URL('./build-windows-installer.ps1', import.meta.url)), join(scripts, 'build-windows-installer.ps1'));
    writeFileSync(join(scripts, 'fetch-sherpa-runtime.ps1'), `
param([string] $Target)
if ($Target -ne 'win-x64-shared') { throw 'Wrong runtime target' }
Add-Content -LiteralPath $env:SOTTO_TEST_LOG -Value 'fetch-and-verify'
Join-Path $env:SOTTO_TEST_FIXTURE 'verified-runtime'
`);
    writeFileSync(join(tauri, 'prepare-native-libs.ps1'), `
param([string] $BuildProfile)
if ($BuildProfile -ne 'release') { throw 'Wrong staging profile' }
foreach ($name in @('onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $env:CARGO_TARGET_DIR "release/$name"))) { throw "Missing $name" }
}
Add-Content -LiteralPath $env:SOTTO_TEST_LOG -Value 'stage'
`);
    const harness = join(fixture, 'harness.ps1');
    writeFileSync(harness, `
$ErrorActionPreference = 'Stop'
$global:buildCount = 0
function cargo {
    Add-Content -LiteralPath $env:SOTTO_TEST_LOG -Value ('cargo ' + ($args -join ' '))
    if ($env:SHERPA_ONNX_LIB_DIR -ne (Join-Path $env:SOTTO_TEST_FIXTURE 'verified-runtime')) { throw 'Unverified runtime' }
    $global:LASTEXITCODE = 0
    if ($args[0] -eq 'build') {
        $global:buildCount++
        if ($env:SOTTO_TEST_SCENARIO -eq 'failed-build') { $global:LASTEXITCODE = 1; return }
        if ($env:SOTTO_TEST_SCENARIO -eq 'missing-cached-dlls' -and $global:buildCount -eq 1) { return }
        $destination = Join-Path $env:CARGO_TARGET_DIR 'release'
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        foreach ($name in @('onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll')) {
            Set-Content -LiteralPath (Join-Path $destination $name) -Value 'synthetic DLL'
        }
    }
}
function pnpm {
    if (-not [Environment]::GetEnvironmentVariables().Contains('TAURI_SIGNING_PRIVATE_KEY_PASSWORD')) {
        throw 'The inherited empty signing password must remain present'
    }
    Add-Content -LiteralPath $env:SOTTO_TEST_LOG -Value ('pnpm ' + ($args -join ' '))
    $global:LASTEXITCODE = 0
}
& (Join-Path $env:SOTTO_TEST_FIXTURE 'scripts/build-windows-installer.ps1')
`);
    const log = join(fixture, 'calls.txt');
    const result = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', harness], {
      encoding: 'utf8',
      env: {
        ...process.env,
        CARGO_TARGET_DIR: join(fixture, 'target'), CARGO_BUILD_TARGET: '',
        TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '',
        SOTTO_TEST_FIXTURE: fixture, SOTTO_TEST_LOG: log, SOTTO_TEST_SCENARIO: scenario,
      },
    });
    const calls = readFileSync(log, 'utf8').trim().split(/\r?\n/);
    const expected = ['fetch-and-verify', 'cargo build --release --locked -p sherpa-onnx-sys'];
    if (scenario === 'missing-cached-dlls') {
      expected.push('cargo clean --release -p sherpa-onnx-sys', 'cargo build --release --locked -p sherpa-onnx-sys');
    }
    if (scenario !== 'failed-build') {
      expected.push('stage', 'pnpm --dir .. exec tauri build --features gpu-vulkan -- --locked');
      assert.equal(result.status, 0, result.stderr);
    } else assert.notEqual(result.status, 0);
    assert.deepEqual(calls, expected);
  });
}
