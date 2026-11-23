import { once } from "node:events";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";
import { readNativeWidgetStyles } from "./read-native-widget-styles.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));
const generated = resolve(root, "packages/ui-presets/builtin-presets.json");
const widgetFile = resolve(root, "packages/ui-presets/builtin-widget-styles.json");
const server = await createServer({ root, configFile: false, server: { host: "127.0.0.1", hmr: false, watch: null } });
let browser;
try {
  const listening = once(server.httpServer, "listening");
  server.httpServer.listen(0, "127.0.0.1");
  await listening;
  const address = server.httpServer.address();
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 560, height: 420 }, locale: "en-US" });
  const themes = ["retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"];
  const output = {};
  const widgets = {};
  for (const theme of themes) {
    output[theme] = {};
    widgets[theme] = {};
    for (const mode of ["light", "dark"]) {
      const url = new URL("/packages/ui-react/showcase/", "http://127.0.0.1:" + address.port);
      url.search = new URLSearchParams({ theme, mode }).toString();
      await page.goto(url.href);
      const appearance = JSON.parse(await page.locator("body[data-appearance]").getAttribute("data-appearance"));
      if (appearance.Mode !== mode || !/^#[0-9a-f]{6}$/.test(appearance.Tokens.Text)) {
        throw Error("Invalid rendered appearance for " + theme + " " + mode);
      }
      output[theme][mode] = appearance.Tokens;
      if (["mica", "acrylic", "liquid-glass"].includes(theme)) {
        const { rgb, power } = await page.evaluate(() => {
          const style = getComputedStyle(document.documentElement);
          return {
            rgb: style.getPropertyValue("--surface-shell-rgb").trim(),
            power: Number(style.getPropertyValue("--system-backdrop-tint-power").trim()),
          };
        });
        const channels = rgb.split(/\s+/).map(Number);
        if (channels.length !== 3 || channels.some(value => !Number.isInteger(value) || value < 0 || value > 255)) {
          throw Error("Invalid window tint for " + theme + " " + mode);
        }
        output[theme][mode].SurfaceTint = "#" + channels.map(value => value.toString(16).padStart(2, "0")).join("");
        if (power) {
          const matches = await page.evaluate(power => {
            const root = document.documentElement;
            const body = document.body;
            const shell = document.getElementById("root");
            const savedOpacity = root.style.getPropertyValue("--surface-opacity");
            const context = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
            const paintedAlpha = () => {
              context.clearRect(0, 0, 1, 1);
              context.fillStyle = getComputedStyle(shell).backgroundColor;
              context.fillRect(0, 0, 1, 1);
              return context.getImageData(0, 0, 1, 1).data[3] / 255;
            };
            const sameAlpha = (actual, expected) => Math.abs(actual - expected) <= 1 / 255;
            root.classList.add("windows-11");
            try {
              return [0, 0.2, 0.65, 0.85, 1].every(opacity => {
                body.classList.remove("has-custom-bg");
                root.style.setProperty("--surface-opacity", String(opacity));
                const system = paintedAlpha();
                body.classList.add("has-custom-bg");
                return sameAlpha(system, opacity ** power) && sameAlpha(paintedAlpha(), opacity);
              });
            } finally {
              root.classList.remove("windows-11");
              body.classList.remove("has-custom-bg");
              root.style.setProperty("--surface-opacity", savedOpacity);
            }
          }, power);
          if (!matches) throw Error("Invalid Windows backdrop tint for " + theme + " " + mode);
          output[theme][mode].SystemBackdropTintPower = power;
        }
      }
      widgets[theme][mode] = await readNativeWidgetStyles(page, url, theme, mode);
    }
  }
  for (const [path, values] of [[generated, output], [widgetFile, widgets]]) {
    const data = JSON.stringify(values, null, 2)
      .replace(/\[\s+(-?\d[\d.,\s-]*)\]/g, (_, numbers) => "[" + numbers.trim().replace(/\s+/g, " ") + "]") + "\n";
    if (process.argv.includes("--check")) {
      if (await readFile(path, "utf8") !== data) throw Error("Native theme data differs from Web CSS: " + path);
    } else {
      await writeFile(path, data, "utf8");
    }
  }
  console.log("Verified 12 CSS-derived theme palettes and component styles");
} finally {
  if (browser) await browser.close();
  await server.close();
}
