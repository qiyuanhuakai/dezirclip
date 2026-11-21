import { once } from "node:events";
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
const selectedFixtures = flags.get("fixture")?.split(",");
const presets = JSON.parse(await readFile(resolve(root, "packages/ui-presets/builtin-presets.json"), "utf8"));
const cases = {
  "surface-opacity": { states: ["0", "20", "65", "85", "100"], text: "Appearance" },
  "quick-paste-panel": { states: ["0", "65", "100"], text: "Release checklist" },
  "settings-field": { states: ["default", "expanded"], text: "Interface font" },
  "settings-section-card": { states: ["default", "expanded", "wide"], text: "General settings" },
  "settings-choices": { states: ["default"], text: "Update channel" },
  "hotkey-list": { states: ["default", "multiple", "win-v", "extended"], text: "Global hotkey" },
  "emoji-favorites": { states: ["default"], text: "Add emoji" },
  "settings-data": { states: ["default", "hover-path"], text: "Data storage path" },
  "backup-dialog": { states: ["default", "encrypted", "error", "import", "replace", "replace-confirmed"], text: "File path" },
  "settings-update": { states: ["default", "unknown-size", "installing"], text: "Installed version" },
  "settings-update-actions": { states: ["default", "busy", "hover"], text: "Check now" },
  "settings-remove": { states: ["default"], text: "Remove theme" },
  "privacy-rules": { states: ["default", "resized"], text: "Privacy rules" },
  "privacy-detection": { states: ["default", "narrow", "focus", "resized"], text: "Custom detection patterns" },
  "tag-toolbar": { states: ["default"], text: "Work" },
  "tag-batch-toolbar": { states: ["default", "selected", "narrow"], text: "selected" },
  "tag-search": { states: ["default"], text: "Work" },
  "search-select": { states: ["default", "apps-filtered", "empty", "typo"], text: "Interface font" },
  "text-fields": { states: ["default"], text: "Idle timeout" },
  "context-menu": { states: ["default", "text", "privacy"], text: "Clipboard entry" },
  "attachment-preview": { states: ["default", "compact", "media", "expanded", "narrow", "narrow-media", "compact-media"], text: "" },
  "history-image": { states: ["default", "compact", "pending", "compact-pending", "missing", "compact-missing", "unavailable", "compact-unavailable"], text: "" },
  "history-ocr": { states: ["default", "expanded", "compact-expanded", "processing", "failed", "unsupported"], text: "" },
  "preview-toolbar": { states: ["default", "pinned"], text: "Content preview" },
  "preview-image": { states: ["default", "zoomed"], text: "Image preview" },
  "preview-resize-handles": { states: ["default", "southeast"], text: "Resize preview" },
  "wheel-hot-zone": { states: ["default", "scrolled"], text: "Wheel input" },
  "history-card-meta": { states: ["default"], text: "Editor" },
  "history-list": { states: ["default", "selected"], text: "Release checklist and verification notes" },
  "history-typography": { states: ["default", "small", "large", "compact", "compact-large", "custom"], text: "Font sample: Wm12" },
  "history-rich-text": { states: ["default", "compact"], text: "Release checklist" },
  "settings-rail": { states: ["default", "selected"], text: "Appearance" },
  "settings-footer": { states: ["default", "compact"], text: "Reset defaults" },
  "history-card": { states: ["default", "selected", "pinned", "compact", "compact-hover", "compact-action-hover", "compact-active-hover"], text: "Copied from the active editor" },
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
const additionalExamples = {
  "character-search": { states: ["default", "filled"], text: "Search symbols" },
  "character-panel": { states: ["default", "after-scroll"], text: "Symbols" },
  "clipboard-search": { states: ["default", "selected"], text: "富文本" },
  "settings-toggle": { states: ["enabled", "default"], text: "Keep clipboard history" },
  "settings-toggle-help": { states: ["default", "expanded"], text: "Move pasted entries to top" },
  "settings-keyboard-preview": { states: ["default", "enabled"], text: "Preview keyboard selection" },
  "settings-slider": { states: ["default", "keyboard"], text: "Opacity 72%" },
  "hotkey-binding": { states: ["default", "recording"], text: "Quick Paste" },
  "hotkey-optional": { states: ["default", "options"], text: "Privacy session shortcut" },
  "scroll-to-top": { states: ["default", "hover"], text: "Clipboard history" },
  "emoji-tabs": { states: ["default", "favorites"], text: "Favorites" },
  "emoji-grid": { states: ["default"], text: "Frequently used" },
};
const available = { ...cases, ...additionalExamples };
if (selectedTheme && !(selectedTheme in presets)) throw Error("Unknown theme " + selectedTheme);
for (const fixture of selectedFixtures ?? []) {
  if (!(fixture in available)) throw Error("Unknown component " + fixture);
}
const fixtures = selectedFixtures
  ? selectedFixtures.map(name => [name, available[name]])
  : Object.entries(flags.get("examples") === "all" ? available : cases);
const server = await createServer({ root, configFile: false, server: { host: "127.0.0.1", hmr: false, watch: null } });
const output = await mkdtemp(join(tmpdir(), "deziro-ui-parity-"));
const size = { width: 560, height: 420 };
const reportPath = join(output, "results.json");

function comparePixels({ web, native, dialog }) {
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
    const backdrop = (width * (height - 18) + 18) * 4;
    // In translucent dialogs, compare controls against the panel material.
    // The outer shadow can otherwise dominate the count of visible controls.
    const area = dialog ? {
      left: Math.ceil(dialog.x + 7), top: Math.ceil(dialog.y + 7),
      right: Math.floor(dialog.x + dialog.width - 7), bottom: Math.floor(dialog.y + dialog.height - 7),
    } : { left: 9, top: 9, right: width - 9, bottom: height - 9 };
    const sample = dialog ? (width * (area.top + 5) + area.right - 5) * 4 : backdrop;
    let pixels = 0;
    let colorDistance = 0;
    let inkWeb = 0;
    let inkNative = 0;
    for (let y = area.top; y < area.bottom; y++) {
      for (let x = area.left; x < area.right; x++) {
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
      backgroundDistance: Math.max(distance(a, sample, b, sample), distance(a, backdrop, b, backdrop)),
      foregroundDistance: pixels ? colorDistance / pixels : 255,
      inkWeb,
      inkNative,
    };
  });
}

