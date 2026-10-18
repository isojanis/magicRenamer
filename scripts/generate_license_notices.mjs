import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Set MAGIC_RENAMER_TARGET to the destination Rust target triple when cross-compiling.
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, "THIRD_PARTY_NOTICES.md");
const textOutput = join(root, "THIRD_PARTY_LICENSE_TEXTS.md");
const run = (command, args) => execFileSync(command, args, { encoding: "utf8", maxBuffer: 128 * 1024 * 1024 });
const target = process.env.MAGIC_RENAMER_TARGET || process.env.CARGO_BUILD_TARGET ||
  run("rustc", ["-vV"]).match(/^host: (.+)$/m)?.[1];
if (!target) throw new Error("Could not determine the Rust target triple");

const metadata = JSON.parse(run("cargo", [
  "metadata", "--locked", "--format-version", "1", "--filter-platform", target,
  "--manifest-path", join(root, "src-tauri", "Cargo.toml"),
]));
const bundleArgument = process.argv.indexOf("--bundle-manifest");
if (bundleArgument < 0 || !process.argv[bundleArgument + 1]) {
  throw new Error("Build the frontend to provide the bundled npm module list before generating notices");
}
const bundle = JSON.parse(process.argv[bundleArgument + 1]);
if (!Array.isArray(bundle.packagePaths) || !Array.isArray(bundle.modulePaths)) {
  throw new Error("Invalid frontend bundle manifest");
}

// Metadata unions features across dependency contexts. Use Cargo's resolved tree
// to exclude dependencies enabled only for build tools or other targets.
const runtimeTree = run("cargo", [
  "tree", "--locked", "--target", target, "--edges", "normal,no-proc-macro",
  "--prefix", "none", "--format", "{p}", "--color", "never",
  "--manifest-path", join(root, "src-tauri", "Cargo.toml"),
]);
const runtime = new Set();
for (const line of runtimeTree.trim().split(/\r?\n/)) {
  const match = line.match(/^(\S+) v(\S+)(?:\s|$)/);
  if (!match) throw new Error(`Unrecognized Cargo tree entry: ${line}`);
  runtime.add(`${match[1]}@${match[2]}`);
}
const crates = [];
for (const key of runtime) {
  const matches = metadata.packages.filter((pkg) => `${pkg.name}@${pkg.version}` === key);
  if (matches.length !== 1) throw new Error(`Expected one Cargo metadata package for ${key}`);
  if (matches[0].source) crates.push(matches[0]);
}
crates.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`, "en"));
const lockedNpmPackages = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8")).packages;
const npmPackages = bundle.packagePaths.map((path) => {
  const pkg = lockedNpmPackages[path];
  if (!pkg) throw new Error(`Bundled npm package ${path} is missing from package-lock.json`);
  return [path, pkg];
}).sort(([a], [b]) => a.localeCompare(b, "en"));
const texts = new Map();
const missingTexts = [];

function licenseFiles(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory)
    .filter((name) => /^(license|licence|copying|notice)/i.test(name) && statSync(join(directory, name)).isFile())
    .sort()
    .map((name) => join(directory, name));
}

function recordText(name, content) {
  const digest = createHash("sha256").update(content).digest("hex").slice(0, 16);
  texts.set(digest, content);
  return `${name} [text ${digest}](THIRD_PARTY_LICENSE_TEXTS.md#text-${digest})`;
}

function record(file) {
  return recordText(file.split(/[\\/]/).at(-1), readFileSync(file).toString("utf8"));
}

const lines = [
  "# Third-party licenses and notices",
  "",
  `Generated for Rust target \`${target}\` from \`src-tauri/Cargo.lock\` and \`package-lock.json\`.`,
  "This file is included with MagicRenamer desktop bundles. Regenerate it for each release target",
  "by running `npm run build` with `MAGIC_RENAMER_TARGET` set",
  "to the target triple. Rust crates follow Cargo tree’s resolved normal dependency graph and exclude",
  "build-only and procedural-macro crates. The npm list comes from modules rendered",
  "into the production frontend bundle.",
  "",
  "MagicRenamer itself is licensed under MIT; see `LICENSE` in the source repository",
  "or `LICENSE-MagicRenamer` beside these files in release packages.",
  "The package license expressions below come from package manifests. License files and",
  "notices are linked to their full text in THIRD_PARTY_LICENSE_TEXTS.md, which is shipped",
  "beside this index. A package source link also gives access",
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
  if (!files.length) missingTexts.push(`Rust: ${pkg.name} ${pkg.version}`);
  lines.push(`- **${pkg.name} ${pkg.version}** — \`${pkg.license || "license file"}\`; ` +
    `[source archive](https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download)` +
    (files.length ? `; bundled texts: ${files.join(", ")}` : ""));
}
lines.push("", "## npm packages", "");
for (const [path, pkg] of npmPackages) {
  if (!pkg.license) throw new Error(`Review the license for npm package ${path}`);
  const name = path.slice(path.lastIndexOf("node_modules/") + "node_modules/".length);
  const files = licenseFiles(join(root, path)).map(record);
  if (!files.length) missingTexts.push(`npm: ${name} ${pkg.version}`);
  lines.push(`- **${name} ${pkg.version}** — \`${pkg.license}\`; ` +
    `[package source](https://www.npmjs.com/package/${name}/v/${pkg.version})` +
    (files.length ? `; bundled texts: ${files.join(", ")}` : ""));
}
const bundledSourceNotices = [];
for (const file of bundle.modulePaths) {
  if (!file.replaceAll("\\", "/").includes("/node_modules/")) continue;
  const source = readFileSync(file, "utf8").slice(0, 4096);
  const comments = source.match(/\/\*[\s\S]*?\*\/|(?:^[ \t]*\/\/[^\r\n]*(?:\r?\n|$))+/gm) || [];
  for (const comment of comments.filter((value) => /copyright/i.test(value))) {
    const label = relative(root, file).replaceAll("\\", "/");
    bundledSourceNotices.push(`- ${label}: ${recordText("source header", comment.trim())}`);
  }
}
if (bundledSourceNotices.length) {
  lines.push("", "## Bundled source notices", "", ...bundledSourceNotices);
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
const textLines = [
  "# Third-party license and notice texts",
  "",
  "Companion to [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Ship both files together.",
  "",
];
for (const digest of [...texts.keys()].sort()) {
  textLines.push(`### Text ${digest}`, "", "````text", texts.get(digest).replace(/\n+$/, ""), "````", "");
}
if (process.argv.includes("--strict") && missingTexts.length) {
  throw new Error(`Unresolved license-text omissions (${missingTexts.length}):\n${missingTexts.join("\n")}`);
}
writeFileSync(output, lines.join("\n"), "utf8");
writeFileSync(textOutput, textLines.join("\n"), "utf8");
console.log(`Generated THIRD_PARTY_NOTICES.md: ${crates.length} Rust crates, ${npmPackages.length} npm packages, ${texts.size} distinct license and notice texts`);
