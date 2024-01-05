import commonjs from "@rollup/plugin-commonjs";
import { nodeResolve } from "@rollup/plugin-node-resolve";
import typescript from "@rollup/plugin-typescript";

const output = (entry) => ({
  input: entry,
  output: {
    dir: "dist",
    format: "es",
    sourcemap: true,
  },
  plugins: [commonjs(), typescript(), nodeResolve()],
});

export default [output("src/index.ts"), output("src/worker.ts")];
