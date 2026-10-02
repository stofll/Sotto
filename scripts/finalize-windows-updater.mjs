import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function withSetupUpdater(manifest, { version, repository, signature, executable }) {
  if (manifest.version !== version) throw new Error("Updater manifest version mismatch");
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error("Invalid repository");
  if (!executable.subarray(0, 2).equals(Buffer.from("MZ"))) throw new Error("Setup executable is missing or invalid");
  const decoded = Buffer.from(signature.trim(), "base64").toString("utf8");
  if (!decoded.startsWith("untrusted comment:") || !decoded.includes("trusted comment:")) {
    throw new Error("Setup signature is missing or invalid");
  }
  const platforms = { ...manifest.platforms };
  const keys = ["windows-x86_64", "windows-x86_64-nsis"].filter(key => platforms[key]);
  if (!keys.length) throw new Error("Manifest has no Windows x64 NSIS target");
  const filename = `Sotto_${version}_x64-setup-ui.exe`;
  for (const key of keys) {
    platforms[key] = {
      ...platforms[key],
      url: `https://github.com/${repository}/releases/download/v${version}/${filename}`,
      signature: signature.trim(),
    };
  }
  return { ...manifest, platforms };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [directory, version, repository] = process.argv.slice(2);
  if (!directory || !version || !repository) throw new Error("Usage: node scripts/finalize-windows-updater.mjs <assets> <version> <owner/repo>");
  const manifestPath = join(directory, "latest.json");
  const setup = join(directory, `Sotto_${version}_x64-setup-ui.exe`);
  const manifest = withSetupUpdater(JSON.parse(readFileSync(manifestPath, "utf8")), {
    version, repository,
    signature: readFileSync(`${setup}.sig`, "utf8"), executable: readFileSync(setup),
  });
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
}
