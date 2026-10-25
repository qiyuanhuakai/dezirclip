import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { parseVersion } from "./updater-release.mjs";

const args = process.argv.slice(2);
const check = args[0] === "--check";
const rawVersion = args[check ? 1 : 0];

if ((!check && !rawVersion) || args.length > (check ? 2 : 1)) {
  throw new Error("Usage: node scripts/set-release-version.mjs <version-or-vtag>; --check [version-or-vtag]");
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

const originals = new Map([...jsonTargets, ...tomlTargets, lockTarget].map(path => [path, readFileSync(path, "utf8")]));
const jsonVersions = jsonTargets.map(path => {
  const json = JSON.parse(originals.get(path));
  if (!json || typeof json !== "object" || Array.isArray(json) || typeof json.version !== "string") {
    throw new Error(`Missing release version in ${path}`);
  }
  return { path, json };
});
const version = parseVersion(rawVersion ?? jsonVersions[0].json.version).value;
const changed = [];
const updates = new Map(originals);
for (const { path, json } of jsonVersions) {
  if (json.version !== version) {
    changed.push(path);
    json.version = version;
    updates.set(path, `${JSON.stringify(json, null, 2)}\n`);
  }
}

const replaceVersion = (block, label) => {
  const declarations = [...block.matchAll(/^version[ \t]*=[^\r\n]*/gm)];
  if (declarations.length !== 1) throw new Error(`Expected exactly one release version in ${label}`);
  const declaration = declarations[0];
  const fields = /^(version[ \t]*=[ \t]*)"([^"\r\n]+)"([ \t]*(?:#[^\r\n]*)?)$/.exec(declaration[0]);
  if (!fields) throw new Error(`Invalid release version field in ${label}`);
  if (fields[2] === version) return block;
  changed.push(label);
  return block.slice(0, declaration.index) + `${fields[1]}"${version}"${fields[3]}`
    + block.slice(declaration.index + declaration[0].length);
};

const workspaceToml = originals.get("Cargo.toml");
const sections = workspaceToml.split(/(?=^\[)/m);
const packageSections = sections.map((block, index) => /^\[workspace\.package\]\r?\n/.test(block) ? index : -1).filter(index => index >= 0);
if (packageSections.length !== 1) throw new Error("Expected exactly one workspace.package section");
sections[packageSections[0]] = replaceVersion(sections[packageSections[0]], "Cargo.toml workspace.package");
updates.set("Cargo.toml", sections.join(""));
const members = workspaceToml.match(/^members\s*=\s*\[([^\]]+)\]/m)?.[1].match(/"[^"]+"/g)?.map((value) => value.slice(1, -1));
if (!members?.length) throw new Error("Workspace package inventory is missing");
const names = members.map((member) => {
  if (!/^[a-zA-Z0-9_/-]+$/.test(member)) throw new Error("Unsupported workspace member path");
  const source = readFileSync(`${member}/Cargo.toml`, "utf8");
  const name = source.match(/^name\s*=\s*"([a-z0-9-]+)"/m)?.[1];
  if (!name || !/^version\.workspace\s*=\s*true/m.test(source)) throw new Error("Workspace member does not inherit its release version");
  return name;
});
if (new Set(names).size !== names.length || new Set(members).size !== members.length) throw new Error("Duplicate workspace package inventory");
const seen = new Set();
const lock = originals.get(lockTarget).split(/(?=^\[\[package\]\])/m).map((block) => {
  const name = block.match(/^name\s*=\s*"([a-z0-9-]+)"/m)?.[1];
  if (!names.includes(name) || /^source\s*=/m.test(block)) return block;
  if (seen.has(name)) throw new Error("Duplicate local workspace package in Cargo.lock");
  seen.add(name);
  return replaceVersion(block, `Cargo.lock ${name}`);
}).join("");
if (names.some((name) => !seen.has(name))) throw new Error("Local workspace package missing from Cargo.lock");
updates.set(lockTarget, lock);
if (check && changed.length) throw new Error(`Release versions differ from ${version}: ${changed.join(", ")}`);
if (!check) {
  for (const [path, content] of updates) {
    if (content !== originals.get(path)) writeFileSync(path, content);
  }
}
console.log(`Release version ${version}: ${check ? "all targets match" : "all targets synchronized"}`);
