import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, linkSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { once } from "node:events";
import { performance } from "node:perf_hooks";
import { setImmediate as nextTurn } from "node:timers/promises";
import { prepareSidecars, sidecarOptions } from "../prepare-tauri-sidecars.mjs";
import { bundleRpm, rpmBundleConfig } from "../bundle-rpm.mjs";
import { nativeWriterTimeoutMs, waitForNativeProcess } from "./native-process-deadline.mjs";

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
  const serviceSource = join(targetDirectory, target, profile, `dzc-service${extension}`);
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
    if (command === source || command === serviceSource) {
      assert.deepEqual(args, command === source ? ["build-info", "--json"] : ["--build-info"]);
      return JSON.stringify({ component: command === source ? "cli" : "service", component_version: "0.2.0-alpha.1", target,
        linked_components: ["dzc-host", "dzc-service"].map((component) => ({component,version:"0.2.0-alpha.1"})),
        source_revision: "a".repeat(40), build_id: "0.2.0-alpha.1@" + "a".repeat(40), source_state: "verified_checkout" });
    }
    if (command === "git") return "a".repeat(40);
    assert.equal(command, "cargo");
    if (args[0] === "metadata") return JSON.stringify({ target_directory: targetDirectory });
    assert(["dzc", "dzc-service"].includes(args[4]));
    assert.deepEqual(args, ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", args[4], "--locked", "--no-default-features", "--target", target,
      ...(profile === "release" ? ["--release"] : []), ...(portable ? ["--features", "portable"] : [])]);
    if (!omitOutput) {
      mkdirSync(dirname(source), { recursive: true });
      writeFileSync(args[4] === "dzc" ? source : serviceSource, args[4] === "dzc" ? payload : Buffer.from("service:"+payload.toString()));
    }
  };
  return { directory, targetDirectory, source, serviceSource, hostDefault, cache, payload, calls, run, host, target };
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
    assert.equal(f.calls.length, 6, "each standalone helper has its own native build identity query");
    assert.equal(proof.buildInfoVerification, "executed_native_cli");
    assert.equal(proof.buildInfo.component, "cli");
    assert.equal(proof.serviceHelper.buildInfo.component, "service");
    assert.equal(proof.serviceHelper.binaryPaths.length, 3);
    for (const item of proof.serviceHelper.binaryPaths) {
      assert.equal(hash(readFileSync(item.path)), proof.serviceHelper.sha256);
      assert.deepEqual(readFileSync(item.path), Buffer.from("service:"+f.payload.toString()));
    }
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
  assert.equal(pkg.scripts["tauri:build"], "tauri build", "forwarded build arguments must retain their original Tauri command");
  assert.equal(pkg.scripts["tauri:bundle:rpm"], "node scripts/bundle-rpm.mjs");
  assert.deepEqual(config.bundle.targets, ["nsis", "deb", "appimage"]);
  const workflow = readFileSync(join(root, ".github/workflows/release.yml"), "utf8");
  const rpmStep = workflow.split("- name: Bundle and upload RPM with pre-release package ordering")[1]?.split("\n  publish-flatpak:")[0];
  assert(rpmStep);
  assert.match(rpmStep, /if: runner\.os == 'Linux'/);
  assert.match(rpmStep, /node scripts\/bundle-rpm\.mjs --receipt/);
  assert.match(rpmStep, /needs\.create-release\.outputs\.release_id/);
  assert.match(rpmStep, /--jq '\.draft'\)" = true/);
  assert.match(rpmStep, /gh release upload "\$GITHUB_REF_NAME" "\$rpm_path" --clobber/);
  const identities = workflow.split("  collect-release-identities:")[1]?.split("\n  verify-publish-and-promote:")[0];
  assert(identities, "publication requires the actual final-package identity workflow");
  assert.match(identities, /needs: \[create-release, publish-tauri, publish-standalone-cli, publish-flatpak\]/);
  assert.match(identities, /uses: \.\/\.github\/workflows\/release-identities\.yml/);
  assert.match(identities, /source: \$\{\{ github\.sha \}\}/);
  assert.match(identities, /release-id: \$\{\{ needs\.create-release\.outputs\.release_id \}\}/);
  const upgrades = workflow.split("  verify-final-windows-upgrade:")[1]?.split("\n  verify-publish-and-promote:")[0];
  assert(upgrades, "publication requires actual final signed NSIS installation and upgrade acceptance");
  assert.match(upgrades, /needs: \[create-release, publish-tauri\]/);
  assert.match(upgrades, /uses: \.\/\.github\/workflows\/release-windows-upgrade\.yml/);
  assert.match(upgrades, /source: \$\{\{ github\.sha \}\}/);
  assert.match(upgrades, /release-id: \$\{\{ needs\.create-release\.outputs\.release_id \}\}/);
  assert.match(workflow.split("  verify-publish-and-promote:")[1], /needs: \[create-release, publish-tauri, publish-standalone-cli, publish-flatpak, collect-release-identities, verify-final-windows-upgrade\]/);
});

