import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const defaultBundleRoot = join(root, "src-tauri/target/release/bundle");
const defaultQuery = (command, args) => execFileSync(command, args, { encoding: "utf8" }).trim();

export function stageLinuxRelease(destination, {
  bundleRoot = defaultBundleRoot,
  query = defaultQuery,
  refType = process.env.GITHUB_REF_TYPE,
  refName = process.env.GITHUB_REF_NAME,
  outputFile = process.env.GITHUB_OUTPUT,
} = {}) {
  if (!destination) throw new Error("Pass a staging root directory");
  const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  const tauriVersion = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")).version;
  const cargoVersion = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version) || version !== tauriVersion || version !== cargoVersion) {
    throw new Error("package.json, tauri.conf.json, and Cargo.toml must have the same release version");
  }
  if (refType === "tag" && refName !== `v${version}`) {
    throw new Error(`Release tag ${refName} does not match v${version}`);
  }

  function singlePackage(format) {
    const folder = join(bundleRoot, format);
    const matches = existsSync(folder) ? readdirSync(folder).filter((name) => name.endsWith(`.${format}`)) : [];
    if (matches.length !== 1) throw new Error(`Expected one ${format.toUpperCase()} in ${folder}; found ${matches.length}`);
    return join(folder, matches[0]);
  }

  const deb = singlePackage("deb");
  const rpm = singlePackage("rpm");
  const checks = [
    ["DEB version", query("dpkg-deb", ["-f", deb, "Version"]), version],
    ["DEB architecture", query("dpkg-deb", ["-f", deb, "Architecture"]), "amd64"],
    ["RPM version", query("rpm", ["-qp", "--qf", "%{VERSION}", rpm]), version],
    ["RPM architecture", query("rpm", ["-qp", "--qf", "%{ARCH}", rpm]), "x86_64"],
  ];
  for (const [label, actual, expected] of checks) {
    if (actual.trim() !== expected) throw new Error(`${label}: expected ${expected}, found ${actual.trim()}`);
  }
  for (const [format, file, command, args] of [
    ["DEB", deb, "dpkg-deb", ["--contents", deb]],
    ["RPM", rpm, "rpm", ["-qpl", rpm]],
  ]) {
    const contents = query(command, args);
    for (const notice of ["LICENSE-MagicRenamer", "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_LICENSE_TEXTS.md"]) {
      if (!contents.includes(notice)) throw new Error(`${format} is missing bundled ${notice}: ${file}`);
    }
  }

  const stage = join(resolve(destination), "releases", `v${version}`);
  const packages = [["linux-deb", deb], ["linux-rpm", rpm]];
  const checksums = [];
  for (const [target, source] of packages) {
    const folder = join(stage, target);
    mkdirSync(folder, { recursive: true });
    const name = basename(source);
    copyFileSync(source, join(folder, name));
    const hash = createHash("sha256").update(readFileSync(source)).digest("hex");
    checksums.push(`${hash}  ${target}/${name}`);
  }
  writeFileSync(join(stage, "SHA256SUMS"), checksums.join("\n") + "\n");
  if (outputFile) appendFileSync(outputFile, `version=${version}\n`);
  return { stage, version };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { stage } = stageLinuxRelease(process.argv[2]);
  console.log(`Staged verified Linux DEB and RPM under ${stage}`);
}
