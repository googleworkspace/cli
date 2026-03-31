#!/usr/bin/env node

"use strict";

const path = require("path");
const fs = require("fs");
const { spawnSync } = require("child_process");
const { getPlatform } = require("./platform");

const platform = getPlatform();
const binPath = path.join(__dirname, "bin", platform.binary);

if (!fs.existsSync(binPath)) {
  console.error(
    `gws binary not found at ${binPath}\nAuto-installing...`
  );
  const install = spawnSync(process.execPath, [path.join(__dirname, "install.js")], {
    cwd: __dirname,
    stdio: "inherit",
  });
  if (install.status !== 0) {
    process.exit(install.status ?? 1);
  }
}

const result = spawnSync(binPath, process.argv.slice(2), {
  cwd: process.cwd(),
  stdio: "inherit",
});

if (result.error) {
  console.error(`Error running gws: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
