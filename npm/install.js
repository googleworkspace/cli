#!/usr/bin/env node

"use strict";

const fs = require("fs");
const path = require("path");
const os = require("os");
const { pipeline } = require("stream/promises");
const { createWriteStream, mkdirSync, rmSync } = require("fs");
const { execSync } = require("child_process");
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
 * Download a file using native fetch (Node 18+).
 */
async function download(url, dest) {
  const res = await fetch(url, { redirect: "follow" });

  if (!res.ok) {
    throw new Error(`Failed to download ${url}: ${res.status} ${res.statusText}`);
  }

  const fileStream = createWriteStream(dest);
  // Convert web ReadableStream to Node stream and pipe
  const { Readable } = require("stream");
  const nodeStream = Readable.fromWeb(res.body);
  await pipeline(nodeStream, fileStream);
}

/**
 * Extract the archive to the install directory.
 */
function extract(archivePath, destDir) {
  const ext = path.extname(archivePath);
  const isZip = archivePath.endsWith(".zip");
  const isTar = archivePath.includes(".tar.");

  if (isTar) {
    execSync(`tar xf "${archivePath}" --strip-components 1 -C "${destDir}"`, {
      stdio: "pipe",
    });
  } else if (isZip) {
    if (process.platform === "win32") {
      execSync(
        `powershell.exe -NoProfile -NonInteractive -Command "Expand-Archive -LiteralPath '${archivePath}' -DestinationPath '${destDir}' -Force"`,
        { stdio: "pipe" },
      );
    } else {
      execSync(`unzip -q -o "${archivePath}" -d "${destDir}"`, {
        stdio: "pipe",
      });
    }
  } else {
    throw new Error(`Unsupported archive format: ${archivePath}`);
  }
}

async function install() {
  const platform = getPlatform();
  const url = getDownloadUrl(platform.artifact);

  // Check if already installed
  const binPath = path.join(INSTALL_DIR, platform.binary);
  if (fs.existsSync(binPath)) {
    console.error(`gws is already installed, skipping installation.`);
    return;
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

    console.error(`Extracting to ${INSTALL_DIR}`);
    extract(tmpFile, INSTALL_DIR);

    // Make binary executable on Unix
    if (process.platform !== "win32") {
      fs.chmodSync(binPath, 0o755);
    }

    console.error(`gws has been installed!`);
  } finally {
    // Clean up temp files
    rmSync(tmpDir, { recursive: true, force: true });
  }
}

install().catch((err) => {
  console.error(`Error installing gws: ${err.message}`);
  process.exit(1);
});
