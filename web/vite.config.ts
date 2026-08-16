import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// A RackForge host serves the interface from its root. The published demo
// lives under a repository path, so the base is configurable at build time.
const base = process.env.RACKFORGE_WEB_BASE ?? "/";

export default defineConfig({
  base,
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:8787",
      "/ws": {
        target: "ws://127.0.0.1:8787",
        ws: true,
      },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
