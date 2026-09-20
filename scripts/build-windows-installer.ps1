$ErrorActionPreference = 'Stop'

# Called after build-installer.bat establishes MSVC, LLVM and signing inputs.
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
Push-Location -LiteralPath (Join-Path $repoRoot 'desktop/src-tauri')
try {
    $env:SHERPA_ONNX_LIB_DIR = & (Join-Path $PSScriptRoot 'fetch-sherpa-runtime.ps1') -Target win-x64-shared
    cargo build --release --locked -p sherpa-onnx-sys
    if ($LASTEXITCODE -ne 0) { throw 'Sherpa runtime build failed' }

    $targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    if ($env:CARGO_BUILD_TARGET) { $targetRoot = Join-Path $targetRoot $env:CARGO_BUILD_TARGET }
    $runtimeRoot = Join-Path $targetRoot 'release'
    $required = @('onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll')
    $missing = $required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $runtimeRoot $_) -PathType Leaf) }
    if ($missing) {
        # Cached fingerprints can survive removal of the copied runtime DLLs.
        cargo clean --release -p sherpa-onnx-sys
        if ($LASTEXITCODE -ne 0) { throw 'Could not invalidate the Sherpa build cache' }
        cargo build --release --locked -p sherpa-onnx-sys
        if ($LASTEXITCODE -ne 0) { throw 'Sherpa runtime rebuild failed' }
    }
    & ./prepare-native-libs.ps1 -BuildProfile release

    # Use the repository's locked CLI, including its updater signing fixes.
    pnpm --dir .. exec tauri build --features gpu-vulkan '--' --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tauri installer build failed' }
} finally {
    Pop-Location
}
