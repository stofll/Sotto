import { describe, expect, it } from "vitest";
import frontendLock from "../../pnpm-lock.yaml?raw";
import applicationLock from "../../src-tauri/Cargo.lock?raw";
import setupLock from "../../setup/src-tauri/Cargo.lock?raw";

function frontendVersion(name: string): string {
  const entry = frontendLock.split(`      '${name}':`)[1];
  const version = entry?.match(/\n        version: (\d+\.\d+\.\d+)/)?.[1];
  if (!version) throw new Error(`Missing locked frontend dependency: ${name}`);
  return version;
}

function nativeVersion(lock: string, name: string): string {
  const entry = lock.replace(/\r\n/g, "\n").split("[[package]]").find((block) =>
    block.includes(`\nname = "${name}"\n`),
  );
  const version = entry?.match(/\nversion = "(\d+\.\d+\.\d+)"/)?.[1];
  if (!version) throw new Error(`Missing locked native dependency: ${name}`);
  return version;
}

// Both native applications share the frontend packages. Tauri requires the
// core minor versions and the plugin exact versions to agree across IPC.
describe.each([
  ["application", applicationLock],
  ["setup", setupLock],
])("%s Tauri compatibility", (_name, lock) => {
  it("keeps the core API on the backend's minor version", () => {
    expect(frontendVersion("@tauri-apps/api").split(".").slice(0, 2)).toEqual(
      nativeVersion(lock, "tauri").split(".").slice(0, 2),
    );
  });

  it("keeps the dialog plugin on the backend's exact version", () => {
    expect(frontendVersion("@tauri-apps/plugin-dialog")).toBe(
      nativeVersion(lock, "tauri-plugin-dialog"),
    );
  });
});
