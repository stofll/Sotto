# Portable version for Windows

Download `Sotto-<version>-windows-x64-portable.zip` from a release, unpack it
into a writable folder, and run `Sotto.exe`. No installer is required. The
machine must have the Microsoft Edge WebView2 Runtime.

A `portable.flag` file next to the EXE enables portable mode. Settings, window
size, models, history, and logs are stored in a sibling `data` folder. API
keys stay in the Windows Credential Manager: on another machine they must be
entered again. Autostart is not registered in portable mode.

To update, exit the app via the "Exit" item in the tray and replace the app
files from the new ZIP, keeping `data` and `portable.flag`. The update
installer is disabled in portable mode. The installed copy and the portable
copy must not run at the same time.

## Building

After a Windows release build, run from the repository root:

```powershell
./scripts/build-portable.ps1 -BinaryDirectory ./desktop/src-tauri/target/release -OutputPath ./artifacts/Sotto-portable.zip
```

If `--target` is used, add the target triplet to the build directory path. The
script packs the EXE, the DLLs, and the mode marker; the release workflow
automatically attaches the ZIP to the release draft.
