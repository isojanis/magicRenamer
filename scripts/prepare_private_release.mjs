import { createHash } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const platforms = [
  ["linux-deb", "linux", ".deb"],
  ["linux-rpm", "linux", ".rpm"],
  ["windows-x64", "windows-x64", "_x64-setup.exe"],
  ["windows-arm64", "windows-arm64", "_arm64-setup.exe"],
  ["macos-apple-silicon", "macos-apple-silicon", "_aarch64.dmg"],
  ["macos-intel", "macos-intel", "_x64.dmg"],
];

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function checkSum(folder, name, expectedPath, file) {
  const lines = readFileSync(join(folder, "SHA256SUMS"), "utf8").trim().split(/\r?\n/);
  const matches = lines.filter((line) => line.endsWith(`  ${expectedPath}`));
  if (matches.length !== 1 || matches[0] !== `${sha256(file)}  ${expectedPath}`) {
    throw new Error(`Invalid SHA256SUMS entry for ${name}`);
  }
}

export function preparePrivateRelease(destination, {
  artifactRoot,
  outputFile = process.env.GITHUB_OUTPUT,
} = {}) {
  if (!destination || !artifactRoot) throw new Error("Pass a checkout directory and downloaded artifact root");
  const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  const tauriVersion = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")).version;
  const cargoVersion = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version) || version !== tauriVersion || version !== cargoVersion) {
    throw new Error("package.json, tauri.conf.json, and Cargo.toml must have the same release version");
  }
  const releaseRoot = join(resolve(destination), "releases", `v${version}`);
  const assets = join(resolve(destination), "release-assets", `v${version}`);
  const selected = [];
  for (const [platform, sourceFolder, suffix] of platforms) {
    const folder = join(resolve(artifactRoot), sourceFolder);
    const packageFolder = sourceFolder === "linux" ? join(folder, platform) : folder;
    const matches = existsSync(packageFolder) ? readdirSync(packageFolder).filter((name) => name.endsWith(suffix)) : [];
    if (matches.length !== 1 || !matches[0].includes(version)) {
      throw new Error(`Expected one ${platform} package for version ${version}; found ${matches.length}`);
    }
    const name = matches[0];
    const file = join(packageFolder, name);
    const checksumPath = sourceFolder === "linux" ? `${platform}/${name}` : name;
    checkSum(folder, name, checksumPath, file);
    if (statSync(file).size >= 100 * 1024 * 1024) {
      throw new Error(`${name} is too large for regular Git; use Git LFS or release assets only`);
    }
    if (sourceFolder !== "linux") {
      for (const notice of ["LICENSE-MagicRenamer", "THIRD_PARTY_NOTICES.md"]) {
        const sidecar = join(folder, notice);
        checkSum(folder, notice, notice, sidecar);
      }
    }
    selected.push({ platform, sourceFolder, packageFolder, name, file });
  }
  mkdirSync(assets, { recursive: true });
  const globalChecksums = [];
  for (const { platform, sourceFolder, packageFolder, name, file } of selected) {
    const target = join(releaseRoot, platform);
    mkdirSync(target, { recursive: true });
    rmSync(join(target, ".gitkeep"), { force: true });
    copyFileSync(file, join(target, name));
    copyFileSync(file, join(assets, name));
    if (sourceFolder !== "linux") {
      for (const notice of ["LICENSE-MagicRenamer", "THIRD_PARTY_NOTICES.md"]) {
        copyFileSync(join(packageFolder, notice), join(target, notice));
      }
    }
    globalChecksums.push(`${sha256(file)}  ${platform}/${name}`);
  }
  writeFileSync(join(releaseRoot, "SHA256SUMS"), globalChecksums.join("\n") + "\n");
  writeFileSync(join(assets, "SHA256SUMS"), selected.map(({ name, file }) => `${sha256(file)}  ${name}`).join("\n") + "\n");
  copyFileSync(join(root, "LICENSE"), join(assets, "LICENSE-MagicRenamer"));
  if (outputFile) appendFileSync(outputFile, `version=${version}\n`);
  return { releaseRoot, assets, version };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { releaseRoot, assets } = preparePrivateRelease(process.argv[2], { artifactRoot: process.argv[3] });
  console.log(`Prepared ${releaseRoot} and draft release assets in ${assets}`);
}
