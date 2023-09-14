module.exports = {
  root: true,
  env: { browser: true, es2020: true },
  extends: [
    "eslint:recommended",
    "plugin:@typescript-eslint/recommended-type-checked",
    "plugin:@typescript-eslint/stylistic-type-checked",
  ],
  ignorePatterns: [
    "dist",
    ".eslintrc.cjs",
    "sqlsync-wasm",
    "rollup.config.mjs",
  ],
  parser: "@typescript-eslint/parser",
  parserOptions: {
    project: ["./tsconfig.json"],
    ecmaVersion: "latest",
    sourceType: "module",
    tsconfigRootDir: __dirname,
  },
  rules: {
    "@typescript-eslint/ban-tslint-comment": "off",
  },
  settings: {
    react: { version: "detect" },
  },
};
