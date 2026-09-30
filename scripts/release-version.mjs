import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const versionFiles = [
  'desktop/src-tauri/Cargo.toml',
  'desktop/src-tauri/Cargo.lock',
  'desktop/package.json',
  'desktop/src-tauri/Info.plist',
];

function parsed(version) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-beta\.([1-9]\d*))?$/.exec(version);
  if (!match) throw new Error(`Expected X.Y.Z or X.Y.Z-beta.N, got '${version}'`);
  return { parts: match.slice(1, 4).map(BigInt), beta: match[4] ? BigInt(match[4]) : null };
}

function parts(version) {
  return parsed(version).parts;
}

export function releaseChannel(version) {
  return parsed(version).beta === null ? 'stable' : 'beta';
}

export function compareReleaseVersions(a, b) {
  const left = parsed(a);
  const right = parsed(b);
  for (let i = 0; i < 3; i++) {
    if (left.parts[i] !== right.parts[i]) return left.parts[i] > right.parts[i] ? 1 : -1;
  }
  if (left.beta === right.beta) return 0;
  if (left.beta === null) return 1;
  if (right.beta === null) return -1;
  return left.beta > right.beta ? 1 : -1;
}

export function nextVersion(current, tags, kind, exact = '', channel = 'stable') {
  parts(current);
  if (!['stable', 'beta'].includes(channel)) throw new Error(`Unknown release channel '${channel}'`);
  if (!['patch', 'minor', 'major'].includes(kind)) {
    throw new Error(`Unknown release kind '${kind}'`);
  }
  let base = current;
  for (const tag of tags) {
    if (/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-beta\.[1-9]\d*)?$/.test(tag)) {
      const version = tag.slice(1);
      if (compareReleaseVersions(version, base) > 0) base = version;
    }
  }
  let next = exact.trim();
  if (next && releaseChannel(next) !== 'stable') throw new Error('Supply an exact version without a prerelease suffix');
  if (!next) {
    const values = parts(base);
    const index = { major: 0, minor: 1, patch: 2 }[kind];
    // Patch preparation continues the pending beta series or promotes it.
    if (kind !== 'patch' || releaseChannel(base) === 'stable') {
      values[index] += 1n;
      for (let i = index + 1; i < 3; i++) values[i] = 0n;
    }
    next = values.join('.');
  }
  if (channel === 'beta') {
    let sequence = 0n;
    for (const version of [current, ...tags.filter((tag) => tag.startsWith('v')).map((tag) => tag.slice(1))]) {
      if (version.startsWith(`${next}-beta.`) && /^[1-9]\d*$/.test(version.slice(`${next}-beta.`.length))) {
        const value = BigInt(version.slice(`${next}-beta.`.length));
        if (value > sequence) sequence = value;
      }
    }
    next += `-beta.${sequence + 1n}`;
  }
  if (compareReleaseVersions(next, base) <= 0) {
    throw new Error(`Release version ${next} must be greater than ${base}`);
  }
  return next;
}

function replaceOne(text, pattern, replacement, label) {
  const matches = [...text.matchAll(new RegExp(pattern.source, `${pattern.flags}g`))];
  if (matches.length !== 1) throw new Error(`Expected exactly one ${label}`);
  return text.replace(pattern, replacement);
}

function replaceVersion(text, pattern, current, next, label) {
  return replaceOne(text, pattern, (_match, prefix, version, suffix) => {
    if (version !== current) {
      throw new Error(`${label} version ${version} disagrees with package.json ${current}`);
    }
    return `${prefix}${next}${suffix}`;
  }, label);
}

function versionChanges(texts, next) {
  parts(next);
  const files = versionFiles.map((path, index) => ({ path, text: texts[index] }));
  const current = JSON.parse(files[2].text).version;
  parts(current);

  // Bound the manifest edit to [package], even if another section has a version.
  files[0].updated = replaceOne(
    files[0].text,
    /^\[package\]\r?\n[\s\S]*?(?=^\[|(?![\s\S]))/m,
    (section) => replaceVersion(section, /^(version\s*=\s*")([^"]+)(")/m, current, next, 'Cargo.toml package version'),
    'Cargo.toml package section',
  );
  files[1].updated = replaceVersion(
    files[1].text,
    /^(\[\[package\]\]\r?\nname = "sotto"\r?\nversion = ")([^"]+)(")/m,
    current, next, 'Cargo.lock sotto version',
  );
  files[2].updated = replaceVersion(
    files[2].text, /^(  "version": ")([^"]+)(")/m,
    current, next, 'package.json version',
  );
  files[3].updated = replaceVersion(
    files[3].text, /(<key>CFBundleShortVersionString<\/key>\s*<string>)([^<]+)(<\/string>)/,
    current, next, 'Info.plist version',
  );

  return files;
}

export function updateVersions(root, next) {
  const files = versionChanges(versionFiles.map((path) => readFileSync(resolve(root, path), 'utf8')), next);
  // Validate all replacements before writing any file.
  for (const file of files) writeFileSync(resolve(root, file.path), file.updated);
}

export function verifyReleaseCommit(root, base, next) {
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' });
  if (git('rev-parse', 'HEAD^').trim() !== base) throw new Error('Release commit must directly follow the checked source');
  const changed = git('diff', '--name-only', '-z', base, 'HEAD').split('\0').filter(Boolean);
  if (changed.length !== versionFiles.length || changed.some((path) => !versionFiles.includes(path))) {
    throw new Error('Release commit must change exactly the four version files');
  }
  const expected = versionChanges(versionFiles.map((path) => git('show', `${base}:${path}`)), next);
  for (const file of expected) {
    if (git('show', `HEAD:${file.path}`) !== file.updated) throw new Error(`Unexpected non-version change in ${file.path}`);
  }
  if (git('diff', '--summary', base, 'HEAD').trim()) throw new Error('Release commit must preserve file modes');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = fileURLToPath(new URL('../', import.meta.url));
    const current = JSON.parse(readFileSync(resolve(root, versionFiles[2]), 'utf8')).version;
    const tags = execFileSync('git', ['tag', '--list'], { cwd: root, encoding: 'utf8' }).trim().split('\n');
    const next = nextVersion(current, tags, process.env.RELEASE_KIND ?? 'patch', process.env.RELEASE_VERSION ?? '', process.env.RELEASE_CHANNEL ?? 'stable');
    updateVersions(root, next);
    execFileSync('sh', ['scripts/check-version.sh', `v${next}`], { cwd: root, stdio: ['ignore', 'inherit', 'inherit'] });
    if (process.env.GITHUB_OUTPUT) {
      writeFileSync(process.env.GITHUB_OUTPUT, `version=${next}\ntag=v${next}\n`, { flag: 'a' });
    }
    console.log(`Prepared v${next}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
