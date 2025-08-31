import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  // Needed for Tauri on some Linux WebKitGTK builds
  envPrefix: ["VITE_", "TAURI_"],
  esbuild: {
    target: "es2021",
  },
  build: {
    target: ["es2021", "chrome100", "safari13"],
  },
}))