let browser;
try {
  const listening = once(server.httpServer, "listening");
  server.httpServer.listen(0, "127.0.0.1");
  await listening;
  const base = "http://127.0.0.1:" + server.httpServer.address().port;
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: size, locale: "en-US" });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const results = [];
  for (const theme of Object.keys(presets).filter(name => !selectedTheme || name === selectedTheme)) {
    for (const mode of ["light", "dark"]) {
      for (const [fixture, { states, text }] of fixtures) {
        for (const state of states) {
          const id = [theme, mode, fixture, state].join("_");
          try {
            const target = join(output, id + "_native.png");
            const nativeArgs = ["-theme", theme, "-mode", mode, "-fixture", fixture, "-state", state, "-output", target, "-inspect"];
            const url = new URL("/packages/ui-react/showcase/", base);
            url.search = new URLSearchParams({ theme, mode, fixture, state }).toString();
            errors.length = 0;
            await page.mouse.move(450, 390);
            await page.goto(url.href);
            await page.locator("main[data-fixture]").waitFor();
            if (fixture === "hotkey-optional") {
              if (await page.getByRole("button", { name: "Clear shortcut" }).count()) {
                throw Error("The optional shortcut exposes a clear action before opening help");
              }
              if (state === "options") {
                await page.getByRole("button", { name: "Setting help: Privacy session shortcut" }).click();
                await page.mouse.move(450, 390);
              }
            }
            if (fixture === "settings-toggle-help" && state === "expanded") {
              await page.getByRole("button", { name: "Setting help: Move pasted entries to top" }).click();
              await page.getByText("After pasting, move the item to the top of the list.").waitFor();
              await page.mouse.move(450, 390);
              if (!await page.getByRole("checkbox", { name: "Move pasted entries to top" }).isChecked()) {
                throw Error("Opening the description changed the preference");
              }
            }
            if (fixture === "character-panel") {
              const viewport = page.locator(".emoji-content");
              const nextGroup = viewport.getByText("Geometric shapes", { exact: true });
              const lastGroup = viewport.getByText("Mathematics", { exact: true });
              if (await nextGroup.count() || await lastGroup.count()) throw Error("Later batches appeared before scrolling");
              if (state === "after-scroll") {
                await viewport.hover();
                await page.mouse.wheel(0, 2000);
                await nextGroup.waitFor({ state: "attached" });
                await page.mouse.wheel(0, 2000);
                await lastGroup.waitFor({ state: "attached" });
                await page.mouse.wheel(0, 2000);
                await page.waitForFunction(() => {
                  const grid = document.querySelector(".emoji-content");
                  const heading = [...grid.querySelectorAll(".emoji-group-title")].find(element => element.textContent === "Mathematics");
                  const bounds = grid.getBoundingClientRect();
                  const title = heading.getBoundingClientRect();
                  return title.top >= bounds.top && title.bottom <= bounds.bottom;
                });
              }
              if (await viewport.getByRole("button", { name: "Show more", exact: true }).count()) throw Error("The list requires a paging button");
              const usesThemeColor = await viewport.evaluate(grid =>
                getComputedStyle(grid.querySelector(".emoji-btn")).color === getComputedStyle(grid.querySelector(".emoji-group-title")).color);
              if (!usesThemeColor) throw Error("Symbols do not use the theme text color");
            }
            if (fixture === "settings-field" && state === "expanded") {
              await page.getByRole("button", { name: "Setting help: Interface font" }).click();
              await page.getByText("Use the theme font by default.", { exact: true }).waitFor();
            }
            if (fixture === "settings-data" && state === "hover-path") await page.locator(".data-panel").hover();
            if (fixture === "backup-dialog" && ["macos", "liquid-glass"].includes(theme)) {
              const styled = await page.locator(".data-modal-content").evaluate(dialog => {
                const panel = getComputedStyle(dialog);
                const title = getComputedStyle(dialog.querySelector(".data-modal-title"));
                const submit = getComputedStyle(dialog.querySelector(".data-btn-primary"));
                const chosen = getComputedStyle(dialog.querySelector(".data-radio-item:has(input:checked)"));
                const other = getComputedStyle(dialog.querySelector(".data-radio-item:has(input:not(:checked))"));
                const confirmation = dialog.querySelector(".data-replace-confirm");
                return panel.borderTopWidth === "1px" && parseFloat(panel.borderTopLeftRadius) >= 14
                  && title.backgroundColor === "rgba(0, 0, 0, 0)" && title.textTransform === "none"
                  && submit.textTransform === "none" && parseFloat(submit.borderTopLeftRadius) >= 8
                  && chosen.borderTopColor !== other.borderTopColor
                  && (!confirmation || parseFloat(getComputedStyle(confirmation).borderTopLeftRadius) >= 8);
              });
              if (!styled) throw Error("Backup dialog lost its themed panel, typography or control states");
            }
            if (fixture === "backup-dialog" && state.startsWith("replace")) {
              const dialog = page.getByRole("dialog", { name: "Import clipboard history" });
              const submit = dialog.getByRole("button", { name: "Import clipboard history" });
              if (await submit.isEnabled()) throw Error("Replacement was enabled without confirmation");
              if (state === "replace-confirmed") {
                await dialog.getByRole("checkbox", { name: "Confirm replacing clipboard history" }).check();
                if (!await submit.isEnabled()) throw Error("The confirmed replacement stayed disabled");
                await page.mouse.move(4, 4);
              }
            }
            if (fixture === "history-ocr" && state.endsWith("expanded")) {
              await page.getByTestId("ocr-toggle").click();
              await page.getByTestId("ocr-text").waitFor();
              await page.mouse.move(450, 390);
            }
            if (fixture === "surface-opacity") {
              await page.getByRole("combobox", { name: "Color mode" }).click();
              await page.getByRole("option", { name: "Light", exact: true }).waitFor();
            }
            if (fixture === "clipboard-search") {
              const search = page.locator("[data-clipboard-search]");
              const field = search.getByRole("textbox", { name: "搜索剪切板..." });
              const filters = await search.getByRole("button").allTextContents();
              if (await field.count() !== 1 || filters.join(",") !== "文本,图片,文件,URL,代码,视频,富文本" ||
                  await page.getByRole("radiogroup").count() !== 0) {
                throw Error("Clipboard search must show one field and seven ordered content filters");
              }
              if (theme === "macos" && await field.evaluate(element => getComputedStyle(element).borderTopLeftRadius) !== "10px") {
                throw Error("The macOS clipboard search must retain its rounded corners");
              }
              if (state === "selected") {
                const image = search.getByRole("button", { name: "图片" });
                await image.click();
                if (await image.getAttribute("aria-pressed") !== "true") throw Error("Image filter did not become selected");
              }
            }
            await page.mouse.move(450, 390);
            if (fixture === "history-card" && state.startsWith("compact")) {
              const pin = page.getByRole("button", { name: "Pin", exact: true });
              await page.waitForFunction(() => getComputedStyle(document.querySelector(".item-actions")).visibility === "hidden");
              if (state.endsWith("hover")) {
                await page.locator(".history-item").hover();
                if (state !== "compact-hover") {
                  await pin.waitFor();
                  const bounds = await pin.boundingBox();
                  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
                }
                await page.waitForFunction(() => getComputedStyle(document.querySelector(".item-actions")).opacity === "1");
              }
            }
            if (fixture === "context-menu" && state !== "default") {
              const del = page.getByRole("menuitem", { name: "Delete" });
              const before = await del.boundingBox();
              await page.getByRole("menuitem", { name: state === "text" ? "Text processing" : "Privacy" }).hover();
              if (JSON.stringify(before) !== JSON.stringify(await del.boundingBox())) {
                throw Error("Opening a submenu moved the following root item");
              }
            }
            if (fixture === "text-fields") {
              for (const [label, value] of [["Entry name", "Release checklist"], ["Idle timeout", "30"], ["Notes", "First line\nSecond line"]]) {
                const field = page.getByRole("textbox", { name: label });
                if (await field.inputValue() !== value || await field.evaluate(element => parseFloat(getComputedStyle(element).paddingLeft)) < 8) {
                  throw Error("Invalid value or text inset for " + label);
                }
              }
            }
            if (fixture === "hotkey-list") {
              const buttons = await page.getByRole("button", { name: /^Delete / }).count();
              const expected = state === "extended" ? 3 : state === "multiple" ? 2 : state === "win-v" ? 1 : 0;
              if (buttons !== expected) throw Error("Incorrect shortcut removal controls");
              const keycaps = state === "extended" ? ["1", "Numpad1", "Semicolon"] : [state === "win-v" ? "Win" : "Super"];
              for (const keycap of keycaps) {
                if (await page.locator(".hotkey-list").getByText(keycap, { exact: true }).count() !== 1) {
                  throw Error("Missing shortcut keycap: " + keycap);
                }
              }
              if (state === "win-v" && await page.getByRole("button", { name: "Delete Win+V", exact: true }).count()) {
                throw Error("The system-controlled Win+V binding can be removed");
              }
            }
            if (fixture === "settings-slider" && state === "keyboard") {
              const range = page.getByRole("slider", { name: "Opacity" });
              await range.press("ArrowRight");
              if (await range.inputValue() !== "73") throw Error("Slider keyboard step did not change opacity to 73");
            }
            const combo = page.getByRole("combobox", { name: "Color mode" });
            if (state === "hover") {
              if (fixture === "scroll-to-top") await page.getByRole("button", { name: "Scroll to top" }).hover();
              else if (fixture === "settings-update-actions") await page.getByRole("button", { name: "Check now" }).hover();
              else await combo.hover();
            }
            if (fixture === "select" && state === "focus") await combo.focus();
            if (state === "popup") await combo.click();
            if (fixture === "disclosure" && state === "expanded") await page.getByRole("button", { name: "Software updates" }).click();
            if (fixture === "help" && state === "expanded") await page.getByRole("button", { name: "Setting help: Software updates" }).click();
            if (fixture === "preview-image") {
              await page.locator(".preview-image-viewport[data-ready=true]").waitFor();
            }
            if (state === "southeast" || state === "scrolled") {
              const target = await page.locator("[data-input-target]").boundingBox();
              if (!target) throw Error("No input region");
              if (state === "southeast") {
                await page.mouse.click(target.x + target.width - 4, target.y + target.height - 4);
              } else {
                await page.mouse.move(target.x + 20, target.y + 20);
                await page.mouse.wheel(0, 52);
                await page.getByText("1 scroll", { exact: true }).waitFor();
              }
            }

            if (fixture === "privacy-detection" && state === "focus") {
              await page.getByRole("textbox", { name: "Patterns" }).focus();
            }
            if (state === "resized" && (fixture === "privacy-detection" || fixture === "privacy-rules")) {
              const label = fixture === "privacy-detection" ? "Patterns" : "Excluded applications";
              const textarea = page.getByRole("textbox", { name: label });
              const before = await textarea.boundingBox();
              if (!before) throw Error("Privacy textarea is not visible");
              const x = before.x + before.width - 5;
              const y = before.y + before.height - 5;
              const delta = 40;
              await page.mouse.move(x, y);
              await page.mouse.down();
              await page.mouse.move(x - 24, y + delta, { steps: 4 });
              await page.mouse.up();
              await page.mouse.move(450, 390);
              const after = await textarea.boundingBox();
              if (!after || Math.abs(after.width - before.width) > 1 ||
                  Math.abs(after.height - before.height - delta) > 1) {
                throw Error(`Privacy textarea ${label} did not resize vertically: ${JSON.stringify({ before, after })}`);
              }
            }
            if (fixture === "toast") {
              await page.waitForFunction(() => getComputedStyle(document.querySelector(".toast-item")).opacity === "1");
            }
            if (fixture === "search-select" && state !== "default") {
              const input = page.getByRole("combobox", { name: "Images" });
              await input.click();
              await input.fill(({ empty: "zzzz0000", typo: "Photo Veiwer" })[state] ?? "viewer");
              if (state === "empty") {
                await page.getByText("No matching applications", { exact: true }).waitFor();
              } else {
                await page.getByRole("option", { name: "Photo Viewer", exact: true }).waitFor();
                if (await page.getByRole("option", { name: "Archive Manager" }).count()) {
                  throw Error("Unmatched application was still offered");
                }
              }
            }
            const webImage = await page.screenshot({ path: join(output, id + "_web.png"), animations: "disabled" });
            const { stdout } = await execute(resolve(native), nativeArgs, { timeout: 10000 });
            const nativeImage = await readFile(target);
            const inspection = JSON.parse(stdout);
            const nativeText = (Array.isArray(inspection) ? inspection : inspection.Texts).join(" ");
            let webText = await page.locator("body").innerText();
            if (fixture === "privacy-rules") {
              const confirmation = page.getByRole("group", { name: "Remove privacy rule", exact: true });
              const excluded = page.getByRole("textbox", { name: "Excluded applications", exact: true });
              if (await confirmation.getByRole("button", { name: "Remove rule", exact: true }).count() !== 1
                || await page.getByRole("button", { name: "Revoke exception", exact: true }).count() !== 1
                || await excluded.inputValue() !== "Example Editor\nExample Browser") {
                throw Error("Privacy confirmation or source exclusions are missing");
              }
            }
            if (fixture === "privacy-detection") {
              const webRows = await page.locator(".privacy-kind-option").evaluateAll(options =>
                options.map(option => ({ label: option.textContent.trim(), y: Math.round(option.getBoundingClientRect().y) })));
              const nativeRows = webRows.map(({ label }) => inspection.Options[label]?.Y);
              const rows = offsets => offsets.map(y => Math.round(y - offsets[0]));
              const relative = rows(webRows.map(({ y }) => y));
              if (webRows.length !== 5 || nativeRows.some(y => typeof y !== "number") ||
                  rows(nativeRows).some((y, i) => Math.abs(y - relative[i]) > 3)) {
                throw Error(`Privacy switches wrap differently: WebView ${relative}, Native ${rows(nativeRows)}`);
              }
              const patterns = page.getByRole("textbox", { name: "Patterns" });
              if (await patterns.inputValue() !== "EXAMPLE-[0-9]+\nproject_[a-z]+") {
                throw Error("Custom patterns did not load the multiline text");
              }
              const bounds = await patterns.boundingBox();
              const counterpart = inspection.Options.Patterns;
              if (!bounds || !counterpart || Math.abs(bounds.width - counterpart.W) > 2 ||
                  Math.abs(bounds.height - counterpart.H) > 2 || Math.abs(bounds.x - counterpart.X) > 2) {
                throw Error(`Privacy patterns field WebView ${bounds?.width}×${bounds?.height}, Native ${counterpart?.W}×${counterpart?.H}`);
              }
              const identity = page.getByRole("checkbox", { name: "Identity numbers" });
              if (await identity.isChecked()) throw Error("Identity detection starts enabled");
              await page.getByText("Identity numbers", { exact: true }).click();
              if (!await identity.isChecked()) throw Error("The detection label did not toggle its switch");
            }
            if (fixture === "tag-search") {
              const field = page.getByRole("textbox", { name: "Search tagged entries", exact: true });
              if (await field.inputValue() !== "Release notes"
                || await page.getByRole("button", { name: "Clear search" }).count() !== 1
                || await page.locator(".tag-manager .search-icon").count() !== 3) {
                throw Error("Incorrect tag search fields or clear button");
              }
            }
            if (fixture === "character-search") {
              const field = page.getByRole("searchbox", { name: "Search symbols" });
              if (await field.inputValue() !== (state === "filled" ? "Clipboard" : "")) throw Error("Wrong character search query");
              webText += await field.getAttribute("aria-label");
            }
            const comparable = (value, expected) => value.toLowerCase().includes(expected.toLowerCase());
            const marker = fixture === "app-header" && state === "settings" ? "Settings"
              : fixture === "settings-slider" && state === "keyboard" ? "Opacity 73%"
              : fixture === "search-history" && state === "empty" ? "No recent searches"
              : fixture === "help" && state === "default" ? "" : text;
            const sameText = marker === "Clipboard history"
              ? ["Clipboard", "history"].every(word => comparable(webText, word) && comparable(nativeText, word))
              : comparable(webText, marker) && comparable(nativeText, marker);
            const extra = state === "popup" || fixture === "surface-opacity" ? "Light"
              : fixture === "hotkey-optional" && state === "options" ? "Clear shortcut"
              : fixture === "settings-toggle-help" && state === "expanded" ? "After pasting, move the item to the top of the list."
              : fixture === "character-panel" ? state === "after-scroll" ? "Mathematics" : "Arrows"
              : fixture === "settings-field" && state === "expanded" ? "Use the theme font by default."
              : state === "expanded" && fixture === "disclosure" ? "Preview channel"
              : fixture === "context-menu" && state !== "default" ? state === "text" ? "Uppercase" : "Ignore this source"
              : fixture === "settings-update" ? {
                default: "42%", "unknown-size": "2.5 MiB", installing: "Official update signature verified",
              }[state]
              : fixture === "settings-update-actions" ? state === "busy" ? "42%" : "Install update"
              : fixture === "backup-dialog" ? {
                default: "Export format", encrypted: "Confirm passphrase", error: "Passphrase must be at least 12 characters",
                import: "Release checklist", replace: "Confirm replacing clipboard history", "replace-confirmed": "Confirm replacing clipboard history",
              }[state]
              : state === "southeast" ? "SouthEast" : state === "scrolled" ? "1 scroll" : "";
            const expectedState = comparable(webText, extra) && comparable(nativeText, extra);
            const dialog = fixture === "backup-dialog" ? await page.locator(".data-modal-content").boundingBox() : undefined;
            const pixels = await page.evaluate(comparePixels, { web: webImage.toString("base64"), native: nativeImage.toString("base64"), dialog });
            if (fixture === "quick-paste-panel" && pixels.backgroundDistance > 2) {
              throw Error(`Quick Paste backdrop differs at ${state}%: ${pixels.backgroundDistance}`);
            }
            if (fixture === "tag-batch-toolbar") {
              const selection = page.getByRole("button", { name: state === "default" ? "Select all" : state === "narrow"
                ? "Clear all matching entries" : "Clear selection" });
              const deletion = page.getByRole("button", { name: "Delete", exact: true });
              if ((await deletion.isDisabled()) !== (state === "default")) throw Error("Delete availability ignored the current selection");
              if (state === "narrow") {
                const button = await deletion.boundingBox();
                const toolbar = await page.locator(".tag-manager-batch-toolbar").boundingBox();
                if (!button || !toolbar || button.x + button.width > toolbar.x + toolbar.width) throw Error("Delete is clipped");
              }
              await selection.press("Enter");
              await page.getByText(state === "default" ? "32 selected" : "0 selected", { exact: true }).waitFor();
              if ((await deletion.isDisabled()) === (state === "default")) throw Error("Delete did not update after changing the selection");
            }
            if (fixture === "settings-update-actions") {
              const check = page.getByRole("button", { name: "Check now" });
              const install = page.getByRole("button", { name: "Install update" });
              const releases = page.getByRole("button", { name: "Open release page" });
              if ((await check.isDisabled()) !== (state === "busy") ||
                  (await install.isDisabled()) !== (state === "busy") || !await releases.isEnabled()) {
                throw Error("Update action availability differs from the update state");
              }
              if (await page.locator(".update-actions button svg").count() !== 3) throw Error("Update action icons are missing");
              if (state === "busy") {
                await releases.click();
                await page.getByText("Release opened", { exact: true }).waitFor();
              } else {
                await install.click();
                await page.getByText("Install chosen", { exact: true }).waitFor();
              }
            }
            if (fixture === "hotkey-optional" && state === "options") {
              const clear = page.getByRole("button", { name: "Clear shortcut" });
              await clear.click();
              await page.getByText("Not set", { exact: true }).waitFor();
              if (await clear.count()) throw Error("Clearing the binding left the action available");
            }
            if (fixture === "tag-dialog") {
              const dialog = page.getByRole("dialog", { name: "Create tag" });
              const input = dialog.getByRole("textbox", { name: "Name" });
              const close = dialog.getByRole("button", { name: "Close" });
              if (!await input.evaluate(element => element.classList.contains("search-input"))) throw Error("Tag name did not use the themed field");
              if (state === "busy") {
                if (await close.isEnabled()) throw Error("Busy dialog close was enabled");
                await page.keyboard.press("Escape");
                if (!await dialog.isVisible()) throw Error("Busy dialog closed on Escape");
              } else {
                await close.click();
                await dialog.waitFor({ state: "hidden" });
              }
            }
            if (fixture === "backup-dialog" && state === "import") {
              const dialog = page.getByRole("dialog", { name: "Import clipboard history" });
              const submit = dialog.getByRole("button", { name: "Import clipboard history" });
              try {
                await page.setViewportSize({ width: 250, height: 300 });
                await dialog.getByRole("radio", { name: "Replace existing entries" }).check();
                if (await submit.isEnabled()) throw Error("Replacement was enabled without confirmation");
                await dialog.getByRole("checkbox", { name: "Confirm replacing clipboard history" }).check();
                const bounds = await submit.boundingBox();
                if (!await submit.isEnabled() || !bounds || bounds.y < 0 || bounds.y + bounds.height > 300) {
                  throw Error("Replacement action was inaccessible at the minimum window size");
                }
                await page.keyboard.press("Escape");
                await dialog.waitFor({ state: "hidden" });
              } finally {
                await page.setViewportSize(size);
              }
            }
            if (fixture === "settings-update") {
              const bar = page.getByRole("progressbar", { name: "Update download progress" });
              const current = await bar.getAttribute("aria-valuenow");
              if (current !== ({ default: "42", installing: "100", "unknown-size": null })[state]) {
                throw Error("Update progress range disagrees with the download state");
              }
            }
            if (fixture === "history-typography") {
              const measurements = await page.locator("[data-card-metrics]").evaluateAll(cards => cards.map(card => {
                const bounds = card.getBoundingClientRect();
                const content = getComputedStyle(card.querySelector(".content-preview"));
                return { name: card.dataset.cardMetrics, width: bounds.width, height: bounds.height, font: content.fontFamily };
              }));
              for (const recorded of measurements) {
                const counterpart = inspection.Cards[recorded.name];
                if (!counterpart || Math.abs(counterpart.H - recorded.height) > 2 ||
                    Math.abs(counterpart.W - recorded.width) > 2) {
                  throw Error(`Card ${recorded.name} WebView ${recorded.width}×${recorded.height}, MyGo ${counterpart?.W}×${counterpart?.H}`);
                }
              }
              if (state === "custom") {
                const family = ["mica", "acrylic"].includes(theme) ? "Georgia" : "Courier New";
                if (!measurements[0].font.includes(family)) throw Error("Unexpected theme font source: " + measurements[0].font);
              }
            }
            if (fixture === "history-rich-text") {
              const content = page.locator(".history-rich-preview");
              const cap = state === "compact" ? 40 : 64;
              if (await content.evaluate(element => element.clientHeight) !== cap) throw Error("Rich text preview does not use the card height limit");
              const webCard = await page.locator("[data-test-clipboard-item]").boundingBox();
              const nativeCard = inspection.Cards["Formatted clipboard card"];
              if (!webCard || !nativeCard || Math.abs(webCard.height - nativeCard.H) > 2) {
                throw Error(`Rich text card height differs: WebView ${webCard?.height}, Native ${nativeCard?.H}`);
              }
              if (!comparable(nativeText, "Review notes") || !comparable(nativeText, "open the draft.")) {
                throw Error("Native rich text is missing the second paragraph");
              }
            }
            if (fixture === "attachment-preview") {
              const gallery = page.locator(".attachment-gallery");
              const names = await gallery.locator("figcaption > span, .attachment-name > span, img[alt]").evaluateAll(elements =>
                [...new Set(elements.map(element => element.getAttribute("alt") ?? element.textContent))]);
              const expected = ["Project.png", "Screenshot.png", "Notes.pdf", "Planning.xlsx", "Desktop.zip", "Summary.txt", "Log.txt"];
              const nativeNames = expected.filter(name => (Array.isArray(inspection) ? inspection : inspection.Texts).includes(name));
              if (expected.filter(name => names.includes(name)).join(",") !== nativeNames.join(",")
                  || Boolean(await gallery.locator(".attachment-ellipsis").count()) !== comparable(nativeText, "…")) {
                throw Error(`Attachment names differ for ${state}: WebView ${names.join(", ")}, Native ${nativeNames.join(", ")}`);
              }
              const webCard = await page.locator("[data-attachment-card]").boundingBox();
              const nativeCard = inspection.Cards["Attachment card"];
              if (!webCard || Math.abs(nativeCard.W - webCard.width) > 2 || Math.abs(nativeCard.H - webCard.height) > 2) {
                throw Error(`Attachment card ${state}: WebView ${webCard?.width}×${webCard?.height}, Native ${nativeCard.W}×${nativeCard.H}`);
              }
            }
            if (fixture === "history-ocr") {
              const expanded = state.endsWith("expanded");
              const stateLabel = { processing: "Recognizing text…", failed: "OCR failed", unsupported: "OCR unavailable" }[state] ?? "OCR text";
              if (!comparable(nativeText, stateLabel) || Boolean(await page.getByTestId("ocr-text").count()) !== expanded
                  || comparable(nativeText, "Total: 86.40") !== expanded) {
                throw Error("OCR status or disclosure content differs");
              }
              if (stateLabel !== "OCR text" && await page.getByTestId("ocr-toggle").count()) {
                throw Error("A recognition result without text can be expanded");
              }
            }
            if (fixture === "history-image") {
              const measurements = await page.locator("[data-media-measure]").evaluateAll(cards => cards.map(card => ({
                name: card.dataset.mediaMeasure, width: card.getBoundingClientRect().width, height: card.getBoundingClientRect().height,
              })));
              for (const image of measurements) {
                const counterpart = inspection.Cards[image.name];
                if (!counterpart || Math.abs(counterpart.W - image.width) > 2 || Math.abs(counterpart.H - image.height) > 2) {
                  throw Error(`Image ${image.name}: WebView ${image.width}×${image.height}, Native ${counterpart?.W}×${counterpart?.H}`);
                }
              }
            }
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
