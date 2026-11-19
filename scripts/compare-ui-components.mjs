import { execFile } from "node:child_process";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { chromium } from "playwright";
import { createServer } from "vite";

const execute = promisify(execFile);
const root = fileURLToPath(new URL("..", import.meta.url));
const flags = new URLSearchParams(process.argv.slice(2).join("&"));
const native = flags.get("native");
if (!native) throw Error("Pass native=<path to dzc-ui-gallery>");
const selectedTheme = flags.get("theme");
const selectedFixture = flags.get("fixture");
const presets = JSON.parse(await readFile(resolve(root, "packages/ui-native/theme/builtin-presets.json"), "utf8"));
const cases = {
  "settings-rail": { states: ["default", "selected"], text: "Appearance" },
  "settings-footer": { states: ["default", "compact"], text: "Reset defaults" },
  "history-card": { states: ["default", "selected", "pinned"], text: "Copied from the active editor" },
  "app-header": { states: ["default", "pinned", "settings"], text: "DezirClip" },
  select: { states: ["default", "disabled", "hover", "focus", "popup"], text: "System" },
  "search-bar": { states: ["default", "filled", "loading", "error"], text: "FTS5" },
  filters: { states: ["default"], text: "Clear" },
  highlight: { states: ["default"], text: "Clipboard history" },
  "search-history": { states: ["default", "empty"], text: "Documents" },
  disclosure: { states: ["default", "expanded"], text: "Software updates" },
  help: { states: ["default", "expanded"], text: "Updates are checked when the app starts." },
  progress: { states: ["default", "done", "error"], text: "Importing" },
  toast: { states: ["default"], text: "Copied to clipboard" },
  confirm: { states: ["default"], text: "Clear history" },
  "tag-dialog": { states: ["default", "busy"], text: "Create tag" },
};
if (selectedTheme && !(selectedTheme in presets)) throw Error("Unknown theme " + selectedTheme);
if (selectedFixture && !(selectedFixture in cases)) throw Error("Unknown component " + selectedFixture);
const server = await createServer({ root, configFile: false, server: { host: "127.0.0.1", port: 0, hmr: false, watch: null } });
const output = await mkdtemp(join(tmpdir(), "deziro-ui-parity-"));
const size = { width: 560, height: 420 };
const reportPath = join(output, "results.json");

function comparePixels({ web, native }) {
  const decode = async base64 => {
    const image = new Image();
    image.src = "data:image/png;base64," + base64;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d");
    // Filter single-pixel text and border antialiasing differences between
    // the Chromium rasterizer and the native renderer.
    context.filter = "blur(1px)";
    context.drawImage(image, 0, 0);
    return context.getImageData(0, 0, image.width, image.height).data;
  };
  return Promise.all([decode(web), decode(native)]).then(([a, b]) => {
    if (a.length !== b.length) throw Error("Renderer image sizes differ");
    const distance = (rgb1, offset1, rgb2, offset2) =>
      (Math.abs(rgb1[offset1] - rgb2[offset2]) + Math.abs(rgb1[offset1 + 1] - rgb2[offset2 + 1])
        + Math.abs(rgb1[offset1 + 2] - rgb2[offset2 + 2])) / 3;
    // Round window corners expose the browser backdrop. Sample the material
    // inside the lower-left edge and ignore a narrow region round the viewport.
    const width = 560;
    const height = 420;
    const sample = (width * (height - 18) + 18) * 4;
    let pixels = 0;
    let colorDistance = 0;
    let inkWeb = 0;
    let inkNative = 0;
    for (let y = 9; y < height - 9; y++) {
      for (let x = 9; x < width - 9; x++) {
        const index = (y * width + x) * 4;
        const webVisible = distance(a, index, a, sample) > 7;
        const nativeVisible = distance(b, index, b, sample) > 7;
        if (webVisible) inkWeb++;
        if (nativeVisible) inkNative++;
        if (!webVisible && !nativeVisible) continue;
        pixels++;
        colorDistance += distance(a, index, b, index);
      }
    }
    return {
      backgroundDistance: distance(a, sample, b, sample),
      foregroundDistance: pixels ? colorDistance / pixels : 255,
      inkWeb,
      inkNative,
    };
  });
}

