param(
    [Parameter(Mandatory = $true)][string] $BinaryDirectory,
    [Parameter(Mandatory = $true)][string] $OutputPath
)
$ErrorActionPreference = 'Stop'
$binaryRoot = (Resolve-Path -LiteralPath $BinaryDirectory).Path
$executable = Join-Path $binaryRoot 'Sotto.exe'
if (-not (Test-Path -LiteralPath $executable)) { throw "Sotto.exe not found in $binaryRoot" }
foreach ($name in @('cargs.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $binaryRoot $name))) { throw "Missing runtime: $name" }
}
$staging = Join-Path ([IO.Path]::GetTempPath()) ('sotto-portable-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
Copy-Item -LiteralPath $executable -Destination $staging
Get-ChildItem -LiteralPath $binaryRoot -Filter '*.dll' -File | Copy-Item -Destination $staging
New-Item -ItemType File -Path (Join-Path $staging 'portable.flag') | Out-Null
Set-Content -LiteralPath (Join-Path $staging 'README.txt') -Encoding UTF8 -Value @"
Sotto portable for Windows x64
Extract this ZIP to a writable folder and run Sotto.exe.
Microsoft Edge WebView2 Runtime is required.
Settings, models, history and logs are stored in the adjacent data folder.
API keys use Windows Credential Manager and do not move to another computer.
To update, close Sotto through its tray menu and replace the application files.
Keep the data folder and portable.flag. Do not run the installed and portable copies together.
"@
$archive = [IO.Path]::GetFullPath($OutputPath)
New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($archive)) | Out-Null
Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $archive -Force
Write-Output $archive
