# Dependency audit tool

This package runs the unchanged `cargo-audit` application through its public entry point. It pins the upstream release and keeps a separate `Cargo.lock` so security fixes in the tool's dependencies can be applied before the next upstream binary release. All upstream default features and checks, including yanked package checks, remain enabled.

Build the tool with the repository's Rust toolchain:

```sh
cargo build --release --locked --manifest-path scripts/dependency-audit/Cargo.toml
```

Run the resulting `cargo-audit` binary with its usual `audit` command. CI audits both application lockfiles and this package's own lockfile; it fetches the advisory database and crate index normally. When updating the tool or its dependencies, review the lockfile diff, rebuild with `--locked`, and audit this lockfile as well.
