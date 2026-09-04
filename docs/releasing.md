# Releasing overview

This is the contributor-facing release outline. The CI workflow and the
maintainer's protected release configuration are authoritative for exact target
matrices, signing, updater keys, and secrets.

Before proposing a release:

1. Agree on the release version and update the package metadata and user-facing
   changelog together.
2. Run the checks in [Testing](testing.md) and verify the affected platforms.
3. Review model/runtime assets, privacy behavior, installer output, and release
   notes for the actual target matrix.
4. Create the release tag only after the version consistency check and CI pass.

The release draft receives a CycloneDX SBOM and a license report alongside the
installers; see [Development](development.md) for how they are produced. Check
that they are present before publishing the draft.

Do not put signing certificates, updater private keys, provider keys, telemetry
secrets, or personal access tokens in the repository or an issue. Maintainers
should keep those in protected CI secrets and document only the public
verification steps.

The maintainer's own procedure — signing secrets, updater keys, asset upload
order, rollback — is in [Release process](RELEASE.md). Contributors do not
need it; it is listed here so the two documents do not drift apart unnoticed.
