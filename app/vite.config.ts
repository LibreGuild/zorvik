/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The dev bridge (crates/devbridge) lets the UI run in a normal browser against
// the real Rust API: `cargo run -p zorvik-devbridge` then `npm run dev`.
const bridge = process.env.ZORVIK_BRIDGE ?? "http://127.0.0.1:18799";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 14200,
    strictPort: true,
    proxy: {
      "/bridge": {
        target: bridge,
        ws: true,
        changeOrigin: false,
        // Page reloads drop the events socket; that's expected, not an error.
        configure: (proxy) => proxy.on("error", () => {}),
      },
    },
  },
  build: {
    target: "es2022",
    chunkSizeWarningLimit: 2000,
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    // One graphql for the tests: cm6-graphql's language service loads its CommonJS build, and graphql
    // refuses schemas made by another copy (the app bundle only ever has the ESM one).
    alias: [{ find: /^graphql$/, replacement: "graphql/index.js" }],
  },
});
