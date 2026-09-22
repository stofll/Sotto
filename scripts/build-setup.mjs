import { createHash } from "node:crypto";
import { readFileSync, mkdirSync, copyFileSync, existsSync } from "node:fs";
import { dirname, basename, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { homedir } from "node:os";

const root = fileURLToPath(new URL("../", import.meta.url));

export function validatePayload(payload, version, expectedHash) {
  if (basename(payload) !== `Sotto_${version}_x64-setup.exe`) {
    throw new Error("Payload filename must match the application version and x64 NSIS target");
  }
  if (!/^[a-f0-9]{64}$/i.test(expectedHash ?? "")) throw new Error("--sha256 is required");
  const bytes = readFileSync(payload);
  if (bytes.subarray(0, 2).toString() !== "MZ") throw new Error("Payload is not a Windows executable");
  if (createHash("sha256").update(bytes).digest("hex") !== expectedHash.toLowerCase()) {
    throw new Error("Payload SHA-256 mismatch");
  }
  if (!bytes.includes(Buffer.from("SottoSetupOptionsV1", "utf16le"))) {
    throw new Error("Payload does not support setup options; rebuild NSIS from the current source");
  }
}

export function parseArgs(args) {
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    if (!["--payload", "--sha256", "--output"].includes(args[i]) || !args[i + 1] || args[i + 1].startsWith("--")) {
      throw new Error("Usage: pnpm setup:package --payload <NSIS.exe> --sha256 <hash> --output <Setup.exe>");
    }
    if (options[args[i]]) throw new Error("Duplicate argument");
    options[args[i]] = args[i + 1];
  }
  for (const key of ["--payload", "--sha256", "--output"]) {
    if (!options[key]) throw new Error(`Missing ${key}`);
  }
  return options;
}

function main() {
  if (process.platform !== "win32") throw new Error("Sotto Setup is built on Windows");
  const args = parseArgs(process.argv.slice(2));
  const payload = resolve(args["--payload"]);
  const output = resolve(args["--output"]);
  if (payload === output || existsSync(output)) throw new Error("Output must be a new file, separate from the NSIS payload");
  const { version } = JSON.parse(readFileSync(join(root, "desktop/package.json"), "utf8"));
  validatePayload(payload, version, args["--sha256"]);
  const pnpm = process.env.npm_execpath;
  if (!pnpm) throw new Error("Run through pnpm setup:package");
  execFileSync(process.execPath, [pnpm, "setup:build"], { cwd: join(root, "desktop"), stdio: "inherit" });
  const native = join(root, "desktop/setup/src-tauri");
  // Isolate setup artifacts from any caller's application/GPU target directory.
  const target = join(native, "target");
  const cargoHome = resolve(process.env.CARGO_HOME || join(homedir(), ".cargo"));
  const inheritedFlags = process.env.CARGO_ENCODED_RUSTFLAGS?.split("\x1f")
    ?? process.env.RUSTFLAGS?.trim().split(/\s+/).filter(Boolean) ?? [];
  const rustFlags = [...inheritedFlags,
    `--remap-path-prefix=${cargoHome}=/cargo`,
    `--remap-path-prefix=${root.replace(/[\\/]$/, "")}=/build`,
  ].join("\x1f");
  execFileSync("cargo", ["build", "--release", "--locked", "--features", "custom-protocol", "--target", "x86_64-pc-windows-msvc"], {
    cwd: native, stdio: "inherit",
    env: { ...process.env, CARGO_TARGET_DIR: target, CARGO_ENCODED_RUSTFLAGS: rustFlags, SOTTO_SETUP_PAYLOAD: payload },
  });
  mkdirSync(dirname(output), { recursive: true });
  copyFileSync(join(target, "x86_64-pc-windows-msvc/release/sotto-setup.exe"), output);
  console.log(`Sotto Setup: ${output}`);
  console.log(`SHA-256: ${createHash("sha256").update(readFileSync(output)).digest("hex")}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
