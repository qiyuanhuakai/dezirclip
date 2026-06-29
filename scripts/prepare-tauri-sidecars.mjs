import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(root, "..");
const hostTriple = execFileSync("rustc", ["--print", "host-tuple"], {
  cwd: repoRoot,
  encoding: "utf8",
}).trim();
const targetTriple = process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple;
const isWindowsTarget = targetTriple.includes("windows");
const extension = isWindowsTarget ? ".exe" : "";
const cargoArgs = ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "dzc", "--release"];
const releaseDir = targetTriple === hostTriple
  ? join(repoRoot, "src-tauri", "target", "release")
  : join(repoRoot, "src-tauri", "target", targetTriple, "release");

if (targetTriple !== hostTriple) {
  cargoArgs.push("--target", targetTriple);
}

execFileSync("cargo", cargoArgs, {
  cwd: repoRoot,
  stdio: "inherit",
});

const binariesDir = join(repoRoot, "src-tauri", "binaries");
mkdirSync(binariesDir, { recursive: true });

copyFileSync(
  join(releaseDir, `dzc${extension}`),
  join(binariesDir, `dzc-${targetTriple}${extension}`),
);
