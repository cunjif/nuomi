// eslint flat config — minimal, strictness comes from tsc.
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/", "src/lib/ipc/bindings.gen.ts"] },
  ...tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
);
