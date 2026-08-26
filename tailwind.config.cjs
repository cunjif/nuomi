/** Design tokens — the ONLY source for colors/spacing (no magic values). */
module.exports = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Dark-first palette; light theme mirrors via CSS vars in M-UI1 polish.
        surface: {
          DEFAULT: "var(--nuomi-surface, #1e2229)",
          raised: "var(--nuomi-surface-raised, #262b33)",
          overlay: "var(--nuomi-surface-overlay, #2f3540)",
          scrim: "var(--nuomi-scrim, rgba(0, 0, 0, 0.5))",
        },
        ink: {
          DEFAULT: "var(--nuomi-ink, #e6e6e6)",
          muted: "var(--nuomi-ink-muted, #9aa3ad)",
          accent: "var(--nuomi-accent, #7aa2f7)",
        },
        state: {
          ok: "var(--nuomi-ok, #9ece6a)",
          warn: "var(--nuomi-warn, #e0af68)",
          danger: "var(--nuomi-danger, #f7768e)",
        },
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "sans-serif"],
        mono: ["JetBrains Mono", "Consolas", "monospace"],
      },
    },
  },
  plugins: [],
};
