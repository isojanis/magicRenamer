import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { readPeMachine, stageWindowsRelease } from "../scripts/stage_windows_release.mjs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;

function fixture(machine, installerArch) {
  const temp = mkdtempSync(join(tmpdir(), "magic-renamer-windows-release-"));
  const buildRoot = join(temp, "build");
  const nsis = join(buildRoot, "bundle", "nsis");
  mkdirSync(nsis, { recursive: true });
  const exe = Buffer.alloc(128);
  exe.write("MZ", 0);
  exe.writeUInt32LE(64, 0x3c);
  exe.write("PE\0\0", 64);
  exe.writeUInt16LE(machine, 68);
  writeFileSync(join(buildRoot, "magic-renamer.exe"), exe);
  const installerName = `MagicRenamer_${version}_${installerArch}-setup.exe`;
  writeFileSync(join(nsis, installerName), "installer contents");
  return { temp, buildRoot, installerName };
}

test("stages a native Windows installer with notices and checksums", () => {
  for (const [target, machine, arch, folderName] of [
    ["x86_64-pc-windows-msvc", 0x8664, "x64", "windows-x64"],
    ["aarch64-pc-windows-msvc", 0xaa64, "arm64", "windows-arm64"],
  ]) {
    const f = fixture(machine, arch);
    try {
      const { folder } = stageWindowsRelease(f.temp, target, { buildRoot: f.buildRoot, outputFile: null });
      assert.equal(folder, join(f.temp, "releases", `v${version}`, folderName));
      assert.equal(readFileSync(join(folder, f.installerName), "utf8"), "installer contents");
      assert.ok(readFileSync(join(folder, "LICENSE-MagicRenamer"), "utf8").includes("MIT License"));
      assert.ok(readFileSync(join(folder, "THIRD_PARTY_NOTICES.md"), "utf8").length > 0);
      const checksum = createHash("sha256").update("installer contents").digest("hex");
      assert.ok(readFileSync(join(folder, "SHA256SUMS"), "utf8").includes(`${checksum}  ${f.installerName}`));
    } finally {
      rmSync(f.temp, { recursive: true, force: true });
    }
  }
});

test("rejects mismatched tag, wrong app architecture, and invalid PE headers", () => {
  const f = fixture(0x8664, "x64");
  try {
    assert.throws(() => stageWindowsRelease(f.temp, "x86_64-pc-windows-msvc", {
      buildRoot: f.buildRoot, refType: "tag", refName: "v99.0.0", outputFile: null,
    }), /does not match/);
    assert.throws(() => stageWindowsRelease(f.temp, "aarch64-pc-windows-msvc", {
      buildRoot: f.buildRoot, outputFile: null,
    }), /PE machine/);
    writeFileSync(join(f.buildRoot, "magic-renamer.exe"), "not a PE");
    assert.throws(() => readPeMachine(join(f.buildRoot, "magic-renamer.exe")), /Invalid PE/);
  } finally {
    rmSync(f.temp, { recursive: true, force: true });
  }
});
