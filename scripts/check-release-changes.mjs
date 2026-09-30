import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compareReleaseVersions, releaseChannel } from './release-version.mjs';

export function checkReleaseChanges(root, source) {
  if (!/^[0-9a-f]{40}$/.test(source ?? '')) throw new Error('A source commit SHA is required');
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' });
  // Include draft tags: their source has already been prepared for a build.
  const tag = git('tag', '--merged', source).trim().split('\n')
    .filter((value) => {
      if (!value.startsWith('v')) return false;
      try { releaseChannel(value.slice(1)); return true; } catch { return false; }
    })
    .sort((a, b) => compareReleaseVersions(b.slice(1), a.slice(1)))[0];
  if (!tag) return { changed: true, reason: 'No stable or beta release tag in the source history.' };

  const paths = git('diff', '--name-only', '-z', `refs/tags/${tag}`, source, '--').split('\0').filter(Boolean);
  const changed = paths.some((path) => !path.startsWith('site/') && !path.startsWith('docs/')
    && !(!path.includes('/') && path.endsWith('.md')));
  return {
    changed,
    reason: changed ? `Application or build changes since ${tag}.` : `No application or build changes since ${tag}.`,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const result = checkReleaseChanges(fileURLToPath(new URL('../', import.meta.url)), process.env.RELEASE_SOURCE);
    console.log(result.reason);
    if (process.env.GITHUB_OUTPUT) writeFileSync(process.env.GITHUB_OUTPUT, `changed=${result.changed}\n`, { flag: 'a' });
    if (process.env.GITHUB_STEP_SUMMARY) writeFileSync(process.env.GITHUB_STEP_SUMMARY, `${result.reason}\n`, { flag: 'a' });
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
