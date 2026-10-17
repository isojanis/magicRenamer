import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { stageMacosRelease } from "../scripts/stage_macos_release.mjs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;

function fixture(arch, suffix) {
  const temp = mkdtempSync(join(tmpdir(), "magic-renamer-macos-release-"));
  const buildRoot = join(temp, "build");
  const contents = join(buildRoot, "bundle", "macos", "MagicRenamer.app", "Contents");
  mkdirSync(join(contents, "MacOS"), { recursive: true });
  mkdirSync(join(contents, "Resources", "licenses"), { recursive: true });
  writeFileSync(join(contents, "Info.plist"), "fixture plist");
  writeFileSync(join(contents, "MacOS", "magic-renamer"), "fixture binary");
  for (const [source, name] of [["LICENSE", "LICENSE-MagicRenamer"], ["THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md"]]) {
    copyFileSync(new URL(`../${source}`, import.meta.url), join(contents, "Resources", "licenses", name));
  }
  const dmgFolder = join(buildRoot, "bundle", "dmg");
  mkdirSync(dmgFolder, { recursive: true });
  const dmgName = `MagicRenamer_${version}_${suffix}.dmg`;
  writeFileSync(join(dmgFolder, dmgName), "dmg contents");
  const query = (command, args) => {
    if (command === "plutil") return args[1] === "CFBundleExecutable" ? "magic-renamer" : version;
    if (command === "lipo") return arch;
    if (command === "hdiutil") return "verified";
    throw new Error(`Unexpected command ${command}`);
  };
  return { temp, buildRoot, dmgName, query, contents };
}

test("stages verified Apple Silicon and Intel DMGs with bundled notices", () => {
  for (const [target, arch, suffix, folderName] of [
    ["aarch64-apple-darwin", "arm64", "aarch64", "macos-apple-silicon"],
    ["x86_64-apple-darwin", "x86_64", "x64", "macos-intel"],
  ]) {
    const f = fixture(arch, suffix);
    try {
      const { folder } = stageMacosRelease(f.temp, target, { buildRoot: f.buildRoot, query: f.query, outputFile: null });
      assert.equal(folder, join(f.temp, "releases", `v${version}`, folderName));
      assert.equal(readFileSync(join(folder, f.dmgName), "utf8"), "dmg contents");
      const checksum = createHash("sha256").update("dmg contents").digest("hex");
      assert.ok(readFileSync(join(folder, "SHA256SUMS"), "utf8").includes(`${checksum}  ${f.dmgName}`));
      assert.ok(readFileSync(join(folder, "LICENSE-MagicRenamer"), "utf8").includes("MIT License"));
    } finally {
      rmSync(f.temp, { recursive: true, force: true });
    }
  }
});

test("rejects wrong architecture, mismatched tags, and missing bundled notices", () => {
  const f = fixture("arm64", "aarch64");
  try {
    const options = { buildRoot: f.buildRoot, query: f.query, outputFile: null };
    assert.throws(() => stageMacosRelease(f.temp, "x86_64-apple-darwin", options), /architecture/);
    assert.throws(() => stageMacosRelease(f.temp, "aarch64-apple-darwin", {
      ...options, refType: "tag", refName: "v99.0.0",
    }), /does not match/);
    rmSync(join(f.contents, "Resources", "licenses", "THIRD_PARTY_NOTICES.md"));
    assert.throws(() => stageMacosRelease(f.temp, "aarch64-apple-darwin", options), /missing bundled/);
  } finally {
    rmSync(f.temp, { recursive: true, force: true });
  }
});
