import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { stageLinuxRelease } from "../scripts/stage_linux_release.mjs";

function packages() {
  const temp = mkdtempSync(join(tmpdir(), "magic-renamer-release-"));
  const bundleRoot = join(temp, "bundle");
  mkdirSync(join(bundleRoot, "deb"), { recursive: true });
  mkdirSync(join(bundleRoot, "rpm"), { recursive: true });
  const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
  writeFileSync(join(bundleRoot, "deb", `MagicRenamer_${version}_amd64.deb`), "deb package contents");
  writeFileSync(join(bundleRoot, "rpm", `MagicRenamer-${version}.x86_64.rpm`), "rpm package contents");
  const query = (command, args) => {
    if (command === "dpkg-deb") {
      if (args[0] === "--contents") return "/usr/lib/magicrenamer/licenses/LICENSE-MagicRenamer\n/usr/lib/magicrenamer/licenses/THIRD_PARTY_NOTICES.md\n/usr/lib/magicrenamer/licenses/THIRD_PARTY_LICENSE_TEXTS.md";
      return args[2] === "Architecture" ? "amd64" : version;
    }
    if (args[0] === "-qpl") return "/usr/lib/magicrenamer/licenses/LICENSE-MagicRenamer\n/usr/lib/magicrenamer/licenses/THIRD_PARTY_NOTICES.md\n/usr/lib/magicrenamer/licenses/THIRD_PARTY_LICENSE_TEXTS.md";
    return args[2] === "%{ARCH}" ? "x86_64" : version;
  };
  return { temp, bundleRoot, query, version };
}

test("stages one verified DEB and RPM with checksums in versioned release folders", () => {
  const fixture = packages();
  try {
    const { stage } = stageLinuxRelease(fixture.temp, { bundleRoot: fixture.bundleRoot, query: fixture.query, outputFile: null });
    for (const [folder, file, content] of [
      ["linux-deb", `MagicRenamer_${fixture.version}_amd64.deb`, "deb package contents"],
      ["linux-rpm", `MagicRenamer-${fixture.version}.x86_64.rpm`, "rpm package contents"],
    ]) {
      assert.equal(readFileSync(join(stage, folder, file), "utf8"), content);
      const hash = createHash("sha256").update(content).digest("hex");
      assert.match(readFileSync(join(stage, "SHA256SUMS"), "utf8"), new RegExp(`${hash}  ${folder}/${file.replaceAll(".", "\\.")}`));
    }
  } finally {
    rmSync(fixture.temp, { recursive: true, force: true });
  }
});

test("rejects a mismatched tag and packages missing license notices", () => {
  const fixture = packages();
  try {
    assert.throws(() => stageLinuxRelease(fixture.temp, {
      bundleRoot: fixture.bundleRoot, query: fixture.query, refType: "tag", refName: "v99.0.0", outputFile: null,
    }), /does not match/);
    const query = (command, args) => args[0] === "-qpl" ? "/usr/bin/magic-renamer" : fixture.query(command, args);
    assert.throws(() => stageLinuxRelease(fixture.temp, {
      bundleRoot: fixture.bundleRoot, query, outputFile: null,
    }), /RPM is missing bundled LICENSE/);
  } finally {
    rmSync(fixture.temp, { recursive: true, force: true });
  }
});
