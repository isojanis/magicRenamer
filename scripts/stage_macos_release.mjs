import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const targets = {
  "aarch64-apple-darwin": { folder: "macos-apple-silicon", arch: "arm64", suffix: "aarch64" },
  "x86_64-apple-darwin": { folder: "macos-intel", arch: "x86_64", suffix: "x64" },
};
const defaultQuery = (command, args) => execFileSync(command, args, { encoding: "utf8" }).trim();

export function stageMacosRelease(destination, target, {
  buildRoot = join(root, "src-tauri/target", target, "release"),
  appFolder,
  query = defaultQuery,
  refType = process.env.GITHUB_REF_TYPE,
  refName = process.env.GITHUB_REF_NAME,
  outputFile = process.env.GITHUB_OUTPUT,
} = {}) {
  if (!destination) throw new Error("Pass a staging root directory");
  const platform = targets[target];
  if (!platform) throw new Error(`Unsupported macOS target: ${target}`);
  const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  const tauriConfig = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const cargoVersion = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version) || version !== tauriConfig.version || version !== cargoVersion) {
    throw new Error("package.json, tauri.conf.json, and Cargo.toml must have the same release version");
  }
  if (refType === "tag" && refName !== `v${version}`) throw new Error(`Release tag ${refName} does not match v${version}`);

  const bundleRoot = join(buildRoot, "bundle");
  const dmgFolder = join(bundleRoot, "dmg");
  const dmgs = existsSync(dmgFolder) ? readdirSync(dmgFolder).filter((name) => name.endsWith(".dmg")) : [];
  if (dmgs.length !== 1) throw new Error(`Expected one DMG in ${dmgFolder}; found ${dmgs.length}`);
  const name = dmgs[0];
  if (!name.includes(version) || !name.endsWith(`_${platform.suffix}.dmg`)) {
    throw new Error(`DMG filename does not match version ${version} and ${platform.suffix}: ${name}`);
  }
  const dmg = join(dmgFolder, name);
  query("hdiutil", ["verify", dmg]);
  const verifiedApp = appFolder ?? join(bundleRoot, "macos", `${tauriConfig.productName}.app`);
  const infoPlist = join(verifiedApp, "Contents", "Info.plist");
  if (!existsSync(infoPlist)) throw new Error(`Missing macOS application bundle: ${verifiedApp}`);
  const bundleVersion = query("plutil", ["-extract", "CFBundleShortVersionString", "raw", "-o", "-", infoPlist]);
  if (bundleVersion !== version) throw new Error(`macOS app version: expected ${version}, found ${bundleVersion}`);
  const executableName = query("plutil", ["-extract", "CFBundleExecutable", "raw", "-o", "-", infoPlist]);
  if (!executableName || executableName.includes("/") || executableName.includes("\\")) throw new Error("Invalid CFBundleExecutable");
  const executable = join(verifiedApp, "Contents", "MacOS", executableName);
  const arches = query("lipo", ["-archs", executable]);
  if (arches !== platform.arch) throw new Error(`macOS app architecture: expected ${platform.arch}, found ${arches}`);

  const resources = {
    "../LICENSE": "licenses/LICENSE-MagicRenamer",
    "../THIRD_PARTY_NOTICES.md": "licenses/THIRD_PARTY_NOTICES.md",
  };
  for (const [source, path] of Object.entries(resources)) {
    if (tauriConfig.bundle?.resources?.[source] !== path) throw new Error(`Tauri bundle must include ${path}`);
    const bundled = join(verifiedApp, "Contents", "Resources", path);
    if (!existsSync(bundled)) throw new Error(`macOS app is missing bundled ${path}`);
    if (!readFileSync(bundled).equals(readFileSync(resolve(root, "src-tauri", source)))) {
      throw new Error(`macOS app has mismatched bundled ${path}`);
    }
  }

  const folder = join(resolve(destination), "releases", `v${version}`, platform.folder);
  mkdirSync(folder, { recursive: true });
  const files = [
    [dmg, name],
    [join(root, "LICENSE"), "LICENSE-MagicRenamer"],
    [join(root, "THIRD_PARTY_NOTICES.md"), "THIRD_PARTY_NOTICES.md"],
  ];
  const checksums = [];
  for (const [source, filename] of files) {
    copyFileSync(source, join(folder, filename));
    checksums.push(`${createHash("sha256").update(readFileSync(source)).digest("hex")}  ${filename}`);
  }
  writeFileSync(join(folder, "SHA256SUMS"), checksums.join("\n") + "\n");
  if (outputFile) appendFileSync(outputFile, `version=${version}\n`);
  return { folder, version };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { folder } = stageMacosRelease(process.argv[2], process.argv[3], { appFolder: process.argv[4] });
  console.log(`Staged verified macOS DMG under ${folder}`);
}
