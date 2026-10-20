import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, linkSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { prepareSidecars, sidecarOptions } from "../prepare-tauri-sidecars.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const hash = (value) => createHash("sha256").update(value).digest("hex");

function fixture(t, { host = "x86_64-pc-windows-msvc", target = host, profile = "release", portable = false, omitOutput = false } = {}) {
  const temporaryRoot = resolve(tmpdir());
  const directory = mkdtempSync(join(temporaryRoot, "dzc-sidecar-contract-"));
  assert.equal(dirname(resolve(directory)), temporaryRoot);
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const targetDirectory = join(directory, "custom cargo target");
  const extension = target.includes("windows") ? ".exe" : "";
  const source = join(targetDirectory, target, profile, `dzc${extension}`);
  const hostDefault = join(targetDirectory, profile, `dzc${host.includes("windows") ? ".exe" : ""}`);
  const cache = join(targetDirectory, "cached-desktop.bin");
  mkdirSync(dirname(hostDefault), { recursive: true });
  writeFileSync(cache, "desktop-feature-cached-artifact");
  linkSync(cache, hostDefault);
  writeFileSync(join(directory, "Cargo.lock"), "locked fixture dependencies\n");
  writeFileSync(join(directory, "package.json"), JSON.stringify({ version: "0.2.0-alpha.1" }));
  const payload = Buffer.from(`standalone:${target}:${profile}:portable=${portable}`);
  const calls = [];
  const run = (command, args, options) => {
    calls.push([command, args]);
    assert.equal(options.cwd, directory);
    if (command === "rustc") {
      assert.deepEqual(args, ["--print", "host-tuple"]);
      return host + "\n";
    }
    if (command === source) {
      assert.deepEqual(args, ["build-info", "--json"]);
      return JSON.stringify({ component: "cli", component_version: "0.2.0-alpha.1", target,
        source_revision: "a".repeat(40), build_id: "0.2.0-alpha.1@" + "a".repeat(40), source_state: "verified_checkout" });
    }
    if (command === "git") return "a".repeat(40);
    assert.equal(command, "cargo");
    if (args[0] === "metadata") return JSON.stringify({ target_directory: targetDirectory });
    assert.deepEqual(args, ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "dzc", "--locked", "--no-default-features", "--target", target,
      ...(profile === "release" ? ["--release"] : []), ...(portable ? ["--features", "portable"] : [])]);
    if (!omitOutput) {
      mkdirSync(dirname(source), { recursive: true });
      writeFileSync(source, payload);
    }
  };
  return { directory, targetDirectory, source, hostDefault, cache, payload, calls, run, host, target };
}

for (const parameters of [
  { host: "x86_64-pc-windows-msvc", profile: "release" },
  { host: "x86_64-pc-windows-msvc", profile: "debug" },
  { host: "x86_64-unknown-linux-gnu", profile: "release" },
  { host: "x86_64-unknown-linux-gnu", profile: "debug" },
]) {
  test(`shared bundle hook restores standalone inputs: ${parameters.host} ${parameters.profile}`, (t) => {
    const f = fixture(t, parameters);
    const proof = prepareSidecars({ repoRoot: f.directory, run: f.run, args: ["--phase", "bundle"], env: { TAURI_ENV_DEBUG: String(parameters.profile === "debug"), TAURI_ENV_TARGET_TRIPLE: f.host } });
    assert.equal(proof.sha256, hash(f.payload));
    assert.equal(proof.phase, "bundle");
    assert.equal(proof.binaryPaths.length, 3);
    for (const item of proof.binaryPaths) {
      assert.deepEqual(readFileSync(item.path), f.payload);
      assert.equal(item.sha256, proof.sha256);
    }
    assert.equal(readFileSync(f.cache, "utf8"), "desktop-feature-cached-artifact", "publishing must not mutate Cargo's cached hardlink");
    const staged = proof.binaryPaths.find((item) => item.role === "external-bin").path;
    assert.deepEqual(JSON.parse(readFileSync(staged + ".proof.json", "utf8")), proof);
    assert.equal(proof.cargoLockSha256, hash(readFileSync(join(f.directory, "Cargo.lock"))));
    assert.equal(f.calls.length, 4, "one native metadata query follows the build");
    assert.equal(proof.buildInfoVerification, "executed_native_cli");
    assert.equal(proof.buildInfo.component, "cli");
  });
}

