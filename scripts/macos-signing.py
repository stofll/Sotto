#!/usr/bin/env python3
"""Persistent, self-signed identity for local macOS builds (not Developer ID).

The password is passed to `security` through argv (`-p`, `-P`, `-k`), where any
process owned by this user can read it out of `ps`. That is deliberate: the
`security` CLI accepts no password over env or stdin, unlike `openssl`, which
reads it here from SOTTO_SIGN_PASSWORD. The identity is self-signed and local,
nothing outside this machine trusts it, and the password sits next to it in
plain text at `<identity-dir>/password` anyway — a non-interactive build has no
other way to unlock the keychain. What protects the key is the 0700 directory,
not the secrecy of the password. Developer ID needs a different path entirely:
a key in the login keychain and notarytool, not this script.
"""

import argparse
import os
import re
import secrets
import shlex
import subprocess
import sys
from contextlib import contextmanager
from pathlib import Path


def run(args, password=None):
    env = os.environ.copy()
    if password is not None:
        env["SOTTO_SIGN_PASSWORD"] = password
    # Not `check=True`: the failure is raised below with the password redacted
    # out of the message, which CalledProcessError would print verbatim.
    result = subprocess.run(args, env=env, capture_output=True, text=True, check=False)
    if result.returncode:
        message = result.stderr.strip() or result.stdout.strip()
        if password:
            message = message.replace(password, "[redacted]")
        raise RuntimeError(f"{Path(args[0]).name}: {message}")
    return result.stdout


@contextmanager
def signing_keychain(path):
    # codesign also searches for the private key through the user's search list,
    # even with --keychain. Restore the original list, including on failure.
    original = shlex.split(run(["security", "list-keychains", "-d", "user"]))
    try:
        run(["security", "list-keychains", "-d", "user", "-s", *original, str(path)])
        yield
    finally:
        run(["security", "list-keychains", "-d", "user", "-s", *original])


def initialize(root):
    if root.exists() and any(root.iterdir()):
        raise RuntimeError("Identity directory is not empty; refusing to replace a signing key")
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    root.chmod(0o700)
    os.umask(0o077)
    password = secrets.token_urlsafe(40)
    (root / "password").write_text(password + "\n")
    run([
        "/usr/bin/openssl", "req", "-x509", "-newkey", "rsa:3072",
        "-keyout", str(root / "key.pem"), "-out", str(root / "certificate.pem"),
        "-days", "3650", "-subj", "/CN=Sotto Local Code Signing/",
        "-passout", "env:SOTTO_SIGN_PASSWORD",
        "-addext", "keyUsage=critical,digitalSignature",
        "-addext", "extendedKeyUsage=codeSigning",
        "-addext", "basicConstraints=critical,CA:FALSE",
    ], password)
    run([
        "/usr/bin/openssl", "pkcs12", "-export", "-inkey", str(root / "key.pem"),
        "-in", str(root / "certificate.pem"), "-out", str(root / "identity.p12"),
        "-name", "Sotto Local Code Signing", "-passin", "env:SOTTO_SIGN_PASSWORD",
        "-passout", "env:SOTTO_SIGN_PASSWORD",
    ], password)
    # The p12 carries the key from here on; a second copy of it would only
    # outlive a failure below and sit next to its own password.
    (root / "key.pem").unlink()
    keychain = root / "signing.keychain-db"
    original = shlex.split(run(["security", "list-keychains", "-d", "user"]))
    try:
        run(["security", "create-keychain", "-p", password, str(keychain)], password)
        run(["security", "unlock-keychain", "-p", password, str(keychain)], password)
        run([
            "security", "import", str(root / "identity.p12"), "-k", str(keychain),
            "-P", password, "-T", "/usr/bin/codesign",
        ], password)
        # Without this call the key stays partitioned to nothing and every
        # signing run raises a GUI prompt, which a build cannot answer. `apple:`
        # is the entry `man security` names for /usr/bin/codesign; `codesign:`
        # is carried only to match the list the release workflow sets.
        run([
            "security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
            "-s", "-k", password, str(keychain),
        ], password)
    finally:
        run(["security", "list-keychains", "-d", "user", "-s", *original])
    print(f"Identity created in {root}. Back up this directory securely; do not regenerate it.")


def sign(root, app):
    if not app.is_dir() or app.suffix != ".app":
        raise RuntimeError("Pass an existing .app bundle")
    for name in ["certificate.pem", "password", "signing.keychain-db"]:
        if not (root / name).is_file():
            raise RuntimeError(f"Missing {name}; initialize or restore the existing identity first")
    password = (root / "password").read_text().strip()
    fingerprint = run([
        "/usr/bin/openssl", "x509", "-in", str(root / "certificate.pem"),
        "-noout", "-fingerprint", "-sha1",
    ]).split("=", 1)[1].replace(":", "").strip()
    if not re.fullmatch(r"[0-9A-Fa-f]{40}", fingerprint):
        raise RuntimeError("Invalid certificate fingerprint")
    keychain = root / "signing.keychain-db"
    run(["security", "unlock-keychain", "-p", password, str(keychain)], password)
    with signing_keychain(keychain):
        run([
            "codesign", "--force", "--sign", fingerprint, "--keychain", str(keychain),
            "--timestamp=none", "--preserve-metadata=identifier,entitlements,flags", str(app),
        ])
    run(["codesign", "--verify", "--deep", "--strict", str(app)])
    print(f"Signed {app} with certificate {fingerprint}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity-dir", type=Path, default=Path.home() / ".tauri/sotto-local-signing")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("init", help="Create once; refuses to replace any existing identity")
    signer = sub.add_parser("sign", help="Reuse the existing identity to sign an app")
    signer.add_argument("app", type=Path)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("macOS is required")
    try:
        if args.command == "init":
            initialize(args.identity_dir.resolve())
        else:
            sign(args.identity_dir.resolve(), args.app.resolve())
    except (RuntimeError, OSError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
