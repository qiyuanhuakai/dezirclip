import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { chromium } from "playwright";
import { preview } from "vite";

const server = await preview({
  configFile: false,
  build: { outDir: process.argv[2] ?? "dist/web" },
  preview: { host: "127.0.0.1", port: 0 },
});
let browser;
const errors = [];
try {
  const channel = existsSync("/usr/bin/google-chrome") ? "chrome" : undefined;
  browser = await chromium.launch({ channel });
  const page = await browser.newPage();
  page.on("pageerror", (error) => errors.push(error.stack ?? error.message));
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main", windowLabel: "main" },
      },
      transformCallback: () => 1,
      unregisterCallback() {},
      invoke: async (command) => {
        if (command === "get_settings") {
          return { "app.language": "en", "app.update_auto_check": "false", "app.emoji_panel_enabled": "true" };
        }
        if (command === "get_clipboard_host_status") {
          return {
            state: { service_instance_id: "web-smoke", history_revision: 0, privacy_epoch: 0 },
            privacy: { privacy_epoch: 0, active: false, expires_at_ms: null },
          };
        }
        return null;
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  });

  await page.goto("http://127.0.0.1:" + server.httpServer.address().port);
  const settings = page.getByRole("button", { name: "Settings", exact: true });
  await settings.waitFor({ timeout: 5000 });
  await settings.click();
  await page.getByText("Settings", { exact: true }).waitFor({ timeout: 5000 });
  await page.locator(".settings-view .settings-layout").waitFor({ state: "attached", timeout: 5000 });
  await page.locator("header").getByRole("button", { name: "Close", exact: true }).click();
  const [catalog] = await Promise.all([
    page.waitForResponse(response => /\/assets\/emoji-[^/]+\.json$/.test(response.url()), { timeout: 5000 }),
    page.getByTitle("Stickers", { exact: true }).click({ timeout: 5000 }),
  ]);
  assert.ok(catalog.ok(), "the packaged character catalog must load");
  const emoji = page.locator(".emoji-panel");
  await emoji.getByRole("button", { name: "Symbols", exact: true }).click();
  await emoji.getByRole("searchbox").fill("slnted equal");
  await emoji.getByRole("button", { name: "⩽", exact: true }).first().waitFor({ timeout: 5000 });
  assert.deepEqual(errors, []);
  console.log("The production WebView opens Settings and searches the packaged character catalog.");
} catch (error) {
  if (errors.length) console.error(errors.join("\n"));
  throw error;
} finally {
  await browser?.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
