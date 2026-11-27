import { execFileSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(import.meta.url);
const digest = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

export function sidecarOptions(args, env) {
  let profile = env.TAURI_ENV_DEBUG === "true" ? "debug" : "release";
  if (![undefined, "", "true", "false"].includes(env.TAURI_ENV_DEBUG)) throw new Error("Invalid TAURI_ENV_DEBUG value");
  let explicitProfile;
  let portable = false;
  let withDesktop = false;
  let phase = "manual";
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--portable") portable = true;
    else if (arg === "--with-desktop") withDesktop = true;
    else if (arg === "--debug" || arg === "--release") {
      const requested = arg.slice(2);
      if (explicitProfile && explicitProfile !== requested) throw new Error("Conflicting sidecar profiles");
      explicitProfile = requested;
      profile = requested;
    } else if (arg === "--phase" && ["build", "bundle", "portable", "manual"].includes(args[index + 1])) phase = args[++index];
    else throw new Error(`Unknown sidecar option: ${arg}`);
  }
  if (withDesktop && (phase !== "bundle" || portable)) throw new Error("A shared Cargo build requires desktop bundle preparation");
  return { profile, portable, phase, ...(withDesktop ? { withDesktop } : {}) };
}

export function prepareSidecars({ args = process.argv.slice(2), env = process.env, run = execFileSync, repoRoot = resolve(dirname(script), "..") } = {}) {
  const options = sidecarOptions(args, env);
  const hostTriple = run("rustc", ["--print", "host-tuple"], { cwd: repoRoot, encoding: "utf8" }).trim();
  const targetTriple = env.TAURI_ENV_TARGET_TRIPLE || hostTriple;
  for (const triple of [hostTriple, targetTriple]) {
    if (!/^[a-zA-Z0-9_.-]+$/.test(triple) || !(triple.includes("linux") || triple.includes("windows"))) throw new Error("Unsupported sidecar target triple");
  }
  const extension = targetTriple.includes("windows") ? ".exe" : "";
  const metadata = JSON.parse(run("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"], { cwd: repoRoot, encoding: "utf8" }));
  if (typeof metadata.target_directory !== "string" || !metadata.target_directory) throw new Error("Cargo target directory is unavailable");
  const targetDirectory = resolve(repoRoot, metadata.target_directory);
  // The Tauri CLI's build path can copy external binaries into its own
  // target directory. The release workflow calls only the bundle command.
  const producer = options.withDesktop ? targetDirectory : join(targetDirectory, "sidecar-build");
  const cargoArgs = ["build", "--locked", "-p", "dzc-cli", "-p", "dzc-service", "--bin", "dzc", "--bin", "dzc-service",
    "--no-default-features", "--target", targetTriple, "--target-dir", producer,
    "--features", "dzc-cli/native-screen,dzc-service/native-screen"];
  if (options.profile === "release") cargoArgs.push("--release");
  if (options.withDesktop) cargoArgs.push("-p", "dezirclip", "--bin", "dezirclip",
    "--features", "dezirclip/desktop,tauri/custom-protocol");
  if (options.portable) cargoArgs.push("--features", "dzc-cli/portable,dzc-service/portable");
  run("cargo", cargoArgs, { cwd: repoRoot, stdio: "inherit" });
  const source = join(producer, targetTriple, options.profile, `dzc${extension}`);
  const sha256 = digest(source);
  const version = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8")).version;
  let buildInfo = null;
  let declaredRevision = env.DZC_BUILD_REVISION || null;
  if (declaredRevision && !/^[a-fA-F0-9]{40}$/.test(declaredRevision)) throw new Error("Invalid declared build revision");
  if (declaredRevision) declaredRevision = declaredRevision.toLowerCase();
  if (targetTriple === hostTriple) {
    buildInfo = JSON.parse(run(source, ["build-info", "--json"], { cwd: repoRoot, encoding: "utf8" }));
    const revisionValid = buildInfo.source_revision === null
      ? buildInfo.build_id === null && buildInfo.source_state === "unknown"
      : typeof buildInfo.source_revision === "string" && /^[a-f0-9]{40}$/.test(buildInfo.source_revision)
        && ["verified_checkout", "modified_checkout"].includes(buildInfo.source_state);
    if (buildInfo.component !== "cli" || buildInfo.component_version !== version || buildInfo.target !== targetTriple
        || !revisionValid
        || (buildInfo.source_revision && buildInfo.build_id !== `${version}@${buildInfo.source_revision}`)
        || (declaredRevision && buildInfo.source_revision !== declaredRevision.toLowerCase())) throw new Error("Standalone CLI build identity differs from its package build");
    declaredRevision = buildInfo.source_revision;
  } else if (!declaredRevision) {
    try { declaredRevision = run("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim(); }
    catch { declaredRevision = null; }
    if (declaredRevision && !/^[a-fA-F0-9]{40}$/.test(declaredRevision)) declaredRevision = null;
  }
  function stage(name, source, sha256) {
    const paths = [{ role: "cargo-target", path: source, sha256 }];
    const copies = [{ role: "external-bin", path: join(repoRoot, "src-tauri", "binaries", name + "-" + targetTriple + extension) }];
    if (options.phase === "portable") {
      if (!options.portable || targetTriple !== hostTriple) throw new Error("Portable packaging requires native portable sidecars");
      copies.push({ role: "portable-runtime", path: join(targetDirectory, options.profile, name + extension) });
    }
    for (const copy of copies) {
      mkdirSync(dirname(copy.path), { recursive: true });
      const temporary = copy.path + ".sidecar-" + randomUUID() + ".tmp";
      try {
        copyFileSync(source, temporary);
        if (digest(temporary) !== sha256) throw new Error("Sidecar copy changed before publication");
        renameSync(temporary, copy.path);
      } finally {
        if (existsSync(temporary)) unlinkSync(temporary);
      }
      if (digest(copy.path) !== sha256) throw new Error("Staged sidecar differs from the producer output");
      paths.push({ ...copy, sha256 });
    }
    if (digest(source) !== sha256) throw new Error("Sidecar staging modified the producer output");
    return paths;
  }

  const paths = stage("dzc", source, sha256);
  const staged = paths.find(item => item.role === "external-bin").path;
  const proof = {
    schemaVersion: 2, kind: "dezirclip-standalone-sidecar", createdAt: new Date().toISOString(),
    ...options, target: targetTriple, host: hostTriple, command: ["cargo", ...cargoArgs], source, sha256, binaryPaths: paths,
    buildInfo, buildInfoVerification: buildInfo ? "executed_native_cli" : "cross_target_not_executed",
    declaredBuildSet: { version, sourceRevision: declaredRevision, buildId: declaredRevision ? `${version}@${declaredRevision}` : null },
    helperSha256: digest(script), cargoLockSha256: digest(join(repoRoot, "Cargo.lock")),
  };
  const serviceSource = join(producer, targetTriple, options.profile, `dzc-service${extension}`);
  const serviceSha256 = digest(serviceSource);
  let serviceInfo = null;
  if (targetTriple === hostTriple) {
    serviceInfo = JSON.parse(run(serviceSource, ["--build-info"], { cwd: repoRoot, encoding: "utf8" }));
    if (serviceInfo.component !== "service" || serviceInfo.component_version !== version || serviceInfo.target !== targetTriple
      || serviceInfo.source_revision !== declaredRevision || serviceInfo.build_id !== buildInfo.build_id
      || serviceInfo.source_state !== buildInfo.source_state
      || !serviceInfo.linked_components?.some((item) => item.component === "dzc-host" && item.version === version)
      || !serviceInfo.linked_components?.some((item) => item.component === "dzc-service" && item.version === version)) throw new Error("Service build identity differs from its package build");
  }
  const servicePaths = stage("dzc-service", serviceSource, serviceSha256);
  proof.serviceHelper = { source: serviceSource, sha256: serviceSha256, binaryPaths: servicePaths, command: proof.command, buildInfo: serviceInfo,
    buildInfoVerification: serviceInfo ? "executed_native_service" : "cross_target_not_executed" };
  writeFileSync(staged + ".proof.json", JSON.stringify(proof, null, 2) + "\n");
  console.log(`Verified standalone sidecar inputs (${options.phase}, ${options.profile}, ${targetTriple}): ${sha256}`);
  return proof;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(script)) prepareSidecars();
