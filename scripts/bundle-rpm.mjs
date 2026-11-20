import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export function rpmBundleConfig(version) {
  const number = "(0|[1-9][0-9]*)";
  const match = new RegExp(`^${number}\\.${number}\\.${number}(?:-(alpha|beta|rc)\\.${number})?(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?$`).exec(version);
  if (!match || match[0] !== version) throw new Error(`Unsupported RPM release version: ${version}`);
  // Tauri 2.12.3 forwards its SemVer unchanged to RPM, which rejects '-'.
  // Only this bundle process uses a base version and a pre-stable release.
  return {
    version: `${match[1]}.${match[2]}.${match[3]}`,
    bundle: {
      createUpdaterArtifacts: false,
      linux: { rpm: { release: match[4] ? `0.${match[4]}.${match[5]}` : "1" } },
    },
  };
}

export function bundleRpm({ repoRoot = root, platform = process.platform, env = process.env, run = execFileSync, reuseSidecars = false } = {}) {
  if (platform !== "linux") return { skipped: true, reason: "RPM bundling requires a Linux host" };
  if (env.CARGO_BUILD_TARGET || env.TAURI_ENV_TARGET_TRIPLE) throw new Error("RPM wrapper supports native Linux release bundles only");
  const pkg = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));
  const config = JSON.parse(readFileSync(join(repoRoot, "src-tauri/tauri.conf.json"), "utf8"));
  if (pkg.version !== config.version) throw new Error("App and package versions must match before RPM bundling");
  const override = rpmBundleConfig(pkg.version);
  const metadata = JSON.parse(run("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"], { cwd: repoRoot, env, encoding: "utf8" }));
  if (!isAbsolute(metadata.target_directory)) throw new Error("Cargo target directory must be absolute");
  if (reuseSidecars) {
    const target = run("rustc", ["--print", "host-tuple"], { cwd: repoRoot, env, encoding: "utf8" }).trim();
    const proof = JSON.parse(readFileSync(join(repoRoot, "src-tauri/binaries", `dzc-${target}.proof.json`), "utf8"));
    if (proof.phase !== "bundle" || proof.profile !== "release" || proof.target !== target
        || proof.declaredBuildSet?.sourceRevision !== env.DZC_BUILD_REVISION) {
      throw new Error("Previous Tauri bundle did not prepare the current release sidecars");
    }
    for (const [name, sha256] of [["dzc", proof.sha256], ["dzc-service", proof.serviceHelper?.sha256]]) {
      const path = join(metadata.target_directory, "release", name);
      if (createHash("sha256").update(readFileSync(path)).digest("hex") !== sha256) throw new Error(`RPM sidecar differs: ${name}`);
    }
    override.build = { beforeBundleCommand: null };
  }
  run("pnpm", ["exec", "tauri", "bundle", "--bundles", "rpm", "--config", JSON.stringify(override)], { cwd: repoRoot, env, stdio: "inherit" });
  const directory = join(metadata.target_directory, "release/bundle/rpm");
  const release = override.bundle.linux.rpm.release;
  const prefix = `${config.productName}-${override.version}-${release}.`;
  const artifacts = readdirSync(directory).filter((name) => name.startsWith(prefix) && name.endsWith(".rpm"));
  if (artifacts.length !== 1) throw new Error("Expected exactly one RPM artifact for the selected release");
  const artifact = join(directory, artifacts[0]);
  const bytes = statSync(artifact).size;
  if (!bytes) throw new Error("RPM artifact is empty");
  return { skipped: false, appVersion: pkg.version, rpmVersion: override.version, rpmRelease: release, artifact, bytes, sha256: createHash("sha256").update(readFileSync(artifact)).digest("hex") };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const flags = process.argv.slice(2);
  const reuseSidecars = flags.includes("--reuse-sidecars");
  const args = flags.filter(value => value !== "--reuse-sidecars");
  if (args.length && (args.length !== 2 || args[0] !== "--receipt")) throw new Error("Usage: node scripts/bundle-rpm.mjs [--reuse-sidecars] [--receipt <path>]");
  const result = bundleRpm({ reuseSidecars });
  if (args.length) writeFileSync(args[1], JSON.stringify(result, null, 2) + "\n");
  console.log(JSON.stringify(result, null, 2));
}
