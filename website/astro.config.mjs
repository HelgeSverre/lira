// @ts-check
import { defineConfig } from "astro/config";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { liraNightTheme } from "./src/lib/lira-night.mjs";

// Load the real Lira TextMate grammar shipped with the VS Code extension.
// This is the same grammar the editor uses, so highlighting on the site
// matches the editor exactly (no "close language" fallback needed).
const liraGrammar = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        "../editors/vscode-lira/syntaxes/lira.tmLanguage.json",
        import.meta.url,
      ),
    ),
    "utf-8",
  ),
);

// https://astro.build/config
export default defineConfig({
  site: "https://liralang.dev",
  output: "static",
  markdown: {
    shikiConfig: {
      theme: liraNightTheme,
      langs: [
        {
          ...liraGrammar,
          // Shiki expects an `id`/`name`; the grammar exposes `scopeName`.
          name: "lira",
          scopeName: "source.lira",
        },
      ],
    },
  },
});
