import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => {
  const env = loadEnv(mode, ".", "");
  const host = env.TAURI_DEV_HOST;
  const mygo = mode === "mygo";
  const shellModule = (name: string) => fileURLToPath(new URL(`./src/shared/mygo/${name}.ts`, import.meta.url));

  return {
    plugins: [react(), ...(mygo ? [{
      name: "mygo-local-content",
      transformIndexHtml() {
        return [{ tag: "meta", injectTo: "head-prepend" as const, attrs: {
          "http-equiv": "Content-Security-Policy",
          content: "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' blob:; img-src 'self' data: blob:; media-src 'self' data: blob:; connect-src 'self'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
        } }];
      },
    }] : [])],
    resolve: { alias: mygo ? {
      "@tauri-apps/api/app": shellModule("app"),
      "@tauri-apps/api/event": shellModule("events"),
      "@tauri-apps/plugin-dialog": shellModule("dialog"),
      "@tauri-apps/plugin-opener": shellModule("opener"),
    } : {} },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
        protocol: "ws",
        host,
        port: 1421,
      }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
    build: {
      outDir: mygo ? "dist/mygo-web" : "dist/web",
      emptyOutDir: true,
    }
  };
});
