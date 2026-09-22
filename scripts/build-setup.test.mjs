import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { validatePayload, parseArgs } from "./build-setup.mjs";

test("requires explicit payload, digest and destination", () => {
  assert.throws(() => parseArgs(["--payload", "file.exe"]));
  assert.throws(() => parseArgs(["--payload", "--output"]));
  assert.throws(() => parseArgs(["--payload", "a", "--payload", "b"]));
  assert.deepEqual(parseArgs(["--payload", "a b.exe", "--sha256", "abc", "--output", "out.exe"]),
    { "--payload": "a b.exe", "--sha256": "abc", "--output": "out.exe" });
});
test("rejects another version or damaged package before building", () => {
  const dir = mkdtempSync(join(tmpdir(), "sotto-setup-test-"));
  try {
    const file = join(dir, "Sotto_1.2.3_x64-setup.exe");
    const bytes = Buffer.concat([Buffer.from("MZ-synthetic-fixture"), Buffer.from("SottoSetupOptionsV1", "utf16le")]);
    writeFileSync(file, bytes);
    const hash = createHash("sha256").update(bytes).digest("hex");
    validatePayload(file, "1.2.3", hash);
    assert.throws(() => validatePayload(file, "1.2.4", hash), /version/);
    assert.throws(() => validatePayload(file, "1.2.3", "0".repeat(64)), /mismatch/);
    const old = Buffer.from("MZ-old-package");
    writeFileSync(file, old);
    assert.throws(() => validatePayload(file, "1.2.3", createHash("sha256").update(old).digest("hex")), /setup options/);
    writeFileSync(file, "not an executable");
    assert.throws(() => validatePayload(file, "1.2.3", hash), /executable/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
