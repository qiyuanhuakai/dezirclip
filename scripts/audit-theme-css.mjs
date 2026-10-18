import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

// Vite already supplies PostCSS for the production CSS build.
const require = createRequire(import.meta.url);
const postcss = createRequire(require.resolve("vite"))("postcss");
const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const styleRoot = join(root, "src", "styles");

function files(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? files(path) : path.endsWith(".css") ? [path] : [];
  });
}

const definitions = new Set();
const references = new Map();
const results = [];
const palettes = new Map();
const componentPolicyViolations = [];
const cleanComponents = new Set(["src/styles/components/tags.css", "src/styles/components/theme-manager.css"]);
const paletteKeys = [
  "surface-window", "surface-panel", "surface-raised", "surface-input", "surface-hover", "surface-active",
  "text-primary", "text-secondary", "text-muted", "text-on-accent", "border", "border-strong",
  "accent", "accent-hover", "accent-soft", "success", "warning", "danger", "info", "radius",
].map((name) => `--dc-${name}`);
for (const path of files(styleRoot)) {
  const name = relative(root, path).replaceAll("\\", "/");
  const sheet = postcss.parse(readFileSync(path, "utf8"), { from: path });
  const result = { file: name, rules: 0, declarations: 0, important: 0,
    unscopedImportant: 0, backdropDeclarations: 0, activeBackdropRules: 0,
    legacyTagRules: 0, longSelectors: 0, idSelectors: 0 };
  sheet.walkRules((rule) => {
    result.rules += 1;
    const selectors = rule.selectors;
    const legacyTag = selectors.some((selector) => selector.includes(".themed-tag-manager"));
    if (legacyTag) result.legacyTagRules += 1;
    result.longSelectors += selectors.filter((selector) => selector.trim().split(/\s+(?![^()]*\))/).length >= 5).length;
    result.idSelectors += selectors.filter((selector) => /#[a-z_][\w-]*/i.test(selector)).length;
    if (rule.nodes.some((node) => node.type === "decl" && /^(?:-webkit-)?backdrop-filter$/.test(node.prop)
      && node.value.trim() !== "none")) result.activeBackdropRules += 1;
    // These are static rule candidates, not a count of painted DOM layers.
    if (name.includes("/themes/")) {
      const match = /^:root\.(dark-mode\.)?theme-([\w-]+)$/.exec(rule.selector);
      if (match) palettes.set(`${match[2]}/${match[1] ? "dark" : "light"}`,
        new Map(rule.nodes.filter((node) => node.type === "decl").map((node) => [node.prop, node.value])));
    }
  });
  sheet.walkDecls((declaration) => {
    result.declarations += 1;
    if (declaration.important) {
      result.important += 1;
      if (!declaration.parent.selector?.includes("theme-")) result.unscopedImportant += 1;
    }
    if (/^(?:-webkit-)?backdrop-filter$/.test(declaration.prop)) result.backdropDeclarations += 1;
    if (cleanComponents.has(name) && (declaration.important
      || (/^(?:-webkit-)?backdrop-filter$/.test(declaration.prop) && declaration.value.trim() !== "none"))) {
      componentPolicyViolations.push({ file: name, line: declaration.source.start.line,
        property: declaration.prop, reason: declaration.important ? "priority override" : "backdrop layer" });
    }
    if (declaration.prop.startsWith("--")) definitions.add(declaration.prop);
    for (const match of declaration.value.matchAll(/var\(\s*(--dc-[\w-]+)/g)) {
      if (!references.has(match[1])) references.set(match[1], new Set());
      references.get(match[1]).add(name);
    }
  });
  results.push(result);
}

const sums = (selected) => Object.fromEntries([
  "rules", "declarations", "important", "unscopedImportant", "backdropDeclarations",
  "activeBackdropRules", "legacyTagRules", "longSelectors", "idSelectors",
].map((key) => [key, selected.reduce((sum, entry) => sum + entry[key], 0)]));
const report = {
  totals: sums(results),
  themes: sums(results.filter((entry) => entry.file.includes("/themes/"))),
  core: sums(results.filter((entry) => entry.file.includes("/theme-core/"))),
  components: sums(results.filter((entry) => entry.file.includes("/components/"))),
  undefinedSemanticTokens: [...references].filter(([name]) => !definitions.has(name))
    .map(([name, paths]) => ({ name, files: [...paths] })),
  invalidPaletteMappings: ["retro", "mica", "acrylic", "macos", "scifi", "liquid-glass"].flatMap((theme) =>
    ["light", "dark"].flatMap((mode) => {
      const name = `${theme}/${mode}`;
      const palette = palettes.get(name);
      const invalid = paletteKeys.filter((key) => !palette?.has(key)
        || !(key === "--dc-radius" ? /^(?:\d|1\d|2[0-4])px$/ : /^#[\da-f]{6}$/i).test(palette.get(key)));
      return invalid.length ? [{ name, tokens: invalid }] : [];
    })),
  componentPolicyViolations,
  files: results,
};
const baselineAt = process.argv.indexOf("--baseline");
if (baselineAt !== -1) {
  const baseline = JSON.parse(readFileSync(process.argv[baselineAt + 1], "utf8"));
  report.change = Object.fromEntries(Object.entries(report.totals).map(([key, value]) => [key, value - baseline.totals[key]]));
}
if (process.argv.includes("--json")) {
  console.log(JSON.stringify(report, null, 2));
} else {
  console.log(JSON.stringify({ totals: report.totals, themes: report.themes,
    core: report.core, components: report.components, change: report.change,
    undefinedSemanticTokens: report.undefinedSemanticTokens, invalidPaletteMappings: report.invalidPaletteMappings,
    componentPolicyViolations }, null, 2));
}
if (process.argv.includes("--check") && (report.undefinedSemanticTokens.length || report.invalidPaletteMappings.length
  || componentPolicyViolations.length || report.totals.legacyTagRules)) process.exitCode = 1;
