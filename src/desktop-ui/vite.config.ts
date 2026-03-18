import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import path from "path";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },
  },
  clearScreen: false,
  server: {
    port: 5181,
    strictPort: true,
    proxy: {
      "/tray": {
        target: "http://localhost:5182",
        rewrite: (p) => p.replace(/^\/tray/, ""),
      },
    },
  },
});
