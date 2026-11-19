import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";
import { readNativeWidgetStyles } from "./read-native-widget-styles.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));
const generated = resolve(root, "packages/ui-presets/builtin-presets.json");
const widgetFile = resolve(root, "packages/ui-presets/builtin-widget-styles.json");
const server = await createServer({ root, configFile: false, server: { host: "127.0.0.1", port: 0, hmr: false, watch: null } });
let browser;
try {
  await server.listen();
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
