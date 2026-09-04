@echo off
REM Build the NSIS installer for Sotto (Windows).
REM
REM Output: %CARGO_TARGET_DIR%\release\bundle\nsis\Sotto_<version>_x64-setup.exe
REM
REM The wrapper points CARGO_TARGET_DIR at a directory outside the working
REM copy on purpose: whisper.cpp is unpacked under it and MSVC bakes those
REM paths into the binary via __FILE__. See scripts/build-installer.sh and #41.
REM
REM Run it through scripts/build-installer.sh -- that wrapper exports the
REM empty signing password this script cannot export itself (see below).
REM
REM Requires:
REM   - Visual Studio BuildTools 2022 (vcvars64.bat)
REM   - LLVM at C:\LLVM\bin (set via LIBCLANG_PATH for bindgen)
REM   - NSIS 3.x installed (winget install NSIS.NSIS)
REM   - tauri-cli installed (cargo install tauri-cli --version "^2" --locked)

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
if errorlevel 1 (
    echo [build-installer] vcvars64.bat failed
    exit /b 1
)

REM CMake is required by whisper-rs-sys (build script). vcvars64
REM does not add the standalone install to PATH on this machine, so
REM prepend it explicitly.
set "PATH=C:\Program Files\CMake\bin;%PATH%"

set LIBCLANG_PATH=C:\LLVM\bin

REM Updater artifacts are signed at bundle time. Without the key
REM "cargo tauri build" fails, because tauri.conf.json has
REM bundle.createUpdaterArtifacts = true. The key lives outside the
REM repo; the public half is in tauri.conf.json.
set "TAURI_SIGNING_PRIVATE_KEY_PATH=%USERPROFILE%\.tauri\sotto.key"
REM The bundler reads TAURI_SIGNING_PRIVATE_KEY, not ..._PATH; it accepts
REM either the key contents or a path to the key file.
set "TAURI_SIGNING_PRIVATE_KEY=%USERPROFILE%\.tauri\sotto.key"
REM The key has an empty password, and tauri prompts interactively unless
REM TAURI_SIGNING_PRIVATE_KEY_PASSWORD is set -- which hangs a non-interactive
REM build. cmd cannot create an empty-but-present variable ("set VAR=" deletes
REM it), so the caller must export it; a POSIX shell can.
REM
REM This is NOT checked with "if defined". That was the bug: cmd's "if defined"
REM reports an empty value as undefined, so the guard rejected exactly the
REM correct invocation and the script could never run. The variable itself does
REM arrive -- verified by reading the environment from a child process, which
REM sees "". Since cmd cannot see it, the wrapper sets a second, non-empty
REM marker alongside it, and that is what we check.
if not defined SOTTO_SIGNING_PASSWORD_EXPORTED (
    echo [build-installer] run this through scripts/build-installer.sh
    echo [build-installer] It exports the empty signing password; started
    echo [build-installer] directly, tauri stops at a password prompt.
    exit /b 1
)
if not exist "%TAURI_SIGNING_PRIVATE_KEY_PATH%" (
    echo [build-installer] signing key not found: %TAURI_SIGNING_PRIVATE_KEY_PATH%
    echo [build-installer] regenerate with: tauri signer generate -w %%USERPROFILE%%\.tauri\sotto.key
    exit /b 1
)

cd /d "%~dp0..\desktop\src-tauri"
if errorlevel 1 (
    echo [build-installer] failed to cd to src-tauri
    exit /b 1
)

REM --features gpu-vulkan: the GPU backend is opt-in so that CI and a plain
REM cargo build need no Vulkan SDK. A release without it would silently ship
REM CPU-only inference.
cargo tauri build --features gpu-vulkan -- --locked
if errorlevel 1 (
    echo [build-installer] cargo tauri build failed
    exit /b 1
)

echo.
echo [build-installer] OK
echo [build-installer] Installer and .sig are in:
echo [build-installer]   %CARGO_TARGET_DIR%\release\bundle\nsis
