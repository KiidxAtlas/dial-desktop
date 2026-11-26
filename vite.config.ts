import { defineConfig, type Plugin } from "vite";
import vue from "@vitejs/plugin-vue";

/**
 * beautiful-mermaid draws every decision as a square as wide as its label, so one question can
 * stand taller than the rest of a diagram. Size it as a flat rhombus instead: its half-width is
 * the label's plus `MARGIN`, and its half-height is just enough that the label's box fits inside
 * (x/a + y/b ≤ 1). Edges are clipped to the diamond's real shape, so they still meet its sides.
 * The build fails if an update changes the code this replaces.
 */
function flatDecisions(): Plugin {
  const MARGIN = 28;
  const original = `  if (shape === "diamond") {
    const side = Math.max(width, height) + NODE_PADDING.diamondExtra;
    width = side;
    height = side;
  }`;
  const flat = `  if (shape === "diamond") {
    width = metrics.width + ${MARGIN * 2};
    height = Math.max(metrics.height + 44, Math.ceil(metrics.height * width / ${MARGIN * 2}) + 8);
  }`;
  return {
    name: "dial:flat-decisions",
    transform(code, id) {
      if (!/beautiful-mermaid[\\/]dist[\\/]index\.js$/.test(id.split("?")[0])) return;
      if (!code.includes(original)) {
        throw new Error("beautiful-mermaid changed its diamond sizing; update flatDecisions in vite.config.ts");
      }
      return { code: code.replace(original, flat), map: null };
    },
  };
}

// Tauri expects a fixed port and loads `dist/` in release builds.
export default defineConfig({
  plugins: [vue(), flatDecisions()],
  clearScreen: false,
  // Served as source so `flatDecisions` applies in development too; its CommonJS dependencies
  // are still pre-bundled.
  optimizeDeps: {
    exclude: ["beautiful-mermaid"],
    include: ["beautiful-mermaid > elkjs/lib/elk.bundled.js", "beautiful-mermaid > entities"],
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // The UI runs in the OS webview, not a bundled Chromium. Also lower dependency syntax.
    target: ["safari13.1", "es2020"],
    chunkSizeWarningLimit: 1500,
  },
});
