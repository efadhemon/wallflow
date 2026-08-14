#!/usr/bin/env node
// Keeps every place that hardcodes the app version in sync with the single
// source of truth: the "version" field in package.json.
//
// Usage:
//   node scripts/sync-version.mjs            # rewrite Cargo.toml + Cargo.lock + website
//   node scripts/sync-version.mjs --check    # exit non-zero if Cargo.toml/lock drifts
//   node scripts/sync-version.mjs --website   # only rewrite the website (deploy step)

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const version = pkg.version;
if (!version || !/^\d+\.\d+\.\d+/.test(version)) {
  console.error(`package.json has an invalid version: ${JSON.stringify(version)}`);
  process.exit(1);
}

const args = new Set(process.argv.slice(2));
const checkOnly = args.has("--check");
const websiteOnly = args.has("--website");

const cargoPath = join(root, "src-tauri", "Cargo.toml");
const cargoLockPath = join(root, "src-tauri", "Cargo.lock");
const websitePath = join(root, "website", "index.html");
const versionJsonPath = join(root, "website", "api", "version.json");

let drift = false;

// --- Cargo.toml: the [package] version line ---
function syncCargo() {
  const original = readFileSync(cargoPath, "utf8");
  // Match the first `version = "x.y.z"` (the [package] one at the top).
  const re = /^version = "([^"]*)"/m;
  const current = original.match(re)?.[1];
  if (current === undefined) {
    console.error("Could not find a version line in src-tauri/Cargo.toml");
    process.exit(1);
  }
  if (current === version) return;
  drift = true;
  if (checkOnly) {
    console.error(`Cargo.toml version (${current}) does not match package.json (${version}).`);
    return;
  }
  writeFileSync(cargoPath, original.replace(re, `version = "${version}"`));
  console.log(`Updated src-tauri/Cargo.toml -> ${version}`);
}

// --- Cargo.lock: the version of the [[package]] named "wallflow" ---
// A plain `cargo build` regenerates this, but keeping it in sync avoids a
// stray lockfile diff after the next build.
function syncCargoLock() {
  const original = readFileSync(cargoLockPath, "utf8");
  // Match the version line that immediately follows `name = "wallflow"`.
  const re = /(name = "wallflow"\nversion = ")([^"]*)(")/;
  const match = original.match(re);
  if (!match) {
    console.error('Could not find the "wallflow" package entry in src-tauri/Cargo.lock');
    process.exit(1);
  }
  if (match[2] === version) return;
  drift = true;
  if (checkOnly) {
    console.error(`Cargo.lock version (${match[2]}) does not match package.json (${version}).`);
    return;
  }
  writeFileSync(cargoLockPath, original.replace(re, `$1${version}$3`));
  console.log(`Updated src-tauri/Cargo.lock -> ${version}`);
}

// --- website/index.html: anchored version occurrences ---
function syncWebsite() {
  const original = readFileSync(websitePath, "utf8");
  // Anchored so unrelated numbers (e.g. libwebkit2gtk-4.1-0) are left alone.
  const replacements = [
    [/(Download Wallflow )\d+\.\d+\.\d+/g, `$1${version}`],
    [/(Wallflow_)\d+\.\d+\.\d+(_)/g, `$1${version}$2`],
    [/(Version )\d+\.\d+\.\d+/g, `$1${version}`],
  ];
  let updated = original;
  for (const [re, replacement] of replacements) {
    updated = updated.replace(re, replacement);
  }
  if (updated === original) return;
  writeFileSync(websitePath, updated);
  console.log(`Updated website/index.html -> ${version}`);
}

// --- website/api/version.json: the version field ---
function syncVersionJson() {
  const original = readFileSync(versionJsonPath, "utf8");
  let current;
  try {
    current = JSON.parse(original).version;
  } catch {
    current = undefined;
  }
  if (current === version) return;
  drift = true;
  if (checkOnly) {
    console.error(`version.json (${current}) does not match package.json (${version}).`);
    return;
  }
  writeFileSync(versionJsonPath, `${JSON.stringify({ version }, null, 2)}\n`);
  console.log(`Updated website/api/version.json -> ${version}`);
}

if (websiteOnly) {
  syncWebsite();
  syncVersionJson();
} else {
  syncCargo();
  syncCargoLock();
  if (!checkOnly) syncWebsite();
  syncVersionJson();
}

if (checkOnly && drift) {
  console.error('Run "yarn version:sync" to fix.');
  process.exit(1);
}
