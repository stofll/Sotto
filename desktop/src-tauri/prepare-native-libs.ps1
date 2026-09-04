param(
    # Tauri exposes the target triple to beforeBundleCommand as
    # TAURI_ENV_TARGET_TRIPLE.  CARGO_BUILD_TARGET is also accepted so the
    # script remains usable from a plain cargo/PowerShell invocation.
    [string] $TargetTriple = '',
    # Not $Profile: that is a PowerShell automatic variable, and shadowing it
    # in a script is the kind of thing that reads fine until someone dot-sources
    # this file.
    [string] $BuildProfile = ''
)

$ErrorActionPreference = 'Stop'

# sherpa-rs-sys copies its downloaded Windows runtime beside the Cargo
# executable. Tauri bundles resources after Cargo has finished, so stage the
# exact runtime files in a stable, ignored directory for the bundle config.
$tauriDir = if (Test-Path -LiteralPath 'src-tauri/Cargo.toml') {
    (Resolve-Path -LiteralPath 'src-tauri').Path
} else {
    (Get-Location).Path
}

$profileName = if ($BuildProfile) {
    $BuildProfile
} elseif ($env:TAURI_ENV_DEBUG -eq 'true') {
    'debug'
} else {
    'release'
}

$targetRoot = if ($env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR
} else {
    Join-Path $tauriDir 'target'
}

# Cargo resolves a relative CARGO_TARGET_DIR from the crate/workspace
# invocation.  The helper is normally called from src-tauri, so resolve it
# there as well instead of accidentally looking in the PowerShell process'
# unrelated current directory.
if (-not [IO.Path]::IsPathRooted($targetRoot)) {
    $targetRoot = Join-Path $tauriDir $targetRoot
}

$targetTripleName = if ($TargetTriple) {
    $TargetTriple
} elseif ($env:TAURI_ENV_TARGET_TRIPLE) {
    $env:TAURI_ENV_TARGET_TRIPLE
} elseif ($env:CARGO_BUILD_TARGET) {
    $env:CARGO_BUILD_TARGET
} else {
    ''
}

$stagingDir = Join-Path $tauriDir '.tauri-native/windows/x64'
$required = @(
    'cargs.dll',
    'onnxruntime.dll',
    'onnxruntime_providers_shared.dll',
    'sherpa-onnx-c-api.dll',
    'sherpa-onnx-cxx-api.dll'
)

function Test-RuntimeDir([string] $dir) {
    foreach ($name in $required) {
        if (-not (Test-Path -LiteralPath (Join-Path $dir $name) -PathType Leaf)) {
            return $false
        }
    }
    return $true
}

# Cargo nests its output under target/<triple>/ only when the build asked for
# an explicit --target.  Tauri exports TAURI_ENV_TARGET_TRIPLE either way — it
# is the host triple even for a plain `cargo tauri build` — so the variable
# says WHICH triple, never WHETHER the directory exists.  Taking it at face
# value sent the default Windows build (scripts/build-installer.sh, no
# --target) looking in a directory Cargo had never created, and the release
# bundle failed on a runtime that was sitting one level up the whole time.
#
# So probe instead of deciding: the nested layout first, because an explicit
# --target is the specific case, then the flat one.
$candidates = @()
if ($targetTripleName) {
    $candidates += (Join-Path (Join-Path $targetRoot $targetTripleName) $profileName)
}
$candidates += (Join-Path $targetRoot $profileName)

$sourceDir = $candidates | Where-Object { Test-RuntimeDir $_ } | Select-Object -First 1
if (-not $sourceDir) {
    throw ("sherpa native runtime not found. Looked in: " + ($candidates -join '; '))
}

if (Test-Path -LiteralPath $stagingDir) {
    Remove-Item -LiteralPath $stagingDir -Recurse -Force
}
New-Item -ItemType Directory -Path $stagingDir -Force | Out-Null

foreach ($name in $required) {
    Copy-Item -LiteralPath (Join-Path $sourceDir $name) -Destination (Join-Path $stagingDir $name)
}

Write-Host "Staged sherpa native runtime ($profileName) from $sourceDir in $stagingDir"
