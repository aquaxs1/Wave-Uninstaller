import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  // three.js is vendored as published: one minified file, not ours to lint.
  { ignores: ["dist", "src-tauri/target", "target", "site/assets/aqx/three.module.min.js"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    // The website's scripts are ES modules that run in the browser — without
    // the browser globals every DOM call reads as an undefined variable.
    files: ["site/**/*.js"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: globals.browser,
    },
  },
  {
    // The one classic script: it runs before the page paints.
    files: ["site/assets/aqx/boot.js"],
    languageOptions: { sourceType: "script" },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/consistent-type-imports": "error",
    },
  },
);