test("RPM metadata derives supported preview and stable versions without build metadata precedence", () => {
  for (const [app, version, release] of [
    ["0.2.0-alpha.0", "0.2.0", "0.alpha.0"], ["0.2.0-alpha.10+ci.22", "0.2.0", "0.alpha.10"],
    ["0.2.0-beta.2", "0.2.0", "0.beta.2"], ["0.2.0-rc.1", "0.2.0", "0.rc.1"],
    ["0.2.0", "0.2.0", "1"], ["1.12.300+linux-x64.001", "1.12.300", "1"],
  ]) {
    const override = rpmBundleConfig(app);
    assert.equal(override.version, version);
    assert.equal(override.bundle.linux.rpm.release, release);
    assert.equal(override.bundle.createUpdaterArtifacts, false, "RPM remains a package-manager update path");
  }
  for (const version of ["0.2.0-alpha.01", "0.2.0-preview.1", "0.2.0-alpha-beta.1", "0.2.0-1", "0.2.0-alpha", "01.2.0", "v0.2.0", "0.2.0+", "0.2.0-alpha.1+bad..metadata", "0.2.0-alpha.1\n"]) {
    assert.throws(() => rpmBundleConfig(version), /Unsupported RPM/);
  }
});

test("RPM wrapper invokes only the bundle process and keeps source versions and prepared CLI bytes", (t) => {
  const f = fixture(t, { host: "x86_64-unknown-linux-gnu" });
  prepareSidecars({ repoRoot: f.directory, run: f.run, args: ["--phase", "bundle"], env: {} });
  mkdirSync(join(f.directory, "src-tauri"), { recursive: true });
  const config = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  config.version = JSON.parse(readFileSync(join(f.directory, "package.json"), "utf8")).version;
  writeFileSync(join(f.directory, "src-tauri/tauri.conf.json"), JSON.stringify(config));
  const protectedPaths = [join(f.directory, "package.json"), join(f.directory, "src-tauri/tauri.conf.json"), join(f.directory, "Cargo.lock"), f.hostDefault];
  const before = protectedPaths.map((path) => hash(readFileSync(path)));
  const env = { CARGO_TARGET_DIR: f.targetDirectory };
  let calls = 0;
  const rpm = Buffer.from("isolated fake RPM output for wrapper invocation contract");
  const result = bundleRpm({ repoRoot: f.directory, platform: "linux", env, run: (command, args, options) => {
    calls += 1;
    assert.equal(options.cwd, f.directory);
    assert.equal(options.env, env);
    if (command === "cargo") {
      assert.deepEqual(args, ["metadata", "--format-version", "1", "--no-deps", "--locked"]);
      return JSON.stringify({ target_directory: f.targetDirectory });
    }
    assert.equal(command, "pnpm");
    assert.deepEqual(args.slice(0, 6), ["exec", "tauri", "bundle", "--bundles", "rpm", "--config"]);
    assert.deepEqual(JSON.parse(args[6]), rpmBundleConfig("0.2.0-alpha.1"));
    assert.equal(options.stdio, "inherit");
    const directory = join(f.targetDirectory, "release/bundle/rpm");
    mkdirSync(directory, { recursive: true });
    writeFileSync(join(directory, "DezirClip-0.2.0-0.alpha.1.aarch64.rpm"), rpm);
  } });
  assert.equal(calls, 2);
  assert.equal(result.appVersion, "0.2.0-alpha.1");
  assert.equal(result.rpmVersion, "0.2.0");
  assert.equal(result.rpmRelease, "0.alpha.1");
  assert.equal(result.sha256, hash(rpm));
  assert.equal(result.bytes, rpm.length);
  assert.deepEqual(protectedPaths.map((path) => hash(readFileSync(path))), before);
  assert.deepEqual(readFileSync(f.hostDefault), f.payload);
});

