## Summary

<!-- Describe the user-visible change and why it is needed. -->

## Verification

- [ ] Frontend: `pnpm exec tsc --noEmit`, `pnpm test`, `pnpm build`, `pnpm bundle:check` (from `desktop`)
- [ ] Rust: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (from `desktop/src-tauri`)
- [ ] Affected platform tested, or the limitation stated alongside these checks

<!-- Full list and rationale: docs/testing.md -->
