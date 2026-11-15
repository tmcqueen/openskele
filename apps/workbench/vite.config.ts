import { defineConfig } from "vite";

// Tauri expects a fixed dev port and its own output on the terminal.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", chunkSizeWarningLimit: 4096 },
});
