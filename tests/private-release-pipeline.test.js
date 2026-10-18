import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { preparePrivateRelease } from "../scripts/prepare_private_release.mjs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;

function fixture() {
  const temp = mkdtempSync(join(tmpdir(), "magic-renamer-private-release-"));
  const artifactRoot = join(temp, "incoming");
  const packages = [
    ["linux", "linux-deb", `MagicRenamer_${version}_amd64.deb`],
    ["linux", "linux-rpm", `MagicRenamer-${version}.x86_64.rpm`],
    ["windows-x64", "", `MagicRenamer_${version}_x64-setup.exe`],
    ["windows-arm64", "", `MagicRenamer_${version}_arm64-setup.exe`],
    ["macos-apple-silicon", "", `MagicRenamer_${version}_aarch64.dmg`],
    ["macos-intel", "", `MagicRenamer_${version}_x64.dmg`],
  ];
  const checksums = new Map();
  for (const [artifact, platform, name] of packages) {
    const folder = join(artifactRoot, artifact);
    const packageFolder = platform ? join(folder, platform) : folder;
    mkdirSync(packageFolder, { recursive: true });
    const bytes = `package ${name}`;
    writeFileSync(join(packageFolder, name), bytes);
    const key = artifact === "linux" ? `${platform}/${name}` : name;
    checksums.set(artifact, [...(checksums.get(artifact) ?? []), `${createHash("sha256").update(bytes).digest("hex")}  ${key}`]);
  }
  for (const artifact of ["windows-x64", "windows-arm64", "macos-apple-silicon", "macos-intel"]) {
    for (const [source, name] of [["LICENSE", "LICENSE-MagicRenamer"], ["THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md"], ["THIRD_PARTY_LICENSE_TEXTS.md", "THIRD_PARTY_LICENSE_TEXTS.md"]]) {
      const file = join(artifactRoot, artifact, name);
      copyFileSync(new URL(`../${source}`, import.meta.url), file);
      const hash = createHash("sha256").update(readFileSync(file)).digest("hex");
      checksums.set(artifact, [...checksums.get(artifact), `${hash}  ${name}`]);
    }
  }
  for (const [artifact, lines] of checksums) writeFileSync(join(artifactRoot, artifact, "SHA256SUMS"), lines.join("\n") + "\n");
  return { temp, artifactRoot, packages };
}

test("assembles six checked packages into release folders and draft assets", () => {
  const f = fixture();
  try {
    const { releaseRoot, assets } = preparePrivateRelease(f.temp, { artifactRoot: f.artifactRoot, outputFile: null });
    for (const [artifact, platform, name] of f.packages) {
      const target = platform || artifact;
      assert.ok(existsSync(join(releaseRoot, target, name)));
      assert.ok(existsSync(join(assets, name)));
      assert.ok(readFileSync(join(releaseRoot, "SHA256SUMS"), "utf8").includes(`${target}/${name}`));
    }
    assert.equal(readFileSync(join(assets, "SHA256SUMS"), "utf8").trim().split("\n").length, 6);
    assert.ok(existsSync(join(assets, "LICENSE-MagicRenamer")));
    assert.ok(existsSync(join(assets, "THIRD_PARTY_NOTICES.md")));
    assert.ok(existsSync(join(assets, "THIRD_PARTY_LICENSE_TEXTS.md")));
  } finally {
    rmSync(f.temp, { recursive: true, force: true });
  }
});

test("rejects a corrupted downloaded installer before creating release files", () => {
  const f = fixture();
  try {
    writeFileSync(join(f.artifactRoot, "windows-arm64", `MagicRenamer_${version}_arm64-setup.exe`), "changed after checksum");
    assert.throws(() => preparePrivateRelease(f.temp, { artifactRoot: f.artifactRoot, outputFile: null }), /Invalid SHA256SUMS/);
    assert.equal(existsSync(join(f.temp, "release-assets")), false);
  } finally {
    rmSync(f.temp, { recursive: true, force: true });
  }
});
