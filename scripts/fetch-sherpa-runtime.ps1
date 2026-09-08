param(
    # Key from scripts/sherpa-runtime.lock.
    [string] $Target = 'win-x64-shared',
    # Where to keep the archive and its extracted tree. Defaults to the Cargo
    # target directory, which is already ignored by git.
    [string] $CacheDirectory = ''
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# `sherpa-onnx-sys` downloads its native archive over the network and extracts
# it without checking a hash. Fetch it here instead, verify it, and let the
# caller point SHERPA_ONNX_LIB_DIR at the result — the build script then
# downloads nothing. See AGENTS.md: downloaded assets stay verified.

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$lockFile = Join-Path $PSScriptRoot 'sherpa-runtime.lock'

$version = $null
$archive = $null
$expected = $null
foreach ($line in Get-Content -LiteralPath $lockFile) {
    $trimmed = $line.Trim()
    if (-not $trimmed -or $trimmed.StartsWith('#')) { continue }
    $fields = $trimmed -split '\s+'
    if ($fields[0] -eq 'version') { $version = $fields[1]; continue }
    if ($fields[0] -eq $Target) { $archive = $fields[1]; $expected = $fields[2].ToLower() }
}
if (-not $version) { throw "No version in $lockFile" }
if (-not $archive) { throw "No entry for target '$Target' in $lockFile" }

if (-not $CacheDirectory) {
    $CacheDirectory = if ($env:CARGO_TARGET_DIR) {
        $env:CARGO_TARGET_DIR
    } else {
        Join-Path $repoRoot 'desktop/src-tauri/target'
    }
    $CacheDirectory = Join-Path $CacheDirectory 'sherpa-runtime'
}
New-Item -ItemType Directory -Path $CacheDirectory -Force | Out-Null

$archivePath = Join-Path $CacheDirectory $archive
$extractedName = $archive -replace '\.tar\.bz2$', ''
$libDir = Join-Path (Join-Path $CacheDirectory $extractedName) 'lib'

if (-not (Test-Path -LiteralPath $archivePath)) {
    $url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$archive"
    Write-Host "Downloading $url"
    # Into a temporary name first: an interrupted download must not be mistaken
    # for a cached archive on the next run.
    $partial = "$archivePath.partial"
    Invoke-WebRequest -Uri $url -OutFile $partial
    Move-Item -LiteralPath $partial -Destination $archivePath -Force
}

$actual = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected) {
    # Remove it: leaving an archive that failed verification on disk invites a
    # later run to pick it up from the cache branch above.
    Remove-Item -LiteralPath $archivePath -Force
    throw "Checksum mismatch for ${archive}: expected $expected, got $actual"
}
Write-Host "Verified $archive ($expected)"

# A directory that exists is not a directory that is complete: an extraction
# killed halfway leaves one behind, and accepting it hands Cargo a runtime with
# libraries missing. Require actual files.
function Test-LibDirectory([string] $dir) {
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) { return $false }
    return (Get-ChildItem -LiteralPath $dir -File | Measure-Object).Count -gt 0
}

if (-not (Test-LibDirectory $libDir)) {
    # Extract into a staging directory and move it into place only once it
    # looks complete, so an interrupted run leaves no half-tree to be cached.
    $staging = Join-Path $CacheDirectory (".extract-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $staging | Out-Null
    try {
        # Extract from inside the staging directory with a relative name.
        # `tar` here may be either bsdtar from System32 or GNU tar from Git for
        # Windows, and GNU tar reads an absolute Windows path as a remote host
        # spec: the drive letter's colon makes `D:\...` look like `host:path`.
        Push-Location -LiteralPath $staging
        try {
            tar -xjf "../$archive"
            if ($LASTEXITCODE -ne 0) { throw "Failed to extract $archive" }
        } finally {
            Pop-Location
        }

        $stagedRoot = Join-Path $staging $extractedName
        if (-not (Test-LibDirectory (Join-Path $stagedRoot 'lib'))) {
            throw "No lib directory with files in $archive"
        }

        $destination = Join-Path $CacheDirectory $extractedName
        if (Test-Path -LiteralPath $destination) {
            Remove-Item -LiteralPath $destination -Recurse -Force
        }
        Move-Item -LiteralPath $stagedRoot -Destination $destination
    } finally {
        if (Test-Path -LiteralPath $staging) {
            Remove-Item -LiteralPath $staging -Recurse -Force
        }
    }
}
if (-not (Test-LibDirectory $libDir)) { throw "No lib directory with files in $archive" }

Write-Host "Native runtime ready in $libDir"
# Set it for this process — which is what a dot-sourced run needs — and write
# the path to stdout so a caller can capture it:
#   $env:SHERPA_ONNX_LIB_DIR = ./scripts/fetch-sherpa-runtime.ps1
# `Write-Host` above deliberately bypasses the pipeline so it does not.
$env:SHERPA_ONNX_LIB_DIR = $libDir
if ($env:GITHUB_ENV) {
    "SHERPA_ONNX_LIB_DIR=$libDir" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
}
Write-Output $libDir
