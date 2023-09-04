import { resolve } from "path";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import dts from "vite-plugin-dts";

export default defineConfig({
  plugins: [
    dts({
      exclude: "test/**/*",
    }),
  ],
  build: {
    lib: {
      entry: resolve(__dirname, "src/sqlsync-react.ts"),
      name: "sqlsync-react",
      formats: ["es", "umd"],
    },
    sourcemap: true,
    rollupOptions: {
      output: {
        exports: "named",
      },
    },
  },
  server: {
    fs: {
      allow: [
        searchForWorkspaceRoot(process.cwd()),
        "../../target/wasm32-unknown-unknown/debug/test_reducer.wasm",
        "../../target/wasm32-unknown-unknown/release/test_reducer.wasm",
      ],
    },
  },
});
