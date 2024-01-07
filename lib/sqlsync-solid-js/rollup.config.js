import babel from "@rollup/plugin-babel";
import { nodeResolve } from "@rollup/plugin-node-resolve";
import typescript from "@rollup/plugin-typescript";

export default {
  input: "src/index.ts",
  output: {
    dir: "dist",
    format: "es",
    sourcemap: true,
  },
  external: ["solid-js", "@orbitinghail/sqlsync-worker"],
  plugins: [
    typescript(),
    nodeResolve(),
    babel({
      extensions: [".ts", ".tsx"],
      babelHelpers: "bundled",
      presets: ["solid", "@babel/preset-typescript"],
      exclude: [/node_modules\//],
    }),
  ],
};