test("RPM wrapper skips Windows before reading files or starting any command", () => {
  const result = bundleRpm({ repoRoot: "missing-fixture", platform: "win32", run: () => { throw new Error("must not execute"); } });
  assert.equal(result.skipped, true);
  for (const env of [{ CARGO_BUILD_TARGET: "aarch64-unknown-linux-gnu" }, { TAURI_ENV_TARGET_TRIPLE: "aarch64-unknown-linux-gnu" }]) {
    assert.throws(() => bundleRpm({ repoRoot: "missing-fixture", platform: "linux", env, run: () => { throw new Error("must not execute"); } }), /native Linux release/);
  }
});

test("RPM wrapper rejects unsupported versions and preserves bundle failures", (t) => {
  const f = fixture(t);
  mkdirSync(join(f.directory, "src-tauri"));
  const configPath = join(f.directory, "src-tauri/tauri.conf.json");
  const writeVersion = (version) => {
    writeFileSync(join(f.directory, "package.json"), JSON.stringify({ version }));
    writeFileSync(configPath, JSON.stringify({ version, productName: "DezirClip" }));
  };
  writeVersion("0.2.0-preview.1");
  assert.throws(() => bundleRpm({ repoRoot: f.directory, platform: "linux", run: () => { throw new Error("must not execute"); } }), /Unsupported RPM/);
  writeVersion("0.2.0-alpha.1");
  const failure = new Error("actual bundler failure");
  assert.throws(() => bundleRpm({ repoRoot: f.directory, platform: "linux", run: (command) => {
    if (command === "cargo") return JSON.stringify({ target_directory: f.targetDirectory });
    throw failure;
  } }), (error) => error === failure);
  writeFileSync(configPath, JSON.stringify({ version: "0.2.0", productName: "DezirClip" }));
  assert.throws(() => bundleRpm({ repoRoot: f.directory, platform: "linux" }), /versions must match/);
});

for (const parameters of [
  { host: "x86_64-pc-windows-msvc" },
  { host: "x86_64-pc-windows-msvc", target: "aarch64-pc-windows-msvc" },
  { host: "x86_64-unknown-linux-gnu" },
  { host: "x86_64-unknown-linux-gnu", target: "aarch64-unknown-linux-gnu" },
]) {
  test(`Cargo bundle binaries contain one prepared CLI: ${parameters.target ?? parameters.host}`, (t) => {
    const config = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
    const manifest = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
    const main = /^default-run\s*=\s*"([^"]+)"/m.exec(manifest)?.[1];
    const binaries = manifest.split(/\r?\n\[\[bin\]\]\r?\n/).slice(1).map((section) => {
      const name = /^name\s*=\s*"([^"]+)"/m.exec(section)?.[1];
      const source = /^path\s*=\s*"([^"]+)"/m.exec(section)?.[1];
      assert(name && source && existsSync(join(root, "src-tauri", source)));
      return { name, main: name === main };
    });
    assert.deepEqual(binaries.filter((item) => item.main).map((item) => item.name), ["dezirclip"]);
    assert.deepEqual(binaries.filter((item) => !item.main).map((item) => item.name), ["dzc", "dzc-service"]);
    assert.deepEqual(readdirSync(join(root, "src-tauri/src/bin")).sort(), ["dzc-service.rs", "dzc.rs"]);

    const f = fixture(t, parameters);
    const proof = prepareSidecars({ repoRoot: f.directory, run: f.run, args: ["--phase", "bundle"], env: { TAURI_ENV_TARGET_TRIPLE: f.target } });
    const out = f.target === f.host ? dirname(f.hostDefault) : dirname(f.source);
    const extension = f.target.includes("windows") ? ".exe" : "";
    const mainPayload = Buffer.from("desktop-main-binary");
    writeFileSync(join(out, `dezirclip${extension}`), mainPayload);
    const cargoFiles = binaries.map((item) => ({ name: item.name + extension, path: join(out, item.name + extension) }));
    const externalFiles = (config.bundle.externalBin ?? []).map((name) => name.split("/").at(-1) + extension);
    const destinations = [...cargoFiles.map((item) => item.name), ...externalFiles];
    assert.equal(new Set(destinations).size, destinations.length, "RPM adds Cargo and external files separately and rejects duplicate destinations");
    assert.equal(destinations.filter((name) => name === `dzc${extension}`).length, 1);
    assert.equal(new Set([...destinations, `dzc${extension}`]).size, destinations.length, "the old externalBin entry would repeat the Cargo CLI destination");
    const bundledCli = cargoFiles.find((item) => item.name === `dzc${extension}`);
    assert.deepEqual(readFileSync(bundledCli.path), f.payload, "the native/cross Cargo bundle input must use the prepared standalone bytes");
    assert.equal(hash(readFileSync(bundledCli.path)), proof.sha256);
    const bundledService = cargoFiles.find((item) => item.name === `dzc-service${extension}`);
    assert.equal(destinations.filter((name) => name === bundledService.name).length, 1);
    assert.equal(hash(readFileSync(bundledService.path)), proof.serviceHelper.sha256);
    assert.deepEqual(readFileSync(cargoFiles.find((item) => item.name === `dezirclip${extension}`).path), mainPayload);
    assert.deepEqual(readFileSync(proof.binaryPaths.find((item) => item.role === "external-bin").path), f.payload, "the diagnostic staging copy retains the same bytes without a second bundle registration");
  });
}

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

