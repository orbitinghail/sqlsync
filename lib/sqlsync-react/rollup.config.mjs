import { nodeResolve } from "@rollup/plugin-node-resolve";
import typescript from "@rollup/plugin-typescript";

export default {
  input: "src/index.ts",
  output: {
    dir: "dist",
    format: "es",
    sourcemap: true,
  },
  external: ["react", "@orbitinghail/sqlsync-worker"],
  plugins: [typescript(), nodeResolve()],
};
