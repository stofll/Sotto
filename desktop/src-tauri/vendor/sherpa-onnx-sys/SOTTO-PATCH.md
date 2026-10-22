# Sherpa build dependency patch

This is the published `sherpa-onnx-sys 1.13.8` crate from <https://static.crates.io/crates/sherpa-onnx-sys/sherpa-onnx-sys-1.13.8.crate>, SHA-256 `14bbeabb73f73f1c1f4a278af0ee9ae90a19f9727482027047fb9422fa14f8fe`. Its Apache-2.0 license, Rust bindings and build script are retained unchanged. Registry metadata records upstream revision `11afbd009a7f8c08f4bcf2fc1b265d0df4670fbf` with uncommitted changes, so the published crate checksum is the source pin.

The only upstream change is the build dependency `zip = "2"` to `zip = "9"`, preserving its default features. ZIP 9 fixes RUSTSEC-2026-0367, RUSTSEC-2026-0368 and RUSTSEC-2026-0369. The ZIP API is used only by upstream's iOS archive extraction; the Windows and macOS application still links the separately SHA-256-verified native runtimes supplied through `SHERPA_ONNX_LIB_DIR`.

Remove this patch when an upstream Sherpa release uses a fixed ZIP dependency, after updating both Rust bindings and the checked native runtime pins together. The application Cargo.lock pins the resolved dependency versions.
