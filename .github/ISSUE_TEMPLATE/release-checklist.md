---
name: Release Checklist
about: Track the steps required for a Sotto release.
title: 'Release vX.Y.Z checklist'
labels: release
assignees: ''

---

## Pre-release

- [ ] Version is consistent across all package/bundle metadata and the user-facing changelog
- [ ] `cargo outdated --exit-code 1` — no outdated deps
- [ ] `cargo audit` — no advisories
- [ ] `cargo fmt --all -- --check` — clean
- [ ] `cargo clippy --all-targets -- -D warnings` — clean
- [ ] `cargo test --all-targets` — green
- [ ] `pnpm exec tsc --noEmit` — clean
- [ ] `pnpm test` — green
- [ ] `pnpm build` — clean
- [ ] `pnpm tauri build` — smoke test passes
- [ ] Code signing secrets present in protected CI storage (Windows cert, Apple notarization)
- [ ] Signing tested locally (`pnpm tauri build --bundles msi` / `--bundles dmg`)

## Tag & Build

- [ ] `git tag vX.Y.Z`
- [ ] `git push origin vX.Y.Z`
- [ ] CI build succeeds for every target currently configured in `.github/workflows/release.yml`
- [ ] Checksums generated (`sha256sum *.dmg *.msi *.exe > SHA256SUMS.txt`)

## Publish

- [ ] GitHub Release created from tag `vX.Y.Z`
- [ ] Release notes filled in (Security / Performance / New Features / Bug Fixes)
- [ ] Assets uploaded for the actual CI target matrix, with `SHA256SUMS.txt` where applicable
- [ ] "What's new" entry written

## Post-release

- [ ] New issues and release health monitored after publication
- [ ] Monitoring for new issues (first 24 h)
- [ ] Rollback procedure documented if needed

## Notes

<!-- Add any release-specific notes, blockers, or caveats here -->
