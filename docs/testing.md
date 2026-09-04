# Testing

Run the frontend checks from the repository root:

```bash
cd desktop
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit
pnpm test
pnpm build
pnpm bundle:check
```

`pnpm bundle:check` reads the built `dist/` and compares each window's startup
JavaScript against a budget declared in `desktop/check-bundle-size.mjs`. Vite's
own 500 kB notice only warns; this step fails. Raising a budget is a deliberate
decision, not a way to make the check quiet.

Run the Rust checks from the repository root:

```bash
cd desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The CI workflow is authoritative for the operating-system matrix. Changes to
microphone capture, global shortcuts, clipboard behavior, installers, or model
loading should be tested on the affected platform in addition to the portable
unit tests.

When a test needs audio, keep the fixture under the repository's test fixture
directory. User recordings and diagnostic output belong in runtime directories
and must not be added to the repository.
