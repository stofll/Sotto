# Platform support

Sotto is a Windows-first desktop application. The table below describes the
support policy for the current tree; a CI build is not by itself a promise of a
published installer.

| Platform | Status | Local speech models | Published installer |
| --- | --- | --- | --- |
| Windows x64 | Primary target | Whisper and the Sherpa-ONNX bundles | Release workflow target |
| macOS arm64 | Port / verify per release | Whisper and the Sherpa-ONNX bundles | Release workflow target |
| macOS x64 | Not currently promised | Whisper and the Sherpa-ONNX bundles when built locally | Confirm per release |
| Linux | CI/build target | Whisper when built locally | No supported artifact currently documented |

The bundle catalogue is gated on `#[cfg(any(windows, target_os = "macos"))]`, so
Windows and macOS carry the same set; there are no Windows-only entries in it.
See [Models](models.md) for the families and their languages.

The macOS deployment minimum is configured in the Tauri bundle metadata. Check
the current release notes before relying on a particular architecture or
installer format. Platform-specific prerequisites, permissions, and the
commands contributors should use are all in [Development](development.md).

## Platform caveats

- Sherpa-ONNX bundles run on CPU on Windows and macOS. The macOS runtime is
  statically linked; Linux currently exposes only Whisper models.
- Whisper is the portable local engine; GPU acceleration depends on the build
  target and available native toolchain.
- Microphone capture, global shortcuts, clipboard access, and accessibility
  behavior are platform-specific. Report a problem with OS version, hardware,
  app version, and exact reproduction steps.
