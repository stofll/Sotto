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

function parts(version) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
    throw new Error(`Expected a stable version X.Y.Z, got '${version}'`);
  }
  return version.split('.').map(BigInt);
}

function compare(a, b) {
  const left = parts(a);
  const right = parts(b);
  for (let i = 0; i < 3; i++) {
    if (left[i] !== right[i]) return left[i] > right[i] ? 1 : -1;
  }
  return 0;
}

export function nextVersion(current, tags, kind, exact = '') {
  parts(current);
  if (!['patch', 'minor', 'major'].includes(kind)) {
    throw new Error(`Unknown release kind '${kind}'`);
  }
  let base = current;
  for (const tag of tags) {
    if (/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag)) {
      const version = tag.slice(1);
      if (compare(version, base) > 0) base = version;
    }
  }
  let next = exact.trim();
  if (!next) {
    const values = parts(base);
    const index = { major: 0, minor: 1, patch: 2 }[kind];
    values[index] += 1n;
    for (let i = index + 1; i < 3; i++) values[i] = 0n;
    next = values.join('.');
  }
  if (compare(next, base) <= 0) {
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

export function updateVersions(root, next) {
  parts(next);
  const files = versionFiles.map((path) => ({ path, text: readFileSync(resolve(root, path), 'utf8') }));
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

  // Validate every source before writing; a mismatch must leave all files untouched.
  for (const file of files) writeFileSync(resolve(root, file.path), file.updated);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = fileURLToPath(new URL('../', import.meta.url));
    const current = JSON.parse(readFileSync(resolve(root, versionFiles[2]), 'utf8')).version;
    const tags = execFileSync('git', ['tag', '--list'], { cwd: root, encoding: 'utf8' }).trim().split('\n');
    const next = nextVersion(current, tags, process.env.RELEASE_KIND ?? 'patch', process.env.RELEASE_VERSION ?? '');
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
