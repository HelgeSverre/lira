// "Lira Night" — the brand syntax theme from docs/branding.md, expressed
// as a Shiki/TextMate theme. Token colors come straight from the brand table.
export const liraNight = {
  name: "lira-night",
  type: "dark",
  colors: {
    "editor.background": "#1A1B26",
    "editor.foreground": "#F5F5F7",
  },
  settings: [
    { settings: { background: "#1A1B26", foreground: "#F5F5F7" } },
    {
      scope: ["comment", "punctuation.definition.comment"],
      settings: { foreground: "#6B7280", fontStyle: "italic" },
    },
    {
      scope: ["keyword", "storage.modifier", "keyword.control"],
      settings: { foreground: "#E8A924" },
    },
    {
      scope: ["string", "string.quoted", "constant.character.escape"],
      settings: { foreground: "#7AA874" },
    },
    {
      scope: ["constant.numeric", "constant.language"],
      settings: { foreground: "#6B9AC4" },
    },
    {
      scope: ["entity.name.function", "support.function"],
      settings: { foreground: "#F5F5F7" },
    },
    {
      scope: [
        "storage.type",
        "entity.name.type",
        "support.type",
        "variable.language",
      ],
      settings: { foreground: "#C4A7E7" },
    },
    {
      scope: ["keyword.operator", "punctuation"],
      settings: { foreground: "#89DDFF" },
    },
    {
      scope: ["variable", "meta.embedded"],
      settings: { foreground: "#F5F5F7" },
    },
  ],
};

/** @type {import('shiki').ThemeRegistrationRaw} */
export const liraNightTheme = liraNight;
