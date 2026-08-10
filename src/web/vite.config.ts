import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import path from "path";
import { defineConfig } from "vite";

const backendTarget = process.env.VITE_VA_BACKEND_URL ?? "http://127.0.0.1:12358";
const packageJson = JSON.parse(
  readFileSync(path.resolve(__dirname, "package.json"), "utf8"),
) as { version?: string };
const packageVersion = packageJson.version ?? "0.0.0";
const backendProxy = {
  "/va/api": {
    target: backendTarget,
    changeOrigin: true,
  },
  "/va/mcp": {
    target: backendTarget,
    changeOrigin: true,
  },
  "/va/ws": {
    target: backendTarget,
    changeOrigin: true,
    ws: true,
  },
  "/va/preview/assets": {
    target: backendTarget,
    changeOrigin: true,
  },
  "/va/preview/s": {
    target: backendTarget,
    changeOrigin: true,
  },
  "^/va/preview/u/[^/]+/(bootstrap|content|chat)": {
    target: backendTarget,
    changeOrigin: true,
    ws: true,
  },
};

function manualChunks(id: string) {
  if (!id.includes("node_modules")) return undefined;
  if (id.includes("shiki") || id.includes("@shikijs")) return undefined;
  if (id.includes("radix-ui") || id.includes("lucide-react")) return "vendor-ui";
  if (id.includes("react") || id.includes("scheduler")) return "vendor-react";
  if (id.includes("@agentclientprotocol")) return "vendor-acp";
  if (id.includes("zod")) return "vendor-zod";
  return undefined;
}

export default defineConfig(({ mode }) => {
  const versionLabel =
    mode === "production" ? packageVersion : `${packageVersion} dev`;

  return {
    base: "/va/",
    define: {
      __APP_VERSION_LABEL__: JSON.stringify(versionLabel),
    },
    plugins: [react(), tailwindcss()],
    build: {
      // Shiki keeps full language grammars in lazy chunks. A few grammars are
      // larger than Vite's 500KB default, but they are fetched only when a
      // matching code block is rendered.
      chunkSizeWarningLimit: 800,
      rollupOptions: {
        output: {
          manualChunks,
        },
      },
    },
    resolve: {
      alias: [
        { find: "@va/ui/button", replacement: path.resolve(__dirname, "../shared/ui/src/button.tsx") },
        { find: "@va/ui/dropdown-menu", replacement: path.resolve(__dirname, "../shared/ui/src/dropdown-menu.tsx") },
        { find: "@va/ui/input", replacement: path.resolve(__dirname, "../shared/ui/src/input.tsx") },
        { find: "@va/ui", replacement: path.resolve(__dirname, "../shared/ui/src/index.ts") },
        { find: "@va/i18n", replacement: path.resolve(__dirname, "../shared/i18n/src/index.tsx") },
        { find: "@va/client", replacement: path.resolve(__dirname, "../shared/client-ts/src/index.ts") },
        { find: "@", replacement: path.resolve(__dirname, "./src") },
      ],
    },
    server: {
      port: 5180,
      strictPort: true,
      proxy: backendProxy,
    },
    preview: {
      proxy: backendProxy,
    },
  };
});
