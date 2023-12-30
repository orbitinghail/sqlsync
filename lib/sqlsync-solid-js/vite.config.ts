import { resolve } from "path";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import dts from "vite-plugin-dts";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
  plugins: [
    solidPlugin(),
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
