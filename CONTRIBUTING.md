# Contributing to Sotto

Thank you for helping improve the project. Please read the relevant guide in
[`docs/README.md`](docs/README.md) before making a change.

## Before you start

Search existing issues and pull requests first. For a substantial behavior or
UI change, open an issue describing the problem and proposed direction before
investing in a large patch. Do not use public issues for security reports; see
[`SECURITY.md`](SECURITY.md).

## Local setup

Prerequisites and the development loop are in
[`docs/development.md`](docs/development.md); platform expectations are in
[`docs/platforms.md`](docs/platforms.md).

## Required checks

The checks expected before a pull request are listed in
[`docs/testing.md`](docs/testing.md). That file is the single source: it is
kept in step with CI, and it includes gates — such as the per-window bundle
budget — that a shortened copy here would silently omit.

If a check cannot run on your platform, say so in the pull request and include
the relevant CI result. Changes to native behavior should be verified on the
affected OS whenever possible.

## Pull requests

- Keep each pull request focused and explain the user-visible effect.
- Add or update tests for behavior changes.
- Update the public documentation when commands, settings, supported platforms,
  privacy behavior, or model handling changes.
- Do not commit secrets, local databases, recordings, generated bundles, or
  agent/IDE state.
- Include reproduction steps and screenshots/log excerpts when they make a UI
  or platform issue easier to review. Redact sensitive data first.

Maintainers may ask for a smaller patch, a regression test, or a platform
verification step before merging.
