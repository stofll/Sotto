fn main() {
    // tauri_build only emits rerun-if-changed for tauri.conf.json, not for the
    // icon file *contents*. When an icon is replaced in-place (same filename),
    // Cargo therefore reuses the previously compiled resource and the EXE keeps
    // the stale embedded icon (wrong Windows shortcut / taskbar icon). Tracking
    // the icon files here forces the resource to recompile when they change.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/icon.icns");

    // Windows: give test and bench binaries the same common-controls v6
    // dependency the app gets from Tauri's application manifest.
    //
    // Something in the dependency tree imports `TaskDialogIndirect`, which
    // only exists in comctl32 **version 6** (the side-by-side copy). Without
    // a manifest declaring that dependency the loader binds to the v5
    // comctl32 in System32, the import cannot be resolved, and the binary
    // dies before `main` with STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139) and
    // no message. The app EXE is unaffected — `tauri_build` embeds a
    // manifest for it — so this only ever broke `cargo test`.
    //
    // `rustc-link-arg` covers every linked target, including the lib's own
    // unit-test harness — `rustc-link-arg-tests` reaches only `tests/*.rs`.
    // The linker merges this into the manifest it already generates, so the
    // app EXE just restates a dependency Tauri declares anyway.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }

    // macOS: link ApplicationServices framework for AXIsProcessTrusted
    // (Accessibility permission check used by the enigo paste pipeline).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }

    tauri_build::build()
}
