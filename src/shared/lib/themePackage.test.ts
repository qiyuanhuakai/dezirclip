import { describe, expect, it } from "vitest";
import { parseThemeCatalog, parseThemeJson, parseThemePackage, resolveThemeSelection, type ThemePackage } from "./themePackage";
import { validateThemeCss } from "./themeCss";

const example = (): ThemePackage => ({
  format: "dezirclip-theme", version: 1, id: "my-theme", name: "我的主题🙂", base: "mica",
  css: `:root.theme-custom { --dc-accent: #456789; }
    body.theme-custom #root { display: grid; border-radius: 18px 4px; background: linear-gradient(120deg,#123456,#abcdef); }
    body.theme-custom.dark-mode #root { background: #123; }
    @media (min-width: 600px) { .theme-custom .panel { width: calc(100% - 2rem); color: var(--dc-accent); } }
    @supports (display: grid) { .theme-custom .panel { display: grid; } }
    @container panel (min-width: 20rem) { .theme-custom .panel { grid-template-columns: 1fr 2fr; } }
    @keyframes breathe { from { opacity: .8; } to { opacity: 1; } }`
});

describe("complete CSS theme packages", () => {
  it("round-trips selectors, geometry, layout, modes, and modern CSS functions", () => {
    const input = example();
    expect(parseThemeJson(JSON.stringify(input))).toEqual(input);
    expect(parseThemeJson(`\uFEFF${JSON.stringify(input)}`)).toEqual(input);
    expect(parseThemePackage({ ...input, name: "  我的主题🙂  ", author: " ", description: null })).toEqual(input);
    expect(() => validateThemeCss(String.raw`.theme-custom::after { content: "url(https://example.com)"; width: max(10px, min(20vw, 300px)); }`)).not.toThrow();
    expect(() => validateThemeCss('/* A comment mentioning sourceURL and url() without a directive. */ .x{}')).not.toThrow();
  });
  it("rejects all resource forms, escaped identifiers, nesting, and debug references", () => {
    for (const css of [
      '@import "remote.css";', String.raw`@\69mport "remote.css";`, '@font-face { font-family:x; }',
      String.raw`@\66ont-face { font-family:x; }`, '@namespace svg "https://example.com";',
      '@document url-prefix("https://example.com") {}', '@-moz-document domain("example.com") {}',
      '.x { background: url(asset.png); }', String.raw`.x { background: \75\72\6c("asset.png"); }`,
      '.x { --asset: url("https://example.com"); }', '.x { background: image-set("a.png" 1x); }',
      '.x { background: -webkit-image-set("a.png" 1x); }', '.x { background: image("a.png"); }',
      '.x { background: src("a.png"); }', '.x { width: expression(alert(1)); }',
      String.raw`@unknown { .x { --asset: u\72l("asset.png"); } }`,
      '.x { behavior: none; }', String.raw`.x { b\65havior: none; }`, '.x { -moz-binding: none; }',
      '/*# sourceMappingURL=remote.css.map */ .x{}', '/*# sourceURL=remote.css */ .x{}'
    ]) expect(() => parseThemePackage({ ...example(), css }), css).toThrow();
  });
  it("rejects malformed strings, unmatched delimiters, excessive nesting and empty CSS", () => {
    for (const css of ['', '   ', ' /* doc */ ', '/* unterminated', '.x { color: red;', '.x { content: "unterminated; }', '.x { color: red; }}', '.x { width: calc(2px]; }', `a{width:${'calc('.repeat(130)}1px${')'.repeat(130)}}`]) expect(() => validateThemeCss(css), css).toThrow();
  });
  it("rejects old palette payloads, unknown fields and unsupported metadata", () => {
    for (const change of [{ modes: { light: {}, dark: {} } }, { script: 'alert(1)' }, { version: 2 }, { base: '../../unknown' }]) expect(() => parseThemePackage({ ...example(), ...change })).toThrow();
    expect(() => parseThemePackage({ ...example(), name: '🙂'.repeat(80) })).not.toThrow();
    for (const name of ['🙂'.repeat(81), 'line\nbreak', 'control\u0085value', 'unpaired\ud800']) expect(() => parseThemePackage({ ...example(), name })).toThrow();
    for (const id of ['', '../x', 'Upper', '-slug', 'slug-', 'two--dashes', 'slug\n', 'x'.repeat(49)]) expect(() => parseThemePackage({ ...example(), id })).toThrow();
  });
  it("enforces the 256KiB file/serialized limits and 32-item unique catalog", () => {
    expect(() => parseThemeJson(' '.repeat(262145))).toThrow('too large');
    expect(() => parseThemePackage({ ...example(), css: '/*' + 'x'.repeat(262144) + '*/' })).toThrow();
    expect(() => parseThemePackage({ ...example(), css: '/*' + '"'.repeat(140000) + '*/ .x{}' })).toThrow('too large');
    expect(() => parseThemeCatalog([example(), example()])).toThrow('Duplicate');
    expect(() => parseThemeCatalog(Array.from({ length: 33 }, (_, index) => ({ ...example(), id: `theme-${index}` })))).toThrow();
  });
  it("resolves native base metadata and missing IDs safely", () => {
    const input = { ...example(), base: 'retro' as const };
    expect(resolveThemeSelection('custom:my-theme', [input])).toEqual({ base: 'retro', custom: input });
    expect(resolveThemeSelection('custom:deleted', [input])).toEqual({ base: 'mica' });
    expect(resolveThemeSelection('liquid-glass', [input])).toEqual({ base: 'liquid-glass' });
  });
});
