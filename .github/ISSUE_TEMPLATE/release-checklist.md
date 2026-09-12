---
name: Release Checklist
about: Verify, publish, and follow up on a Sotto release.
title: 'Release vX.Y.Z checklist'
labels: release
assignees: ''
---

<!-- Record actual results and blockers. Follow the release runbook; do not treat successful compilation as proof of native behavior. -->

## Prepare

- [ ] Prepare Release run started on `main` with the intended bump or exact stable version.
- [ ] Dependency review and required checks completed: [release preparation](https://github.com/stofll/Sotto/blob/main/docs/RELEASE.md#pre-release) and [testing](https://github.com/stofll/Sotto/blob/main/docs/testing.md).
- [ ] Source tree passed Rust CI and UI tests; the bot committed only version changes and pushed the intended tag.

## Build and verify

- [ ] Windows and macOS draft release jobs and the artifact path checks pass.
- [ ] Draft contains all [required assets](https://github.com/stofll/Sotto/blob/main/docs/RELEASE.md#draft-contents), including update signatures, checksums, the portable ZIP, and dependency/license reports.
- [ ] Windows and macOS installers tested with isolated data; model loading, recording, stop/cancel, and paste/copy checked. Record OS versions, models, and results below.
- [ ] Windows portable ZIP tested, including manual update while preserving its data folder.

## Publish

- [ ] Release description follows the [release notes template](https://github.com/stofll/Sotto/blob/main/docs/RELEASE.md#whats-new-template); placeholders and empty sections removed, changelog link checked.
- [ ] Required user actions and known limitations documented; release blockers resolved.
- [ ] Draft published after verification. This makes the update available to existing installations.

## Follow up

- [ ] Update from the previous version offered and applied successfully.
- [ ] Release health and new issues reviewed during the first 24 hours; use the [rollback procedure](https://github.com/stofll/Sotto/blob/main/docs/RELEASE.md#rollback-procedure) if needed.

## Results and blockers

<!-- Add verification results, CI links, and any remaining limitations. Keep signing keys, credentials, recordings, and personal data out of this issue. -->
