export function nativeSurfaceAppearance() {
  const style = getComputedStyle(document.documentElement);
  const dark = document.documentElement.classList.contains("dark-mode");
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 1;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  const color = (value: string, fallback: string) => {
    if (!context || !CSS.supports("color", value)) return fallback;
    context.clearRect(0, 0, 1, 1);
    context.fillStyle = value;
    context.fillRect(0, 0, 1, 1);
    const bytes = context.getImageData(0, 0, 1, 1).data;
    if (bytes[3] < 20) return fallback;
    return "#" + Array.from(bytes.slice(0, 3), byte => byte.toString(16).padStart(2, "0")).join("");
  };
  const token = (name: string, fallback: string) => color(style.getPropertyValue(name).trim(), fallback);
  const number = (name: string, fallback: number, low: number, high: number) => {
    const value = Number.parseFloat(style.getPropertyValue(name));
    return Number.isFinite(value) ? Math.max(low, Math.min(high, value)) : fallback;
  };
  return { Mode: dark ? "dark" : "light", Locale: document.documentElement.lang || "zh-CN",
    Opacity: number("--surface-opacity", 0.65, 0, 1), Tokens: {
      Background: color(`rgb(${style.getPropertyValue("--surface-shell-rgb").trim()})`, dark ? "#202124" : "#fcfcfc"),
      Panel: token("--bg-input", dark ? "#292a2e" : "#ffffff"),
      Text: token("--text-primary", dark ? "#eeeeee" : "#373737"),
      Muted: token("--text-secondary", dark ? "#aaaaaa" : "#707070"),
      Accent: token("--accent-color", dark ? "#88afff" : "#487bdb"),
      Selected: token("--bg-selected", dark ? "#344668" : "#e6edfb"),
      Border: token("--border-light", dark ? "#45464c" : "#d5d5d5"),
      Radius: Math.round(number("--radius-md", 8, 0, 32)),
      FontSize: Math.round(number("--clipboard-item-font-size", 13, 8, 32)),
      FontFamilies: [(style.getPropertyValue("--font-main").split(",")[0] || "Segoe UI").trim().replace(/^['"]|['"]$/g, "").slice(0, 256)],
      Padding: 12, Gap: 8, RowHeight: 52, SearchHeight: 36, TransitionMs: 120, RegularWeight: 400, EmphasisWeight: 600
    } };
}

export function observeNativeAppearance() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let previous = "";
  const schedule = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const appearance = nativeSurfaceAppearance();
      const key = JSON.stringify(appearance);
      if (key === previous) return;
      void window.mygo.call("DesktopShell.SyncAppearance", appearance).then(() => { previous = key; }).catch(console.error);
    }, 150);
  };
  const observer = new MutationObserver(schedule);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style", "lang"] });
  observer.observe(document.head, { childList: true, subtree: true });
  document.addEventListener("load", schedule, true);
  window.addEventListener("dezirclip-theme-applied", schedule);
  schedule();
  window.addEventListener("pagehide", () => { observer.disconnect(); clearTimeout(timer); document.removeEventListener("load", schedule, true); window.removeEventListener("dezirclip-theme-applied", schedule); }, { once: true });
}
