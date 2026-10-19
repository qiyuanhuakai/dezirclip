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
  let phase = "manual";
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--portable") portable = true;
    else if (arg === "--debug" || arg === "--release") {
      const requested = arg.slice(2);
      if (explicitProfile && explicitProfile !== requested) throw new Error("Conflicting sidecar profiles");
      explicitProfile = requested;
      profile = requested;
    } else if (arg === "--phase" && ["build", "bundle", "portable", "manual"].includes(args[index + 1])) phase = args[++index];
    else throw new Error(`Unknown sidecar option: ${arg}`);
  }
  return { profile, portable, phase };
}

export function prepareSidecars({ args = process.argv.slice(2), env = process.env, run = execFileSync, repoRoot = resolve(dirname(script), "..") } = {}) {
  const options = sidecarOptions(args, env);
  const hostTriple = run("rustc", ["--print", "host-tuple"], { cwd: repoRoot, encoding: "utf8" }).trim();
  const targetTriple = env.TAURI_ENV_TARGET_TRIPLE || hostTriple;
  for (const triple of [hostTriple, targetTriple]) {
    if (!/^[a-zA-Z0-9_.-]+$/.test(triple) || !(triple.includes("linux") || triple.includes("windows"))) throw new Error("Unsupported sidecar target triple");
  }
  const extension = targetTriple.includes("windows") ? ".exe" : "";
  const metadata = JSON.parse(run("cargo", ["metadata", "--manifest-path", "src-tauri/Cargo.toml", "--format-version", "1", "--no-deps"], { cwd: repoRoot, encoding: "utf8" }));
  if (typeof metadata.target_directory !== "string" || !metadata.target_directory) throw new Error("Cargo target directory is unavailable");
  const targetDirectory = resolve(repoRoot, metadata.target_directory);
  // An explicit target avoids CARGO_BUILD_TARGET/config changing the source
  // namespace. Hook environment does not distinguish an explicit host target.
  const cargoArgs = ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "dzc", "--locked", "--no-default-features", "--target", targetTriple];
  if (options.profile === "release") cargoArgs.push("--release");
  if (options.portable) cargoArgs.push("--features", "portable");
  run("cargo", cargoArgs, { cwd: repoRoot, stdio: "inherit" });
  const source = join(targetDirectory, targetTriple, options.profile, `dzc${extension}`);
  const sha256 = digest(source);
  const staged = join(repoRoot, "src-tauri", "binaries", `dzc-${targetTriple}${extension}`);
  const paths = [{ role: "cargo-target", path: source }, { role: "external-bin", path: staged }];
  if (targetTriple === hostTriple) paths.push({ role: "cargo-host-default", path: join(targetDirectory, options.profile, `dzc${extension}`) });
  for (const item of paths) {
    mkdirSync(dirname(item.path), { recursive: true });
    if (resolve(item.path) !== resolve(source)) {
      const temporary = item.path + `.sidecar-${randomUUID()}.tmp`;
      try {
        copyFileSync(source, temporary);
        if (digest(temporary) !== sha256) throw new Error("Sidecar copy changed before publication");
        // Replace the directory entry rather than writing through Cargo's
        // possible hardlink to a cached desktop-feature executable.
        renameSync(temporary, item.path);
      } finally {
        if (existsSync(temporary)) unlinkSync(temporary);
      }
    }
    item.sha256 = digest(item.path);
    if (item.sha256 !== sha256) throw new Error(`Sidecar input differs from standalone build: ${item.role}`);
  }
  const proof = {
    schemaVersion: 1, kind: "dezirclip-standalone-sidecar", createdAt: new Date().toISOString(),
    ...options, target: targetTriple, host: hostTriple, command: ["cargo", ...cargoArgs], source, sha256, binaryPaths: paths,
    helperSha256: digest(script), cargoLockSha256: digest(join(repoRoot, "Cargo.lock")),
  };
  writeFileSync(staged + ".proof.json", JSON.stringify(proof, null, 2) + "\n");
  console.log(`Verified standalone sidecar inputs (${options.phase}, ${options.profile}, ${targetTriple}): ${sha256}`);
  return proof;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(script)) prepareSidecars();