let browser;
try {
  await server.listen();
  const base = "http://127.0.0.1:" + server.httpServer.address().port;
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: size, locale: "en-US" });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const results = [];
  for (const theme of Object.keys(presets).filter(name => !selectedTheme || name === selectedTheme)) {
    for (const mode of ["light", "dark"]) {
      for (const [fixture, { states, text }] of Object.entries(cases).filter(([name]) => !selectedFixture || name === selectedFixture)) {
        for (const state of states) {
          const id = [theme, mode, fixture, state].join("_");
          try {
            const target = join(output, id + "_native.png");
            const nativeArgs = ["-theme", theme, "-mode", mode, "-fixture", fixture, "-state", state, "-output", target, "-inspect"];
            const url = new URL("/packages/ui-react/showcase/", base);
            url.search = new URLSearchParams({ theme, mode, fixture, state }).toString();
            errors.length = 0;
            await page.goto(url.href);
            await page.locator("main[data-fixture]").waitFor();
            await page.mouse.move(450, 390);
            const combo = page.getByRole("combobox", { name: "Color mode" });
            if (state === "hover") await combo.hover();
            if (state === "focus") await combo.focus();
            if (state === "popup") await combo.click();
            if (fixture === "disclosure" && state === "expanded") await page.getByRole("button", { name: "Software updates" }).click();
            if (fixture === "help" && state === "expanded") await page.getByRole("button", { name: "Setting help: Software updates" }).click();

            if (fixture === "toast") {
              await page.waitForFunction(() => getComputedStyle(document.querySelector(".toast-item")).opacity === "1");
            }
            const webImage = await page.screenshot({ path: join(output, id + "_web.png"), animations: "disabled" });
            const { stdout } = await execute(resolve(native), nativeArgs, { timeout: 10000 });
            const nativeImage = await readFile(target);
            const nativeText = JSON.parse(stdout).join(" ");
            const webText = await page.locator("body").innerText();
            const comparable = (value, expected) => value.toLowerCase().includes(expected.toLowerCase());
            const marker = fixture === "app-header" && state === "settings" ? "Settings"
              : fixture === "search-history" && state === "empty" ? "No recent searches"
              : fixture === "help" && state === "default" ? "" : text;
            const sameText = marker === "Clipboard history"
              ? ["Clipboard", "history"].every(word => comparable(webText, word) && comparable(nativeText, word))
              : comparable(webText, marker) && comparable(nativeText, marker);
            const extra = state === "popup" ? "Light" : state === "expanded" && fixture === "disclosure" ? "Preview channel" : "";
            const expectedState = comparable(webText, extra) && comparable(nativeText, extra);
            const pixels = await page.evaluate(comparePixels, { web: webImage.toString("base64"), native: nativeImage.toString("base64") });
            const ratio = pixels.inkNative / pixels.inkWeb;
            const passed = !errors.length && sameText && expectedState && Number.isFinite(ratio) &&
              ratio >= 0.4 && ratio <= 2.5 && pixels.backgroundDistance <= 18 && pixels.foregroundDistance <= 36;
            results.push({ id, passed, sameText, expectedState, ...pixels, errors: [...errors] });
          } catch (error) {
            results.push({ id, passed: false, error: error.message });
          }
        }
      }
      await writeFile(reportPath, JSON.stringify(results, null, 2) + "\n");
    }
  }
  const failures = results.filter(result => !result.passed);
  console.log(JSON.stringify({ passed: results.length - failures.length, total: results.length, output, failures: failures.slice(0, 10) }, null, 2));
  if (failures.length) process.exitCode = 1;
} finally {
  if (browser) await browser.close();
  await server.close();
}
