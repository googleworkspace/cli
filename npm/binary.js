const { Package } = require("./binary-install");
const os = require("os");
const cTable = require("console.table");
const libc = require("detect-libc");
const { configureProxy } = require("axios-proxy-builder");

const error = (msg) => {
  console.error(msg);
  process.exit(1);
};

const {
  name,
  artifactDownloadUrls,
  supportedPlatforms,
  glibcMinimum,
} = require("./package.json");

// FIXME: implement NPM installer handling of fallback download URLs
const artifactDownloadUrl = artifactDownloadUrls[0];
const builderGlibcMajorVersion = glibcMinimum.major;
const builderGlibcMinorVersion = glibcMinimum.series;

const getPlatform = () => {
  const rawOsType = os.type();
  const rawArchitecture = os.arch();

  // We want to use rust-style target triples as the canonical key
  // for a platform, so translate the "os" library's concepts into rust ones
  let osType = "";
  switch (rawOsType) {
    case "Windows_NT":
      osType = "pc-windows-msvc";
      break;
    case "Darwin":
      osType = "apple-darwin";
      break;
    case "Linux":
      osType = "unknown-linux-gnu";
      break;
  }

  let arch = "";
  switch (rawArchitecture) {
    case "x64":
      arch = "x86_64";
      break;
    case "arm64":
      arch = "aarch64";
      break;
  }

  if (rawOsType === "Linux") {
    if (libc.familySync() == "musl") {
      osType = "unknown-linux-musl-dynamic";
    } else if (libc.isNonGlibcLinuxSync()) {
      console.warn(
        "Your libc is neither glibc nor musl; trying static musl binary instead",
      );
      osType = "unknown-linux-musl-static";
    } else {
      let libcVersion = libc.versionSync();
      if (libcVersion == null) {
        // detect-libc could not determine the libc version (e.g. Android/Termux
        // with bionic libc). Fall back to static musl binary.
        console.warn(
          "Could not detect libc version; trying static musl binary instead",
        );
        osType = "unknown-linux-musl-static";
      } else {
        let splitLibcVersion = libcVersion.split(".");
        let libcMajorVersion = splitLibcVersion[0];
        let libcMinorVersion = splitLibcVersion[1];
        if (
          libcMajorVersion != builderGlibcMajorVersion ||
          libcMinorVersion < builderGlibcMinorVersion
        ) {
          // We can't run the glibc binaries, but we can run the static musl ones
          // if they exist
          console.warn(
            "Your glibc isn't compatible; trying static musl binary instead",
          );
          osType = "unknown-linux-musl-static";
        }
      }
    }
  }

  // Assume the above succeeded and build a target triple to look things up with.
  // If any of it failed, this lookup will fail and we'll handle it like normal.
  let targetTriple = `${arch}-${osType}`;
  let platform = supportedPlatforms[targetTriple];

  if (!platform) {
    error(
      `Platform with type "${rawOsType}" and architecture "${rawArchitecture}" is not supported by ${name}.\nYour system must be one of the following:\n\n${Object.keys(
        supportedPlatforms,
      ).join(",")}`,
    );
  }

  return platform;
};

const getPackage = () => {
  const platform = getPlatform();
  const url = `${artifactDownloadUrl}/${platform.artifactName}`;
  let filename = platform.artifactName;
  let ext = platform.zipExt;
  let binary = new Package(platform, name, url, filename, ext, platform.bins);

  return binary;
};

const install = (suppressLogs) => {
  if (!artifactDownloadUrl || artifactDownloadUrl.length === 0) {
    console.warn("in demo mode, not installing binaries");
    return;
  }
  const package = getPackage();
  const proxy = configureProxy(package.url);

  return package.install(proxy, suppressLogs);
};

const run = (binaryName) => {
  const package = getPackage();
  const proxy = configureProxy(package.url);

  package.run(binaryName, proxy);
};

module.exports = {
  install,
  run,
  getPackage,
};
