import babelParser from "@babel/eslint-parser";
import js from "@eslint/js";
import eslintConfigPrettier from "eslint-config-prettier";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import { defineConfig, globalIgnores } from "eslint/config";
import globals from "globals";

/**
 * typescript-eslint does not support TypeScript 7 yet, so TS/TSX is parsed
 * with Babel. `tsc --noEmit` remains the typechecker (`yarn build`).
 */
const tsLanguageOptions = {
  parser: babelParser,
  parserOptions: {
    requireConfigFile: false,
    babelOptions: {
      babelrc: false,
      configFile: false,
      parserOpts: {
        plugins: ["typescript", "jsx"],
      },
    },
  },
};

export default defineConfig([
  globalIgnores(["dist/**", "dist-ssr/**", "node_modules/**", "src-tauri/target/**", "src-tauri/gen/**"]),
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended],
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: globals.browser,
      ...tsLanguageOptions,
    },
    rules: {
      // Babel's ESTree output does not carry TypeScript types, so these would
      // false-positive on `type`/`interface` and type-only imports.
      "no-undef": "off",
      "no-unused-vars": "off",
      "no-redeclare": "off",
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },
  {
    files: ["vite.config.ts"],
    languageOptions: {
      globals: globals.node,
    },
  },
  {
    files: ["**/*.{js,mjs}"],
    extends: [js.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
  },
  eslintConfigPrettier,
]);