test("cross target updates only target and external inputs, preserving host artifacts", (t) => {
  const f = fixture(t, { target: "aarch64-pc-windows-msvc" });
  const proof = prepareSidecars({ repoRoot: f.directory, run: f.run, args: ["--phase", "bundle"], env: { TAURI_ENV_TARGET_TRIPLE: f.target } });
  assert.equal(proof.binaryPaths.length, 2);
  assert.equal(proof.target, f.target);
  assert.equal(proof.buildInfo, null);
  assert.equal(proof.buildInfoVerification, "cross_target_not_executed");
  assert.equal(readFileSync(f.hostDefault, "utf8"), "desktop-feature-cached-artifact");
  assert.deepEqual(readFileSync(proof.source), f.payload);
});

test("native component version and unknown or mismatched identities fail closed", (t) => {
  for (const changed of [
    { component_version: "0.1.20" }, { component: "desktop" }, { build_id: "different-build" },
    { source_revision: null, source_state: "unknown", build_id: "claimed-unknown-build" },
  ]) {
    const f = fixture(t);
    const run = (command, args, options) => {
      const result = f.run(command, args, options);
      return command === f.source ? JSON.stringify({ ...JSON.parse(result), ...changed }) : result;
    };
    assert.throws(() => prepareSidecars({ repoRoot: f.directory, run, args: [], env: {} }), /build identity/);
    assert.equal(readFileSync(f.hostDefault, "utf8"), "desktop-feature-cached-artifact");
  }
});

test("portable no-bundle preparation explicitly matches release and portable features", (t) => {
  const f = fixture(t, { portable: true });
  const proof = prepareSidecars({ repoRoot: f.directory, run: f.run, args: ["--portable", "--release", "--phase", "portable"], env: { TAURI_ENV_DEBUG: "true" } });
  assert.equal(proof.portable, true);
  assert.equal(proof.profile, "release");
  assert.equal(proof.phase, "portable");
  assert.deepEqual(readFileSync(f.hostDefault), f.payload);
});

test("missing explicit-target output fails instead of reusing a desktop executable", (t) => {
  const f = fixture(t, { omitOutput: true });
  assert.throws(() => prepareSidecars({ repoRoot: f.directory, run: f.run, args: [], env: {} }), /ENOENT/);
  assert.equal(readFileSync(f.hostDefault, "utf8"), "desktop-feature-cached-artifact");
  assert.equal(existsSync(join(f.directory, "src-tauri", "binaries")), false);
});

test("invalid hook settings fail closed before building", () => {
  for (const [args, env] of [
    [["--phase", "unknown"], {}], [["--target", "../../escape"], {}], [["--debug", "--release"], {}], [[], { TAURI_ENV_DEBUG: "1" }],
  ]) assert.throws(() => sidecarOptions(args, env));
  assert.equal(sidecarOptions([], {}).profile, "release");
  assert.equal(sidecarOptions([], { TAURI_ENV_DEBUG: "true" }).profile, "debug");
  let calls = 0;
  assert.throws(() => prepareSidecars({ args: [], env: { TAURI_ENV_TARGET_TRIPLE: "../windows" }, run: () => { calls += 1; return "x86_64-pc-windows-msvc"; } }), /Unsupported/);
  assert.equal(calls, 1);
});

test("packaging entry points use the shared hook and portable CLI feature", () => {
  const config = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  assert.equal(config.build.beforeBuildCommand, "pnpm run build && pnpm run tauri:prepare-sidecars --phase build");
  assert.equal(config.build.beforeBundleCommand, "pnpm run tauri:prepare-sidecars --phase bundle");
  assert.match(pkg.scripts["build:portable"], /^tauri build --no-bundle --features portable && pnpm run tauri:prepare-sidecars --portable --release --phase portable && powershell /);
  const native = readFileSync(join(root, "scripts/build-native-windows-fixture.ps1"), "utf8");
  assert.match(native, /pnpm exec tauri build --debug --bundles nsis --no-sign/);
  assert.doesNotMatch(native, /beforeBuildCommand\s*=|beforeBundleCommand\s*=/, "the native fixture must not replace the production hooks");
  assert.match(native, /sidecarProof\.phase -ne 'bundle'/);
});

test("native acceptance refuses local execution before any product operation", { skip: process.platform !== "win32" }, () => {
  for (const args of [
    ["scripts/build-native-windows-fixture.ps1", "-Version", "0.2.0-alpha.1", "-Destination", "unused-local-fixture.exe"],
    ["scripts/test-native-windows-upgrade.ps1", "-Scenario", "LegacyUninstall"],
  ]) {
    const result = spawnSync("pwsh", ["-NoProfile", "-File", ...args], {
      cwd: root, encoding: "utf8", env: { ...process.env, GITHUB_ACTIONS: "false", RUNNER_ENVIRONMENT: "local" },
    });
    assert.equal(result.error, undefined);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /GitHub-hosted/);
  }
});
