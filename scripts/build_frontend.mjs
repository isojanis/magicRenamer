import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const renderedModules = new Set();

await build({
  root,
  plugins: [{
    name: "collect-shipped-npm-modules",
    generateBundle(_options, bundle) {
      for (const output of Object.values(bundle)) {
        if (output.type !== "chunk") continue;
        for (const [id, details] of Object.entries(output.modules)) {
          if (details.renderedLength > 0 && existsSync(id)) renderedModules.add(id);
        }
      }
    },
  }],
});

if (!renderedModules.size) throw new Error("Frontend build did not report rendered modules");

const packagePaths = new Set();
const modulePaths = [];
for (const id of renderedModules) {
  const normalized = id.split(sep).join("/");
  const marker = "/node_modules/";
  const offset = normalized.lastIndexOf(marker);
  if (offset < 0) continue;
  const suffix = normalized.slice(offset + marker.length).split("/");
  const name = suffix[0].startsWith("@") ? suffix.slice(0, 2).join("/") : suffix[0];
  if (!name || name.endsWith("/")) throw new Error("Could not identify bundled npm package for " + id);
  const packageRoot = normalized.slice(0, offset + marker.length) + name;
  const packagePath = relative(root, packageRoot).split(sep).join("/");
  if (packagePath.startsWith("..")) throw new Error("Bundled npm package is outside the project: " + id);
  packagePaths.add(packagePath);
  modulePaths.push(id);
}

const manifest = {
  packagePaths: [...packagePaths].sort(),
  modulePaths: modulePaths.sort(),
};
execFileSync(process.execPath, [
  join(root, "scripts", "generate_license_notices.mjs"),
  "--bundle-manifest", JSON.stringify(manifest),
  ...(process.argv.includes("--strict") ? ["--strict"] : []),
], { cwd: root, env: process.env, stdio: "inherit" });
