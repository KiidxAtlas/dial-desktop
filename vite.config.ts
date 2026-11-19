import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri expects a fixed port and loads `dist/` in release builds.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    chunkSizeWarningLimit: 1500,
  },
});
