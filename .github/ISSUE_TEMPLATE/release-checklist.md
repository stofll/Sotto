---
name: Release Checklist
about: Track the steps required for a Sotto release.
title: 'Release vX.Y.Z checklist'
labels: release
assignees: ''

---

## Pre-release

- [ ] `sh scripts/check-version.sh vX.Y.Z` — every version source agrees with the tag
- [ ] `cargo outdated --exit-code 1` — no outdated deps
- [ ] `cargo audit` — no advisories
- [ ] `cargo fmt --all -- --check` — clean
- [ ] `cargo clippy --all-targets -- -D warnings` — clean
- [ ] `cargo test --all-targets` — green
- [ ] `pnpm exec tsc --noEmit` — clean
- [ ] `pnpm test` — green
- [ ] `pnpm build` — clean
- [ ] `pnpm bundle:check` — every window within budget
- [ ] `pnpm i18n:check` — no untranslated keys, no Cyrillic outside `t()`
- [ ] `pnpm tauri build` — smoke test passes
- [ ] `sh scripts/release.sh` — dry run passes on `main`

## Tag & Build

- [ ] `git tag -a -m 'release X.Y.Z' vX.Y.Z` on `main`
- [ ] `git push origin vX.Y.Z`
- [ ] Both matrix targets in `.github/workflows/release.yml` succeed
- [ ] "Проверить, что в бинаре нет путей сборочной машины" is green — a red one
      is a reason not to publish the draft
- [ ] `SHA256SUMS.txt` attached by the `checksums` job
- [ ] SBOM and license report attached by the `sbom` job

## Publish

- [ ] Draft installed and smoke-tested on each target before publishing
- [ ] Release body written for users — the updater shows it as "what's new"
- [ ] Draft published (this is what makes the update visible to existing installs)
- [ ] Update from the previous version actually offered and applied

## Post-release

- [ ] New issues and release health monitored after publication
- [ ] Monitoring for new issues (first 24 h)
- [ ] Rollback lever known: un-latest the release, see `docs/RELEASE.md`

## Notes

<!-- Add any release-specific notes, blockers, or caveats here -->
