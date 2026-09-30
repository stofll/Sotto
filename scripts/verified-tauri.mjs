#!/usr/bin/env node
// Prepare the pinned native runtime before Tauri can start Cargo build scripts.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repoFile = (path) => fileURLToPath(new URL(path, import.meta.url));
const args = process.argv.slice(2);
const env = { ...process.env };

if (args[0] === 'dev' || args[0] === 'build') {
  let command;
  let fetchArgs;
  if (process.platform === 'win32' && process.arch === 'x64') {
    command = 'powershell.exe';
    // PowerShell 7's module search path can hide Windows PowerShell cmdlets.
    delete env.PSModulePath;
    env.SOTTO_SHERPA_FETCH_SCRIPT = repoFile('./fetch-sherpa-runtime.ps1');
    fetchArgs = [
      '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command',
      '$OutputEncoding = [Console]::OutputEncoding = [Text.UTF8Encoding]::new(); & $env:SOTTO_SHERPA_FETCH_SCRIPT -Target win-x64-shared',
    ];
  } else if (process.platform === 'darwin' && process.arch === 'arm64') {
    command = 'sh';
    fetchArgs = [repoFile('./fetch-sherpa-runtime.sh'), 'osx-arm64-static'];
  } else if (process.platform !== 'linux') {
    console.error(`No verified Sherpa runtime is pinned for ${process.platform}/${process.arch}`);
    process.exit(1);
  }

  if (command) {
    const fetched = spawnSync(command, fetchArgs, { env, encoding: 'utf8' });
    if (fetched.stderr) process.stderr.write(fetched.stderr);
    if (fetched.error || fetched.status !== 0) {
      console.error(fetched.error ?? `Sherpa runtime preparation failed (${fetched.status})`);
      process.exit(1);
    }
    const lines = fetched.stdout.trim().split(/\r?\n/);
    const libDir = lines.at(-1);
    if (!libDir) {
      console.error('Sherpa runtime preparation returned no library directory');
      process.exit(1);
    }
    for (const message of lines.slice(0, -1)) process.stdout.write(`${message}\n`);
    env.SHERPA_ONNX_LIB_DIR = libDir;
  }
}

const tauri = repoFile('../desktop/node_modules/@tauri-apps/cli/tauri.js');
const result = spawnSync(process.execPath, [tauri, ...args], { env, stdio: 'inherit' });
if (result.error) {
  console.error(result.error);
  process.exit(1);
}
process.exit(result.status ?? 1);
