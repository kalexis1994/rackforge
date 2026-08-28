import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// The UI carries the revision it was built from, and every deploy of this
// dist writes the same stamp beside it (ui-revision.txt) so each host's
// /api/v1/health can report which interface it is actually serving.
function uiRevision(): string {
  try {
    return execSync("git rev-parse --short HEAD", { cwd: __dirname })
      .toString()
      .trim();
  } catch {
    try {
      const stamped = readFileSync(join(__dirname, "..", "REVISION"), "utf8").trim();
      if (!stamped.includes("Format")) return stamped;
    } catch {
      /* archives without the stamp fall through */
    }
    return "dev";
  }
}

const revision = uiRevision();

function emitRevisionStamp(): Plugin {
  return {
    name: "rackforge-ui-revision",
    closeBundle() {
      writeFileSync(join(__dirname, "dist", "ui-revision.txt"), revision + "\n");
    },
  };
}

// A RackForge host serves the interface from its root. The published demo
// lives under a repository path, so the base is configurable at build time.
const base = process.env.RACKFORGE_WEB_BASE ?? "/";

export default defineConfig({
  base,
  plugins: [react(), emitRevisionStamp()],
  define: {
    __UI_REVISION__: JSON.stringify(revision),
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    // Cross-origin isolation, which is what makes SharedArrayBuffer exist:
    // without it the browser host cannot share memory with its render
    // workers and stays on its sequential path. Same headers every RackForge
    // gateway serves; the dev server must match or multicore is untestable.
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    proxy: {
      "/api": "http://127.0.0.1:8787",
      "/ws": {
        target: "ws://127.0.0.1:8787",
        ws: true,
      },
    },
  },
  preview: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
