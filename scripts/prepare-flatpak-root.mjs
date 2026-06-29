import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appId = "io.github.qiyuanhuakai.dezirclip";
const root = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(root, "..");
const debDir = join(repoRoot, "src-tauri", "target", "release", "bundle", "deb");
const flatpakDir = join(repoRoot, "flatpak");
const buildRoot = join(flatpakDir, "build-root");

const debFile = readdirSync(debDir)
  .filter((name) => name.endsWith(".deb"))
  .sort()
  .at(-1);

if (!debFile) {
  throw new Error(`No .deb bundle found in ${debDir}`);
}

rmSync(buildRoot, { recursive: true, force: true });
mkdirSync(buildRoot, { recursive: true });

execFileSync("dpkg-deb", ["-x", join(debDir, debFile), buildRoot], {
  cwd: repoRoot,
  stdio: "inherit",
});

const applicationsDir = join(buildRoot, "usr", "share", "applications");
const sourceDesktop = join(applicationsDir, "DezirClip.desktop");
const targetDesktop = join(applicationsDir, `${appId}.desktop`);

if (existsSync(sourceDesktop)) {
  const desktopContent = readFileSync(sourceDesktop, "utf8")
    .replace(/^Icon=.*$/m, `Icon=${appId}`)
    .replace(/^Exec=.*$/m, "Exec=dezirclip");
  writeFileSync(targetDesktop, desktopContent);
  rmSync(sourceDesktop);
}

const iconDir = join(buildRoot, "usr", "share", "icons", "hicolor", "512x512", "apps");
mkdirSync(iconDir, { recursive: true });
copyFileSync(
  join(repoRoot, "src-tauri", "icons", "icon.png"),
  join(iconDir, `${appId}.png`),
);
