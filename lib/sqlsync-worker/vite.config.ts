import { resolve } from "path";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import dts from "vite-plugin-dts";
import wasmPack from "vite-plugin-wasm-pack";

export default defineConfig({
  plugins: [
    wasmPack(["sqlsync-worker-crate"]),
    dts({
      include: ["src/main.ts"],
      rollupTypes: true,
      insertTypesEntry: true,
    }),
  ],
  build: {
    lib: {
      entry: {
        main: resolve(__dirname, "src/main.ts"),
        worker: resolve(__dirname, "src/worker.ts"),
      },
      fileName: "[name]",
      formats: ["es"],
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
      ],
    },
  },
});
