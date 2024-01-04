import { nodeResolve } from "@rollup/plugin-node-resolve";
import typescript from "@rollup/plugin-typescript";

export default {
  input: "src/index.ts",
  output: {
    dir: "dist",
    format: "umd",
    sourcemap: true,
    name: "SQLSyncReact",
  },
  plugins: [nodeResolve(), typescript()],
};
