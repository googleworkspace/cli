#!/usr/bin/env node

"use strict";

const path = require("path");
const { spawnSync } = require("child_process");
const { getPlatform } = require("./platform");

const platform = getPlatform();
const binPath = path.join(__dirname, "bin", platform.binary);

const result = spawnSync(binPath, process.argv.slice(2), {
  cwd: process.cwd(),
  stdio: "inherit",
});

if (result.error) {
  console.error(`Error running gws: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status);
