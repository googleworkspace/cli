#!/usr/bin/env node

"use strict";

const os = require("os");
const path = require("path");
const fs = require("fs");
const { spawnSync } = require("child_process");

const { supportedPlatforms } = require("./package.json");

/**
 * Map Node.js os.type() and os.arch() to Rust-style target triples.
 */
function getPlatformKey() {
  const rawOs = os.type();
  const rawArch = os.arch();

  let osType;
  switch (rawOs) {
    case "Windows_NT":
      osType = "pc-windows-msvc";
      break;
    case "Darwin":
      osType = "apple-darwin";
      break;
    case "Linux":
      osType = "unknown-linux-gnu";
      break;
    default:
      throw new Error(`Unsupported operating system: ${rawOs}`);
  }

  let arch;
  switch (rawArch) {
    case "x64":
      arch = "x86_64";
      break;
    case "arm64":
      arch = "aarch64";
      break;
    default:
      throw new Error(`Unsupported architecture: ${rawArch}`);
  }

  // On Linux, try to detect musl libc
  if (rawOs === "Linux") {
    try {
      const result = spawnSync("ldd", ["--version"], {
        encoding: "utf8",
        stdio: ["pipe", "pipe", "pipe"],
      });
      // musl ldd prints version info to stderr
      const output = (result.stdout || "") + (result.stderr || "");
      if (output.toLowerCase().includes("musl")) {
        osType = "unknown-linux-musl";
      }
    } catch {
      // If ldd fails, assume glibc
    }
  }

  const key = `${arch}-${osType}`;

  if (!supportedPlatforms[key]) {
    // Try musl fallback on Linux if glibc binary is not available
    if (rawOs === "Linux") {
      const muslKey = `${arch}-unknown-linux-musl`;
      if (supportedPlatforms[muslKey]) {
        return muslKey;
      }
    }
    throw new Error(
      `Unsupported platform: ${key}\nSupported platforms: ${Object.keys(supportedPlatforms).join(", ")}`,
    );
  }

  return key;
}

function getPlatform() {
  const key = getPlatformKey();
  return supportedPlatforms[key];
}

module.exports = { getPlatform, getPlatformKey };
