import commonjs from "@rollup/plugin-commonjs";
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
  plugins: [commonjs(), typescript(), nodeResolve()],
});

export default [output("SQLSync", "src/index.ts"), output("SQLSyncWorker", "src/worker.ts")];
