import { nodeResolve } from "@rollup/plugin-node-resolve";
import typescript from "@rollup/plugin-typescript";

const output = (name, entry) => ({
  input: entry,
  output: {
    dir: "dist",
    format: "umd",
    sourcemap: true,
    name,
  },
  plugins: [typescript(), nodeResolve()],
});

export default [
  output("SQLSyncWorkerTypes", "src/index.ts"),
  output("SQLSyncWorker", "src/worker.ts"),
];
