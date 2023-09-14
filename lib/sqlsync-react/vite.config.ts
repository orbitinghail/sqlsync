import { resolve } from "path";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import react from "@vitejs/plugin-react";
import dts from "vite-plugin-dts";

export default defineConfig({
  plugins: [
    react(),
    dts({
      exclude: "test/**/*",
    }),
  ],
  build: {
    lib: {
      entry: resolve(__dirname, "src/index.ts"),
      name: "SQLSyncReact",
      formats: ["es", "umd"],
    },
    sourcemap: true,
    rollupOptions: {
      external: ["react"],
      output: {
        exports: "named",
        globals: {
          react: "React",
        },
      },
    },
  },
  server: {
    fs: {
      allow: [searchForWorkspaceRoot(process.cwd())],
    },
  },
});
