# Working in Sotto

These instructions apply across this repository. Keep this file focused on
durable project rules; put detailed guides in `docs/` and link to them here.
Do not turn it into a task log, implementation inventory, or collection of
temporary plans. If a subtree needs different rules, place an `AGENTS.md` there
and keep those rules specific to that subtree.

## Product priorities

- Keep dictation responsive: hotkey, recording, transcription, then predictable
  paste/copy. Avoid blocking the UI or adding unnecessary work to this path.
- Preserve local transcription by default; cloud STT and LLM formatting are
  optional, explicitly configured features.
- Prefer the simplest solution that meets the actual requirement. Avoid
  speculative abstractions and unrelated refactors; simplify existing complexity
  when it directly obstructs the task.

## Start here

- Read [CONTRIBUTING.md](CONTRIBUTING.md) and the guides relevant to the change
  in [docs/README.md](docs/README.md).
- Use [docs/architecture.md](docs/architecture.md) for application boundaries,
  [docs/development.md](docs/development.md) for setup, and
  [docs/testing.md](docs/testing.md) for the required pre-PR checks.
- Treat manifests, source, and executable CI configuration as evidence of
  current behavior. Flag documentation discrepancies; historical drafts and
  local reports are not product requirements.
- Check `git status` before editing. Preserve existing staged, unstaged, and
  untracked work; keep the patch scoped to the requested task.

## Architecture and implementation

- Sotto is a Windows-first desktop dictation app with a macOS port. React and
  TypeScript live in `desktop/src/`; the Tauri/Rust backend lives in
  `desktop/src-tauri/`. See [docs/platforms.md](docs/platforms.md) before making
  claims about platform support.
- Keep audio capture, speech engines, cloud adapters, persistence, and native
  integrations in Rust. Use the existing `desktop/src/bridge/` abstractions
  for frontend commands and events. When changing an IPC contract, update both
  sides and the relevant bridge tests together.
- Preserve separate settings, overlay, and tray entry points. Avoid importing
  the settings UI into the lightweight windows. Do not raise bundle budgets
  merely to make a failing check pass.
- Do not reintroduce the Python sidecar or `sidecar_invoke`. Python helpers
  under `scripts/` are distinct from the application's native runtime.
- Before adding logic, search for an existing implementation to reuse or extend.
  Keep the search proportionate to the change; avoid parallel implementations
  of the same behavior. Follow nearby code conventions.
- Name new modules for their domain responsibility, not generic `utils` or
  `helpers` buckets. Keep comments concise and focused on non-obvious reasons,
  constraints, or usage; do not narrate the code or rename unrelated files.
- Keep file transcription independent of focused-window paste and history,
  as described in the architecture guide.

## UI and complete behavior

- Reuse tokens and styles from `desktop/src/styles.css` and components from
  `desktop/src/components/`. Do not introduce a new color, spacing, shadow, or
  control variant when an existing one serves the same purpose.
- Use the existing i18n functions: Russian source text is the key, and English
  translations live in `desktop/src/i18n/en.ts`. Evaluate translations at
  render/call time, not in module-level constants.
- For the feature being changed, check all affected entry points and windows
  (settings, overlay, tray), speech engines/providers, and operating systems.
  Check start/stop/cancel, loading, empty, error, and retry states where applicable;
  a fix in one entry point must not leave the same behavior broken elsewhere.
- Verify visual changes in the affected UI, including light/dark themes, both
  locales, and keyboard focus where applicable. Frontend-only preview cannot
  verify native window behavior. Report any UI checks you could not perform.
- Keep idle UI work small. Avoid unnecessary polling, continuous redraws, and
  heavy imports in lightweight windows; measure suspected performance regressions
  rather than loosening budgets.

## Privacy and integrity

- Follow [docs/privacy.md](docs/privacy.md) and
  [docs/telemetry.md](docs/telemetry.md) for network and telemetry changes.
  Telemetry must stay within its allow-listed contract, respect its disable
  switch, and exclude audio, text, secrets, paths, and raw errors.
- Keep credentials, user recordings, local databases, diagnostic output, and
  generated bundles out of commits. Use synthetic or repository test fixtures
  for regression tests; redact sensitive data in shared diagnostics.
- Use isolated temporary data for tests. Before a manual run that writes state,
  establish where config, history, recordings, and model files will be stored;
  do not assume a dev build or worktree isolates application data. Do not reset,
  overwrite, or migrate the user's live data as a testing shortcut.
- Stop only development processes you started and can identify by their tracked
  PID/session. Do not kill processes by a broad name or path match.
- Preserve checksum/signature verification for downloaded assets and updates.
  Consult [docs/models.md](docs/models.md) for model changes and
  [docs/RELEASE.md](docs/RELEASE.md) for release work.
- Pin external GitHub Actions to full commit SHAs with version comments, as
  documented in the development guide.

## Setup and verification

- Take Node and Rust versions from `.node-version` and `rust-toolchain.toml`,
  and pnpm from `desktop/package.json`'s `packageManager`. Use pnpm and preserve
  the checked-in lockfiles; update them when intentionally changing dependencies.
- Run frontend commands in `desktop/`: `pnpm install --frozen-lockfile` for
  setup, `pnpm tauri dev` for the full app, or `pnpm dev` for frontend-only work.
  Native prerequisites are in the development guide.
- Use [docs/testing.md](docs/testing.md) as the checklist rather than maintaining
  another full copy here. [.github/workflows/rust-ci.yml](.github/workflows/rust-ci.yml)
  defines the actual CI jobs, platform matrix, and preparation steps.
- For UI text/i18n changes, also run `pnpm i18n:check` from `desktop/`.
- Before native checks on a clean checkout, build the frontend with `pnpm build`
  from `desktop/`. On Windows, follow CI's Sherpa runtime preparation before
  direct Cargo builds/tests; plain Cargo does not run Tauri's bundling hook.
  Run Cargo commands in `desktop/src-tauri/`, using `--locked` where CI does.
- Ordinary Rust checks use CPU defaults. Enable `gpu-vulkan` or `gpu-metal`
  only for the appropriate platform and task; do not use `--all-features` as
  a generic check because these GPU backends are platform-specific.
- Add or update regression tests for behavior changes. During development,
  run checks relevant to the change; before a PR, follow the required checklist.
  Test observable behavior and meaningful logic, including failure paths; avoid
  tests that merely repeat implementation details or assert callback wiring.
  For documentation-only changes, verify links, commands against their sources,
  and the diff; an application build is unnecessary.
- Native audio, hotkeys, clipboard, model loading, and installer changes need
  verification on the affected OS. Report checks actually run and their results,
  plus any unverified behavior or environment blockers. Do not present a compile
  or unit-test pass as proof of native UI behavior.

## Documentation and completion

- Write commit messages, pull requests, and code comments in English. The
  app's own UI strings remain Russian: they are the i18n keys, and English
  translations live in `desktop/src/i18n/en.ts`.
- Update public guides when a change affects how users accomplish a task,
  configuration, commands, platform support, privacy, or model handling. A purely
  cosmetic change does not need a documentation entry.
- Keep durable decisions, their reasons, and constraints that span components
  in documentation; explain local implementation details in nearby comments.
  Do not duplicate types, field lists, control flow, or PR history in prose.
- Rewrite or remove stale guidance when behavior changes instead of appending
  another version. Keep temporary plans and agent reports out of commits unless
  explicitly requested as deliverables.
- In the final report, explain the resulting behavior, the checks performed,
  and remaining limitations. Keep the report proportional to the change.
