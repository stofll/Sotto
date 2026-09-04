@echo off
REM ================================================================
REM  Sotto - Desktop launcher (dev)
REM  Starts the native Tauri desktop shell (Rust + whisper-rs).
REM  No Python: the app is fully native since the Rust rewrite.
REM ================================================================
REM
REM Prerequisites (one-time):
REM   1. Rust:       winget install Rustlang.Rustup   (or https://rustup.rs)
REM   2. Node.js:    winget install OpenJS.NodeJS.LTS
REM   3. pnpm:       npm install -g pnpm
REM   4. CMake:      required by whisper-rs-sys build script
REM   5. LLVM:       required by bindgen (set LIBCLANG_PATH)
REM   6. MSVC:       Visual Studio Build Tools 2022
REM
REM Run:
REM   desktop\run_desktop.cmd
REM ================================================================

setlocal enabledelayedexpansion

set PROJECT_ROOT=%~dp0..
set DESKTOP_DIR=%~dp0

echo.
echo === Sotto Desktop Shell (dev) ===
echo Project: %PROJECT_ROOT%
echo.

REM --- Frontend deps ---
echo [1/2] Verifying frontend deps...
cd /d "%DESKTOP_DIR%"
if not exist node_modules (
    echo    Installing frontend deps...
    pnpm install
    if %errorlevel% neq 0 (
        echo ERROR: pnpm install failed
        exit /b 1
    )
)
echo    Frontend deps: ok

REM --- Tauri dev (builds Rust + frontend, hot-reload) ---
echo [2/2] Launching Tauri dev...
echo.
echo    The main window should appear shortly.
echo    Press Ctrl+C in this terminal to stop.
echo.

set RUST_LOG=info
pnpm tauri dev

if %errorlevel% neq 0 (
    echo.
    echo ERROR: Tauri failed to start.
    echo Troubleshooting:
    echo   1. Rust installed?    rustc --version
    echo   2. Cargo available?   cargo --version
    echo   3. Tauri CLI?         pnpm tauri --version
    echo   4. WebView2?          winget install Microsoft.EdgeWebView2Runtime
    echo   5. CMake + LLVM on PATH ^(whisper-rs-sys / bindgen^)?
    echo.
    echo For first-build issues, try: cargo clean ^&^& pnpm tauri dev
)

cd /d "%PROJECT_ROOT%"
endlocal
