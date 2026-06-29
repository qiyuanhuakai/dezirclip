import { readFileSync, writeFileSync } from "node:fs";

const rawVersion = process.argv[2];

if (!rawVersion) {
  throw new Error("Usage: node scripts/set-release-version.mjs <version-or-vtag>");
}

const version = rawVersion.replace(/^v/, "");

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid release version: ${rawVersion}`);
}

const replaceJsonVersion = (path) => {
  const json = JSON.parse(readFileSync(path, "utf8"));
  json.version = version;
  writeFileSync(path, `${JSON.stringify(json, null, 2)}\n`);
};

const replaceTomlPackageVersion = (path) => {
  const content = readFileSync(path, "utf8").replace(
    /(^\[package\][\s\S]*?^version\s*=\s*)"[^"]+"/m,
    `$1"${version}"`,
  );
  writeFileSync(path, content);
};

replaceJsonVersion("package.json");
replaceJsonVersion("src-tauri/tauri.conf.json");
replaceTomlPackageVersion("src-tauri/Cargo.toml");
replaceTomlPackageVersion("dzc-standalone/Cargo.toml");
