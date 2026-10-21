import { existsSync, readFileSync, writeFileSync } from "node:fs";

const rawVersion = process.argv[2];

if (!rawVersion) {
  throw new Error("Usage: node scripts/set-release-version.mjs <version-or-vtag>");
}

const version = rawVersion.replace(/^v/, "");

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid release version: ${rawVersion}`);
}

// Resolve every target up front: a release tag must not half-rewrite the tree,
// and a target that no longer exists is a hard error rather than a raw ENOENT.
const jsonTargets = ["package.json", "src-tauri/tauri.conf.json"];
const tomlTargets = ["Cargo.toml"];
const lockTarget = "Cargo.lock";

const missing = [...jsonTargets, ...tomlTargets, lockTarget].filter(
  (path) => !existsSync(path),
);
if (missing.length > 0) {
  throw new Error(`Missing release-version target(s): ${missing.join(", ")}`);
}

const replaceJsonVersion = (path) => {
  const json = JSON.parse(readFileSync(path, "utf8"));
  json.version = version;
  writeFileSync(path, `${JSON.stringify(json, null, 2)}\n`);
};

const replaceTomlPackageVersion = (path) => {
  const content = readFileSync(path, "utf8").replace(
    /(^\[workspace\.package\][\s\S]*?^version\s*=\s*)"[^"]+"/m,
    `$1"${version}"`,
  );
  writeFileSync(path, content);
};

const workspaceToml = readFileSync("Cargo.toml", "utf8");
const members = workspaceToml.match(/^members\s*=\s*\[([^\]]+)\]/m)?.[1].match(/"[^"]+"/g)?.map((value) => value.slice(1, -1));
if (!members?.length) throw new Error("Workspace package inventory is missing");
const names = members.map((member) => {
  if (!/^[a-zA-Z0-9_/-]+$/.test(member)) throw new Error("Unsupported workspace member path");
  const source = readFileSync(`${member}/Cargo.toml`, "utf8");
  const name = source.match(/^name\s*=\s*"([a-z0-9-]+)"/m)?.[1];
  if (!name || !/^version\.workspace\s*=\s*true/m.test(source)) throw new Error("Workspace member does not inherit its release version");
  return name;
});
const seen = new Set();
const lock = readFileSync(lockTarget, "utf8").split(/(?=^\[\[package\]\])/m).map((block) => {
  const name = block.match(/^name\s*=\s*"([a-z0-9-]+)"/m)?.[1];
  if (!names.includes(name) || /^source\s*=/m.test(block)) return block;
  if (seen.has(name)) throw new Error("Duplicate local workspace package in Cargo.lock");
  seen.add(name);
  return block.replace(/(^version\s*=\s*)"[^"]+"/m, `$1"${version}"`);
}).join("");
if (names.some((name) => !seen.has(name))) throw new Error("Local workspace package missing from Cargo.lock");
for (const path of jsonTargets) replaceJsonVersion(path);
for (const path of tomlTargets) replaceTomlPackageVersion(path);
writeFileSync(lockTarget, lock);
