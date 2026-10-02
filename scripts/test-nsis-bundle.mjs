// Compile the production installer around an inert executable, then exercise isolated NSIS options.
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

if (process.platform !== "win32") throw new Error("NSIS regression requires Windows");
const root = fileURLToPath(new URL("../", import.meta.url));
const temporary = mkdtempSync(join(tmpdir(), "sotto-nsis-bundle-"));
try {
  const target = join(temporary, "target");
  const release = join(target, "release");
  mkdirSync(release, { recursive: true });
  const source = join(temporary, "fixture.rs");
  writeFileSync(source, "fn main() {}\n");
  execFileSync("rustc", ["--crate-name", "nsis_fixture", source, "-o", join(release, "Sotto.exe")], { stdio: "inherit" });
  const config = join(temporary, "tauri.fixture.json");
  writeFileSync(config, JSON.stringify({
    productName: "Sotto NSIS fixture", identifier: "com.sotto.nsis-fixture",
    build: { beforeBundleCommand: "" },
    bundle: { createUpdaterArtifacts: false, resources: [], windows: { nsis: { compression: "none" } } },
  }));
  const env = { ...process.env, CARGO_TARGET_DIR: target };
  delete env.CARGO_BUILD_TARGET;
  delete env.TAURI_SIGNING_PRIVATE_KEY;
  delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
  execFileSync(process.execPath, [join(root, "desktop/node_modules/@tauri-apps/cli/tauri.js"), "bundle", "--bundles", "nsis", "--config", config], {
    cwd: join(root, "desktop"), env, stdio: "inherit",
  });
  const generated = join(release, "nsis/x64");
  const nsis = join(process.env.LOCALAPPDATA, "tauri/NSIS/makensis.exe");
  if (!readFileSync(join(generated, "utils.nsh"), "utf8").includes("RestartManager_StartSession")) {
    throw new Error("Pinned bundler did not generate Restart Manager helpers");
  }
  execFileSync("python", [join(root, "scripts/test-setup-options.py"), "--makensis", nsis, "--utils", join(generated, "utils.nsh")], {
    cwd: root, stdio: "inherit",
  });
} finally {
  if (!resolve(temporary).startsWith(resolve(tmpdir()) + sep)) throw new Error("Unexpected temporary directory");
  rmSync(temporary, { recursive: true, force: true });
}
