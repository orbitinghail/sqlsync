module.exports = {
  root: true,
  env: { browser: true, es2020: true },
  extends: [
    "eslint:recommended",
    "plugin:@typescript-eslint/recommended-type-checked",
    "plugin:@typescript-eslint/stylistic-type-checked",
    "plugin:react/recommended",
    "plugin:react/jsx-runtime",
    "plugin:react-hooks/recommended",
  ],
  ignorePatterns: ["dist", ".eslintrc.cjs"],
  parser: "@typescript-eslint/parser",
  parserOptions: {
    project: ["./tsconfig.json", "./tsconfig.node.json"],
    ecmaVersion: "latest",
    sourceType: "module",
    tsconfigRootDir: __dirname,
  },
  plugins: ["react-refresh"],
  rules: {
    "react-refresh/only-export-components": ["warn"],
  },
  settings: {
    react: { version: "detect" },
  },
};
