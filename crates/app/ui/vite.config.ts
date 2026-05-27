/// <reference types="vitest" />
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port. 1420 matches `devUrl` in tauri.conf.json.
const TAURI_DEV_PORT = 1420;

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: TAURI_DEV_PORT,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});
