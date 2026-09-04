# Verifying a download

Sotto is **not signed with a publisher certificate** yet: no Authenticode
certificate on Windows, no Apple Developer ID on macOS. Both systems therefore
warn about the download, and the warning is honest — the operating system
genuinely cannot tell who built the file.

What exists instead is verification you can perform yourself. It answers a
narrower question than a publisher signature — "is this the file the release
workflow produced?" rather than "who is the publisher?" — but it is the part
that catches a tampered mirror or a corrupted download.

## Checksums

Every release carries `SHA256SUMS.txt`, generated from the draft release's own
assets by the release workflow. Download it next to the installer and check:

```bash
# macOS / Linux
sha256sum --ignore-missing -c SHA256SUMS.txt
```

```powershell
# Windows PowerShell — compare against the line for your file
Get-FileHash .\Sotto_0.0.1_x64-setup.exe -Algorithm SHA256
Select-String -Path .\SHA256SUMS.txt -Pattern 'x64-setup.exe'
```

A checksum published in the same place as the file protects against an
accidentally corrupted download and against a mirror that serves something
else. It does not protect against someone who can rewrite the release itself —
for that, see the update signature below.

## Update signature (minisign)

Update artifacts are signed with a minisign key whose public half is compiled
into the app (`plugins.updater.pubkey` in `tauri.conf.json`) and reproduced
here:

```text
untrusted comment: minisign public key: A9C6A58CB5E59739
RWQ5l+W1jKXGqdNMnGRzgTdrAGl9xu+fRs0CSQnQb+h5MMb1B2pvOfKj
```

The installed app verifies this automatically before applying an update, and
refuses artifacts signed by any other key. To check a downloaded artifact by
hand, take the matching `.sig` file from the release and run
[minisign](https://jedisct1.github.io/minisign/):

```bash
minisign -Vm Sotto_0.0.1_x64-setup.nsis.zip \
  -P 'RWQ5l+W1jKXGqdNMnGRzgTdrAGl9xu+fRs0CSQnQb+h5MMb1B2pvOfKj'
```

The key above is worth exactly as much as the channel you read it over: if this
page could be rewritten, so could the key printed on it. It is most useful when
you compare it against the copy you already have — inside an app you installed
earlier, or in the repository history.

## Opening an unsigned build

**Windows.** SmartScreen shows "Windows protected your PC" for an unknown
publisher. To continue: **More info** → **Run anyway**. The warning is
reputation-based, so it fades as more people install the same signed-by-nobody
build, and it returns whenever the file changes.

**macOS.** Gatekeeper refuses the app because it is neither signed with a
Developer ID nor notarized. After dragging Sotto to Applications:

1. Open it once — macOS refuses and offers only **Done**.
2. **System Settings** → **Privacy & Security**, scroll to the message about
   Sotto being blocked, and press **Open Anyway**.
3. Confirm in the dialog that follows.

The right-click → **Open** shortcut still works on older macOS versions; on
current ones the Privacy & Security route is the reliable one.

Both of these are the operating system doing its job. They will stop appearing
only when the builds are signed — see the code-signing checklist in
[Release process](RELEASE.md).
