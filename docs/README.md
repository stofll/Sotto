# Documentation

This is the public documentation index for Sotto. Start with the guides below; dated plans, design drafts, and handoff notes are kept outside this navigation path because they describe historical implementation work rather than the current product contract.

## Using Sotto

- [Platform support](platforms.md) — supported, experimental, and CI-only targets.
- [Models](models.md) — local model families, storage, and platform limits.
- [Overlay appearance](overlay.md) — shape, colors, size, placement and cancellation.
- [Dictionaries](dictionaries.md) — inspect, create and enable term sets, and manage the verbal tics the cleanup removes.
- [Privacy](privacy.md) — telemetry, optional cloud providers, and network data flow.
- [Verifying a download](verifying-downloads.md) — checksums, the update signature, and how to open a build that is not signed by a publisher.
- [Troubleshooting](troubleshooting.md) — common installation and runtime issues.

## Contributing

- [Development](development.md) — prerequisites and a local development loop.
- [Browser UI testing](ui-testing.md) — Python/Playwright setup, isolation, coverage boundaries, and artifacts.
- [Testing](testing.md) — checks expected before opening a pull request.
- [Releasing](releasing.md) — contributor-facing release overview.
- [Release process](RELEASE.md) — the maintainer's step-by-step runbook: signing, updater keys, asset order, rollback.
- [Benchmarks](benchmarks.md) — performance measurements and regression budgets.

## Project context

- [Architecture](architecture.md) — stable high-level boundaries and data flow.
- [Dictionary design](dictionary-sets-plan.md) — decision rationale, limits, and possible extensions; current behavior is in the dictionaries guide.

If a document here disagrees with the executable CI configuration or the application UI, open an issue with the discrepancy and include the relevant platform and version.
