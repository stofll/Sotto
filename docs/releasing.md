# Releasing overview

This is the contributor-facing release outline. The CI workflow and the maintainer's protected release configuration are authoritative for exact target matrices, signing, updater keys, and secrets.

Before proposing a release:

1. Agree on the release scope and prepare user-facing release notes using the [release notes template](RELEASE.md#whats-new-template).
2. Run the checks in [Testing](testing.md) and verify the affected platforms.
3. Review model/runtime assets, privacy behavior, installer output, and release notes for the actual target matrix.
4. Let **Prepare Release** prepare a beta from `main` on Monday or Thursday at 21:00 Moscow time, or run it manually to choose a stable or beta channel, another time, version bump, or exact base version. Scheduled runs skip sources without application/build changes. Both paths run full source CI, commit only version changes and push the commit and tag through the release App to start a draft build. See [preparation and repository permissions](RELEASE.md#1-prepare-the-version-in-github-actions).

The release draft receives a CycloneDX SBOM and a license report alongside the installers; see [Development](development.md) for how they are produced. Check that they are present before publishing the draft.

Track verification and publication with the [release checklist](../.github/ISSUE_TEMPLATE/release-checklist.md). Write the release description in the GitHub draft; it is also shown in Sotto's update dialog.

Beta builds carry a `-beta.N` version suffix and must retain the GitHub prerelease flag when published. Installed copies receive stable updates unless the user enables **Help → Updates → Receive beta builds**; stable users therefore do not receive scheduled betas. A stable release needs its own version and build rather than relabeling a beta draft.

Do not put signing certificates, updater private keys, provider keys, telemetry secrets, or personal access tokens in the repository or an issue. Maintainers should keep those in protected CI secrets and document only the public verification steps.

The maintainer's own procedure — signing secrets, updater keys, asset upload order, rollback — is in [Release process](RELEASE.md). Contributors do not need it; it is listed here so the two documents do not drift apart unnoticed.
