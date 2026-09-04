## Summary

<!-- Describe the user-visible change and why it is needed. -->

## Verification

- [ ] `pnpm exec tsc --noEmit` (from `desktop`)
- [ ] `pnpm test` (from `desktop`)
- [ ] `pnpm build` (from `desktop`)
- [ ] `pnpm bundle:check` (from `desktop`)
- [ ] `cargo fmt --all -- --check` (from `desktop/src-tauri`)
- [ ] `cargo clippy --all-targets -- -D warnings` (from `desktop/src-tauri`)
- [ ] `cargo test` (from `desktop/src-tauri`)
- [ ] Affected platform tested, or limitation explained below

<!-- Full list and rationale: docs/testing.md -->

## Documentation and privacy

- [ ] Public docs updated if behavior, setup, platform support, models, or privacy changed
- [ ] No secrets, local databases, recordings, generated bundles, or agent state committed
- [ ] Telemetry/network behavior reviewed if applicable

## Notes

<!-- Include screenshots, migration notes, known limitations, or skipped checks. -->
