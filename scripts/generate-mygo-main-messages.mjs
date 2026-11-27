import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { en } from "../src/locales/en.ts";
import { zh } from "../src/locales/zh.ts";
import { tw } from "../src/locales/tw.ts";

const desktop = new URL("../frontends/dzc-surface/cmd/dzc-desktop/", import.meta.url);
export function usedNativeMainMessageKeys() {
  const keys = new Set();
  for (const name of readdirSync(desktop)) {
    if (!/^native_main.*\.go$/.test(name) || name.endsWith("_test.go")) continue;
    const source = readFileSync(new URL(name, desktop), "utf8");
    for (const [, key] of source.matchAll(/\.text\("([a-z0-9_.]+)"/g)) keys.add(key);
  }
  const catalog = JSON.parse(readFileSync(new URL("../protocol/settings.catalog.json", import.meta.url), "utf8"));
  for (const setting of Object.values(catalog).flat()) {
    keys.add(setting.labelKey);
    for (const key of Object.values(setting.optionLabelKeys ?? {})) keys.add(key);
  }
  const { themes } = JSON.parse(readFileSync(new URL("../design/themes.json", import.meta.url), "utf8"));
  for (const { labelKey } of themes) keys.add(labelKey);
  return [...keys].sort();
}

function generate(check) {
  const keys = usedNativeMainMessageKeys();
  const messages = {};
  for (const [locale, dictionary] of Object.entries({ en, zh, tw })) {
    messages[locale] = {};
    for (const key of keys) {
      const value = dictionary[key];
      if (typeof value !== "string" || !value.trim()) throw new Error(`Missing ${locale} translation for ${key}`);
      messages[locale][key] = value;
    }
  }
  const output = fileURLToPath(new URL("native-main-messages.json", desktop));
  const expected = JSON.stringify(messages, null, 2) + "\n";
  if (check) {
    if (readFileSync(output, "utf8").replaceAll("\r\n", "\n") !== expected) {
      throw new Error("MyGo main translations are stale. Run node scripts/generate-mygo-main-messages.mjs.");
    }
  } else {
    writeFileSync(output, expected);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  generate(process.argv.includes("--check"));
}
