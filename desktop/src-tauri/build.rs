#[path = "build_support/engine_versions.rs"]
mod engine_versions;

/// Every command in `generate_handler!`. Declaring them makes Tauri check each
/// call against the window's capability, so a window reaches only the
/// commands `capabilities/<window>.json` grants it; the frontend test
/// `bridge/window-capabilities.test.ts` keeps this list, the handler and the
/// grants in step.
const APP_COMMANDS: &[&str] = &[
    "pick_audio_file",
    "transcribe_audio_file",
    "cancel_audio_file",
    "hide",
    "current_state",
    "overlay_ready",
    "overlay_notch",
    "paste_target_name",
    "open_url",
    "validate_hotkey",
    "fetch_provider_models",
    "cancel_model_download",
    "set_overlay_presentation",
    "cancel_recording",
    "start_microphone_test",
    "stop_microphone_test",
    "set_microphone_test_monitor",
    "get_stats",
    "list_history",
    "delete_history_entry",
    "clear_history",
    "history_recording",
    "preview_history_ai_processing",
    "apply_history_ai_processing",
    "copy_history_entry",
    "open_history_entry",
    "get_config",
    "save_config",
    "app_version",
    "get_whats_new",
    "dismiss_whats_new",
    "check_update",
    "install_update",
    "list_microphones",
    "list_models",
    "model_assessments",
    "get_runtime_status",
    "download_model",
    "set_model",
    "delete_model",
    "save_api_key",
    "has_api_key",
    "delete_api_key",
    "test_ai_prompt",
    "process_text_ai",
    "preview_format",
    "preview_replacements",
    "check_accessibility",
    "test_paste",
    "preview_sound_cue",
    "preview_output_duck",
    "get_diagnostics",
    "get_public_diagnostics",
    "get_public_logs",
    "save_public_logs",
    "open_diagnostics_folder",
    "open_recordings_folder",
    "recordings_size",
    "logs_size",
    "clear_logs",
    "dictionary_presets",
    "parasite_sets",
    "term_matching",
    "check_dictionary_text",
    "analyze_dictionary",
];

fn main() {
    // Derive benchmark compatibility from the dependencies actually locked for
    // this build. Historical reference files keep their original method label.
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=../../scripts/sherpa-runtime.lock");
    println!("cargo:rerun-if-changed=build_support/engine_versions.rs");
    let lockfile = std::fs::read_to_string("Cargo.lock").expect("read Cargo.lock");
    let runtime_lock = std::fs::read_to_string("../../scripts/sherpa-runtime.lock")
        .expect("read Sherpa runtime lock");
    let (whisper, sherpa) = engine_versions::versions(&lockfile, &runtime_lock)
        .expect("identify locked speech engines");
    println!("cargo:rustc-env=SOTTO_WHISPER_VERSION={whisper}");
    println!("cargo:rustc-env=SOTTO_SHERPA_VERSION={sherpa}");

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

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
