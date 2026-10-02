import { test } from "node:test";
import assert from "node:assert/strict";
import { withSetupUpdater } from "./finalize-windows-updater.mjs";

const input = {
  version: "1.2.3-beta.2", notes: "Release notes", pub_date: "2026-10-02T00:00:00Z",
  platforms: {
    "windows-x86_64": { url: "old.exe", signature: "old" },
    "windows-x86_64-nsis": { url: "old.exe", signature: "old" },
    "darwin-aarch64": { url: "mac.tar.gz", signature: "mac" },
  },
};
const options = {
  version: input.version, repository: "stofll/Sotto", executable: Buffer.from("MZ-fixture"),
  signature: Buffer.from("untrusted comment: fixture\nunsigned fixture\ntrusted comment: fixture\n").toString("base64"),
};
test("selects the signed setup for both old and new Windows clients without changing macOS or release metadata", () => {
  const result = withSetupUpdater(input, options);
  assert.equal(result.platforms["windows-x86_64"].url, "https://github.com/stofll/Sotto/releases/download/v1.2.3-beta.2/Sotto_1.2.3-beta.2_x64-setup-ui.exe");
  assert.equal(result.platforms["windows-x86_64-nsis"].signature, options.signature);
  assert.deepEqual(result.platforms["darwin-aarch64"], input.platforms["darwin-aarch64"]);
  assert.equal(result.notes, input.notes);
  assert.equal(result.pub_date, input.pub_date);
  assert.equal(input.platforms["windows-x86_64"].url, "old.exe");
  assert.deepEqual(withSetupUpdater(result, options), result);
});
test("refuses incomplete or mismatched releases", () => {
  assert.throws(() => withSetupUpdater(input, { ...options, version: "1.2.3" }), /version/);
  assert.throws(() => withSetupUpdater(input, { ...options, signature: "" }), /signature/);
  assert.throws(() => withSetupUpdater(input, { ...options, executable: Buffer.from("bad") }), /executable/);
  assert.throws(() => withSetupUpdater({ ...input, platforms: {} }, options), /target/);
});
