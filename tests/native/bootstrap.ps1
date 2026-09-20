$ErrorActionPreference = 'Stop'

# This is a Sandbox logon script, never a launcher for the host account.
if ($env:USERNAME -ne 'WDAGUtilityAccount') {
    throw 'Run this only through the generated Windows Sandbox configuration.'
}
if (-not (Test-Path -LiteralPath 'C:\SottoReports')) {
    throw 'The dedicated report mapping is missing.'
}
try {
    & 'C:\SottoInput\node.exe' 'C:\SottoInput\probe.mjs' *> 'C:\SottoReports\probe.log'
    $probeExit = $LASTEXITCODE
    Set-Content -LiteralPath 'C:\SottoReports\exit-code.txt' -Value $probeExit
} catch {
    $_ | Out-String | Set-Content -LiteralPath 'C:\SottoReports\bootstrap-error.txt'
    throw
}