const nativeObservation = () => ({ status: "starting", stdout: "", stderr: "" });

test("native writer wait retains output and settles once for real process exits", async () => {
  assert.equal(nativeWriterTimeoutMs, 15000);
  for (const exitCode of [0, 7]) {
    const child = spawn(process.execPath, ["-e", `process.stdout.write('synthetic-output'); process.stderr.write('synthetic-error'); process.exitCode=${exitCode}`],
      { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    const observation = nativeObservation();
    const waiting = waitForNativeProcess(child, observation);
    if (exitCode) await assert.rejects(waiting, /failed \(7\)/);
    else await waiting;
    assert.equal(observation.exitCode, exitCode);
    assert.equal(observation.status, exitCode ? "failed" : "passed");
    assert.equal(observation.stdout, "synthetic-output");
    assert.equal(observation.stderr, "synthetic-error");
  }
});

async function runningNativeChild(t) {
  const source = "process.stdout.write('synthetic-before-timeout'); process.send({ ready: true }); setInterval(() => {}, 1000)";
  const child = spawn(process.execPath, ["-e", source], { windowsHide: true, stdio: ["ignore", "pipe", "pipe", "ipc"] });
  const kill = child.kill.bind(child);
  t.after(() => {
    kill();
    child.stdout.destroy();
    child.stderr.destroy();
    if (child.connected) child.disconnect();
    child.unref();
  });
  await once(child, "message", { signal: AbortSignal.timeout(10000) });
  return { child, kill };
}

test("native writer deadline rejects immediately when its real process cannot be terminated", async t => {
  const { child, kill } = await runningNativeChild(t);
  child.kill = () => false;
  const observation = nativeObservation();
  const started = performance.now();
  await assert.rejects(waitForNativeProcess(child, observation, 150), /timed out/);
  assert(performance.now() - started < 1000, "Cleanup must not add the former two-second grace period");
  assert.equal(observation.status, "timeout");
  await nextTurn();
  assert.equal(observation.killRequested, false);
  assert.equal(child.exitCode, null, "The controlled child is still alive after the failed termination request");
  assert.equal(observation.stdout, "synthetic-before-timeout");
  const finishedAt = observation.finishedAt;
  child.ref();
  const closed = once(child, "close", { signal: AbortSignal.timeout(5000) });
  kill();
  await closed;
  assert.equal(observation.status, "timeout");
  assert.equal(observation.finishedAt, finishedAt);
  assert.equal(observation.exitCode, null);
});
