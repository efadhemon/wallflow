#!/usr/bin/env node
// Puts the bundled FFmpeg for a target where Tauri expects sidecars:
// src-tauri/binaries/{ffmpeg,ffprobe}-<triple>[.exe], plus its license and
// build notes. The archive comes from the `ffmpeg-<tag>-wf<n>` release made by
// .github/workflows/ffmpeg.yml and must match scripts/ffmpeg/SHA256SUMS.
// Needs a GitHub token (the repository is private).
//
//   node scripts/fetch-ffmpeg.mjs [target-triple]   (default: this machine)
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const env = Object.fromEntries(
  readFileSync(join(root, "scripts/ffmpeg/version.env"), "utf8")
    .split("\n")
    .filter(line => /^[A-Z_]+=/.test(line))
    .map(line => line.split("=", 2)),
);

const hostTriple = () => execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)[1];
const target = process.argv[2] ?? hostTriple();
if (target.includes("linux")) {
  console.log("Linux uses the distribution's FFmpeg (package dependency); nothing to fetch.");
  process.exit(0);
}

const archive = `ffmpeg-${target}.tar.gz`;
const sums = readFileSync(join(root, "scripts/ffmpeg/SHA256SUMS"), "utf8");
const expected = sums.match(new RegExp(`^([0-9a-f]{64})\\s+\\*?${archive.replace(/\./g, "\\.")}$`, "m"))?.[1];
if (!expected) {
  console.error(
    `scripts/ffmpeg/SHA256SUMS has no entry for ${archive}. Run the FFmpeg workflow and copy its SHA256SUMS.`,
  );
  process.exit(1);
}

const exe = target.includes("windows") ? ".exe" : "";
const binaries = join(root, "src-tauri/binaries");
const stamp = join(binaries, `.ffmpeg-${target}.sha256`);
if (existsSync(stamp) && readFileSync(stamp, "utf8").trim() === expected) {
  console.log(`FFmpeg for ${target} is up to date.`);
  process.exit(0);
}

const tag = `ffmpeg-${env.FFMPEG_TAG}-wf${env.WALLFLOW_FFMPEG_BUILD}`;
// The repository is private: release files come through the API, with a token
// (GH_TOKEN / GITHUB_TOKEN in CI, else the GitHub CLI's login).
const token =
  process.env.GH_TOKEN ??
  process.env.GITHUB_TOKEN ??
  (() => {
    try {
      return execFileSync("gh", ["auth", "token"], { encoding: "utf8" }).trim();
    } catch {
      return undefined;
    }
  })();
const headers = { "User-Agent": "wallflow-fetch-ffmpeg", ...(token && { Authorization: `Bearer ${token}` }) };
const api = `https://api.github.com/repos/efadhemon/wallflow/releases/tags/${tag}`;
const release = await fetch(api, { headers });
if (!release.ok) {
  console.error(`Release ${tag}: HTTP ${release.status}${token ? "" : " (no token: set GH_TOKEN or log in with gh)"}`);
  process.exit(1);
}
const asset = (await release.json()).assets.find(a => a.name === archive);
if (!asset) {
  console.error(`Release ${tag} has no ${archive}`);
  process.exit(1);
}
console.log(`Downloading ${tag}/${archive}`);
const response = await fetch(asset.url, { headers: { ...headers, Accept: "application/octet-stream" } });
if (!response.ok) {
  console.error(`Download failed: HTTP ${response.status}`);
  process.exit(1);
}
const data = Buffer.from(await response.arrayBuffer());
const actual = createHash("sha256").update(data).digest("hex");
if (actual !== expected) {
  console.error(`Checksum mismatch for ${archive}: got ${actual}, expected ${expected}`);
  process.exit(1);
}

const temp = mkdtempSync(join(tmpdir(), "wallflow-ffmpeg-"));
try {
  writeFileSync(join(temp, archive), data);
  execFileSync("tar", ["-xzf", archive], { cwd: temp, stdio: "inherit" });
  mkdirSync(binaries, { recursive: true });
  for (const tool of ["ffmpeg", "ffprobe"]) {
    // Copied, not renamed: the temp folder may be on another filesystem.
    const dest = join(binaries, `${tool}-${target}${exe}`);
    copyFileSync(join(temp, tool + exe), dest);
    chmodSync(dest, 0o755);
  }
  for (const note of ["FFmpeg-LICENSE.txt", "FFmpeg-BUILD.txt"]) {
    copyFileSync(join(temp, note), join(binaries, note));
  }
  writeFileSync(stamp, expected + "\n");
  console.log(`FFmpeg for ${target} is in src-tauri/binaries.`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}
