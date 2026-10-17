import { createHash } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const targets = {
  "x86_64-pc-windows-msvc": { folder: "windows-x64", machine: 0x8664, installerArch: "x64" },
  "aarch64-pc-windows-msvc": { folder: "windows-arm64", machine: 0xaa64, installerArch: "arm64" },
};

export function readPeMachine(executable) {
  const bytes = readFileSync(executable);
  if (bytes.length < 64 || bytes.toString("ascii", 0, 2) !== "MZ") throw new Error(`Invalid PE executable: ${executable}`);
  const offset = bytes.readUInt32LE(0x3c);
  if (offset < 64 || offset + 6 > bytes.length || bytes.toString("ascii", offset, offset + 4) !== "PE\0\0") {
    throw new Error(`Invalid PE header: ${executable}`);
  }
  return bytes.readUInt16LE(offset + 4);
}

export function stageWindowsRelease(destination, target, {
  buildRoot = join(root, "src-tauri/target", target, "release"),
  refType = process.env.GITHUB_REF_TYPE,
  refName = process.env.GITHUB_REF_NAME,
  outputFile = process.env.GITHUB_OUTPUT,
} = {}) {
  if (!destination) throw new Error("Pass a staging root directory");
  const platform = targets[target];
  if (!platform) throw new Error(`Unsupported Windows target: ${target}`);
  const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  const tauriConfig = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const cargoVersion = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version) || version !== tauriConfig.version || version !== cargoVersion) {
    throw new Error("package.json, tauri.conf.json, and Cargo.toml must have the same release version");
  }
  if (refType === "tag" && refName !== `v${version}`) throw new Error(`Release tag ${refName} does not match v${version}`);

  const executable = join(buildRoot, "magic-renamer.exe");
  const machine = readPeMachine(executable);
  if (machine !== platform.machine) throw new Error(`App executable has PE machine 0x${machine.toString(16)}; expected 0x${platform.machine.toString(16)}`);

  const bundleFolder = join(buildRoot, "bundle", "nsis");
  const installers = existsSync(bundleFolder) ? readdirSync(bundleFolder).filter((name) => name.toLowerCase().endsWith("-setup.exe")) : [];
  if (installers.length !== 1) throw new Error(`Expected one NSIS installer in ${bundleFolder}; found ${installers.length}`);
  const installerName = installers[0];
  if (!installerName.includes(version) || !installerName.toLowerCase().endsWith(`_${platform.installerArch}-setup.exe`)) {
    throw new Error(`NSIS installer filename does not match version ${version} and ${platform.installerArch}: ${installerName}`);
  }

  const expectedResources = {
    "../LICENSE": "licenses/LICENSE-MagicRenamer",
    "../THIRD_PARTY_NOTICES.md": "licenses/THIRD_PARTY_NOTICES.md",
  };
  for (const [source, destinationPath] of Object.entries(expectedResources)) {
    if (tauriConfig.bundle?.resources?.[source] !== destinationPath) throw new Error(`Tauri bundle must include ${destinationPath}`);
  }

  const folder = join(resolve(destination), "releases", `v${version}`, platform.folder);
  mkdirSync(folder, { recursive: true });
  const files = [
    [join(bundleFolder, installerName), installerName],
    [join(root, "LICENSE"), "LICENSE-MagicRenamer"],
    [join(root, "THIRD_PARTY_NOTICES.md"), "THIRD_PARTY_NOTICES.md"],
  ];
  const checksums = [];
  for (const [source, name] of files) {
    copyFileSync(source, join(folder, name));
    checksums.push(`${createHash("sha256").update(readFileSync(source)).digest("hex")}  ${name}`);
  }
  writeFileSync(join(folder, "SHA256SUMS"), checksums.join("\n") + "\n");
  if (outputFile) appendFileSync(outputFile, `version=${version}\n`);
  return { folder, version };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { folder } = stageWindowsRelease(process.argv[2], process.argv[3]);
  console.log(`Staged verified Windows NSIS installer under ${folder}`);
}
