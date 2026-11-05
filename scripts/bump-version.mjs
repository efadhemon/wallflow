import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import semver from "semver";

const [, , type] = process.argv; // patch, minor, or major

if (!["patch", "minor", "major"].includes(type)) {
  console.error("Usage: node scripts/bump-version.mjs [patch|minor|major]");
  process.exit(1);
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const packageJsonPath = join(root, "package.json");

const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8"));
const currentVersion = packageJson.version;

const newVersion = semver.inc(currentVersion, type);
if (!newVersion) {
  console.error(`Cannot bump invalid version: ${currentVersion}`);
  process.exit(1);
}

packageJson.version = newVersion;
writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + "\n");

console.info(`✅ Version bumped: ${currentVersion} -> ${newVersion}`);
