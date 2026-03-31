#!/usr/bin/env node

"use strict";

const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const os = require("os");
const { pipeline } = require("stream/promises");
const { createWriteStream, mkdirSync, rmSync } = require("fs");
const { spawnSync } = require("child_process");
const { getPlatform } = require("./platform");

const INSTALL_DIR = path.join(__dirname, "bin");

/**
 * Get the GitHub release download URL base for the current package version.
 */
function getDownloadUrl(artifactName) {
  const { version } = require("./package.json");
  return `https://github.com/googleworkspace/cli/releases/download/v${version}/${artifactName}`;
}

/**
 * Strip ANSI escape sequences from a string.
 */
function sanitize(str) {
  // eslint-disable-next-line no-control-regex
  return String(str).replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
}

/**
 * Download a file using native fetch (Node 18+).
 *
 * NOTE: Native fetch does not respect HTTP_PROXY / HTTPS_PROXY environment
 * variables. If proxy support is needed, consider using the `undici` ProxyAgent
 * or a Node.js build with proxy support.
 */
async function download(url, dest) {
  const res = await fetch(url, { redirect: "follow" });

  if (!res.ok) {
    throw new Error(`Failed to download ${url}: ${res.status} ${res.statusText}`);
  }

  if (!res.body) {
    throw new Error(`Failed to download ${url}: Response body is empty`);
  }

  const fileStream = createWriteStream(dest);
  // Convert web ReadableStream to Node stream and pipe
  const { Readable } = require("stream");
  const nodeStream = Readable.fromWeb(res.body);
  await pipeline(nodeStream, fileStream);
}

/**
 * Run a command and throw on failure.
 */
function run(cmd, args) {
  const result = spawnSync(cmd, args, { stdio: "pipe" });
  if (result.error) {
    throw new Error(`Failed to run ${cmd}: ${result.error.message}`);
  }
  if ((result.status ?? 1) !== 0) {
    const stderr = result.stderr ? result.stderr.toString() : "";
    throw new Error(
      `Command failed: ${cmd} ${args.join(" ")}\n${stderr}`,
    );
  }
}

/**
 * Extract the archive to the install directory.
 */
function extract(archivePath, destDir) {
  const isZip = archivePath.endsWith(".zip");
  const isTar = archivePath.includes(".tar.");

  if (isTar) {
    run("tar", ["xf", archivePath, "-C", destDir]);
  } else if (isZip) {
    if (process.platform === "win32") {
      // Use single-quoted PowerShell strings with doubled single-quote escaping
      // to safely handle paths containing spaces and special characters.
      const psArchive = archivePath.replace(/'/g, "''");
      const psDest = destDir.replace(/'/g, "''");
      run("powershell.exe", [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `Expand-Archive -LiteralPath '${psArchive}' -DestinationPath '${psDest}' -Force`,
      ]);
    } else {
      run("unzip", ["-q", "-o", archivePath, "-d", destDir]);
    }
  } else {
    throw new Error(`Unsupported archive format: ${archivePath}`);
  }
}

async function install() {
  const platform = getPlatform();
  const { version } = require("./package.json");
  const url = getDownloadUrl(platform.artifact);

  // Check if the correct version is already installed
  const binPath = path.join(INSTALL_DIR, platform.binary);
  const versionFile = path.join(INSTALL_DIR, ".version");
  if (fs.existsSync(binPath) && fs.existsSync(versionFile)) {
    const installed = fs.readFileSync(versionFile, "utf8").trim();
    if (installed === version) {
      console.error(`gws v${version} is already installed, skipping.`);
      return;
    }
    console.error(`Upgrading gws from v${installed} to v${version}`);
  }

  // Clean and create install directory
  if (fs.existsSync(INSTALL_DIR)) {
    rmSync(INSTALL_DIR, { recursive: true, force: true });
  }
  mkdirSync(INSTALL_DIR, { recursive: true });

  // Download to a temp file
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "gws-"));
  const archiveName = path.basename(platform.artifact);
  const tmpFile = path.join(tmpDir, archiveName);

  try {
    console.error(`Downloading gws from ${url}`);
    await download(url, tmpFile);

    // Verify SHA256 checksum
    const sha256Url = `${url}.sha256`;
    const sha256File = `${tmpFile}.sha256`;
    console.error(`Verifying checksum from ${sha256Url}`);
    await download(sha256Url, sha256File);

    const expectedHash = fs.readFileSync(sha256File, "utf8").trim().split(/\s+/)[0].toLowerCase();
    const fileBuffer = fs.readFileSync(tmpFile);
    const actualHash = crypto.createHash("sha256").update(fileBuffer).digest("hex").toLowerCase();

    if (actualHash !== expectedHash) {
      throw new Error(
        `SHA256 checksum mismatch!\n  Expected: ${expectedHash}\n  Actual:   ${actualHash}\nThe downloaded binary may have been tampered with.`,
      );
    }
    console.error("Checksum verified ✓");

    console.error(`Extracting to ${INSTALL_DIR}`);
    extract(tmpFile, INSTALL_DIR);

    // Make binary executable on Unix
    if (process.platform !== "win32") {
      fs.chmodSync(binPath, 0o755);
    }

    console.error(`gws v${version} has been installed!`);
    fs.writeFileSync(versionFile, version);
  } finally {
    // Clean up temp files
    rmSync(tmpDir, { recursive: true, force: true });
  }
}

install().catch((err) => {
  console.error(`Error installing gws: ${sanitize(err.message)}`);
  process.exit(1);
});
