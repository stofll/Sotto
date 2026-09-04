$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'src-tauri/prepare-native-libs.ps1') @args
