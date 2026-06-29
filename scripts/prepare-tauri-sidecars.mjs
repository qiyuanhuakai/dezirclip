import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(root, "..");
const extension = process.platform === "win32" ? ".exe" : "";
const targetTriple = execFileSync("rustc", ["--print", "host-tuple"], {
  cwd: repoRoot,
  encoding: "utf8",
}).trim();

execFileSync("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "dzc", "--release"], {
  cwd: repoRoot,
  stdio: "inherit",
});

const binariesDir = join(repoRoot, "src-tauri", "binaries");
mkdirSync(binariesDir, { recursive: true });

copyFileSync(
  join(repoRoot, "src-tauri", "target", "release", `dzc${extension}`),
  join(binariesDir, `dzc-${targetTriple}${extension}`),
);
