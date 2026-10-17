import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Set MAGIC_RENAMER_TARGET to the destination Rust target triple when cross-compiling.
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, "THIRD_PARTY_NOTICES.md");
const run = (command, args) => execFileSync(command, args, { encoding: "utf8", maxBuffer: 128 * 1024 * 1024 });
const target = process.env.MAGIC_RENAMER_TARGET || process.env.CARGO_BUILD_TARGET ||
  run("rustc", ["-vV"]).match(/^host: (.+)$/m)?.[1];
if (!target) throw new Error("Could not determine the Rust target triple");

const metadata = JSON.parse(run("cargo", [
  "metadata", "--locked", "--format-version", "1", "--filter-platform", target,
  "--manifest-path", join(root, "src-tauri", "Cargo.toml"),
]));
const resolved = new Set(metadata.resolve.nodes.map((node) => node.id));
const crates = metadata.packages
  .filter((pkg) => resolved.has(pkg.id) && pkg.source)
  .sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`, "en"));
const npmPackages = Object.entries(JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8")).packages)
  .filter(([path]) => path)
  .sort(([a], [b]) => a.localeCompare(b, "en"));
const texts = new Map();

function licenseFiles(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory)
    .filter((name) => /^(license|licence|copying|notice)/i.test(name) && statSync(join(directory, name)).isFile())
    .sort()
    .map((name) => join(directory, name));
}

function record(file) {
  const content = readFileSync(file);
  const digest = createHash("sha256").update(content).digest("hex").slice(0, 16);
  texts.set(digest, content.toString("utf8"));
  return `${file.split(/[\\/]/).at(-1)} [text ${digest}]`;
}

const lines = [
  "# Third-party licenses and notices",
  "",
  `Generated for Rust target \`${target}\` from \`src-tauri/Cargo.lock\` and \`package-lock.json\`.`,
  "This file is included with MagicRenamer desktop bundles. Regenerate it for each release target",
  "by running `node scripts/generate_license_notices.mjs` with `MAGIC_RENAMER_TARGET` set",
  "to the target triple. The npm list includes build tools and optional platform packages;",
  "some of those packages are absent from the final app.",
  "",
  "MagicRenamer itself is licensed under MIT; see `LICENSE` next to this file.",
  "The package license expressions below come from package manifests. License files and",
  "notices copied below retain their original text. A package source link also gives access",
  "to its full source archive and any additional source notices.",
  "",
  "## Rust crates",
  "",
];
for (const pkg of crates) {
  if (!pkg.source.startsWith("registry+") || !pkg.license) {
    throw new Error(`Review the source and license for Rust package ${pkg.name} ${pkg.version}`);
  }
  const files = licenseFiles(dirname(pkg.manifest_path)).map(record);
  lines.push(`- **${pkg.name} ${pkg.version}** — \`${pkg.license || "license file"}\`; ` +
    `[source archive](https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download)` +
    (files.length ? `; bundled texts: ${files.join(", ")}` : ""));
}
lines.push("", "## npm packages", "");
for (const [path, pkg] of npmPackages) {
  if (!pkg.license) throw new Error(`Review the license for npm package ${path}`);
  const name = path.slice(path.lastIndexOf("node_modules/") + "node_modules/".length);
  const files = licenseFiles(join(root, path)).map(record);
  lines.push(`- **${name} ${pkg.version}** — \`${pkg.license || "see source"}\`; ` +
    `[package source](https://www.npmjs.com/package/${name}/v/${pkg.version})` +
    (files.length ? `; bundled texts: ${files.join(", ")}` : ""));
}
lines.push(
  "", "## MPL 2.0 source availability", "",
  "The following Rust crates are licensed under MPL 2.0. Their exact, unmodified source",
  "archives are available at the links below. Any changes to these crates distributed in a",
  "release must also be made available under MPL 2.0.", "",
);
for (const pkg of crates.filter((pkg) => pkg.license?.includes("MPL-2.0"))) {
  lines.push(`- ${pkg.name} ${pkg.version}: https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download`);
}
lines.push("", "## Original license and notice texts", "");
for (const digest of [...texts.keys()].sort()) {
  lines.push(`### Text ${digest}`, "", "````text", texts.get(digest).replace(/\n+$/, ""), "````", "");
}
writeFileSync(output, lines.join("\n"), "utf8");
console.log(`Generated THIRD_PARTY_NOTICES.md: ${crates.length} Rust crates, ${npmPackages.length} npm packages, ${texts.size} distinct license and notice texts`);
