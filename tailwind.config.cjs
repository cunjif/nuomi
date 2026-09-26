/** Design tokens — the ONLY source for colors/spacing (no magic values). */
module.exports = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Dark-first palette; light theme mirrors via CSS vars in M-UI1 polish.
        surface: {
          DEFAULT: "var(--nuomi-surface, #0b0c0e)",
          raised: "var(--nuomi-surface-raised, #141518)",
          overlay: "var(--nuomi-surface-overlay, #1b1d21)",
          scrim: "var(--nuomi-scrim, rgba(0, 0, 0, 0.55))",
        },
        // Hairline separators. The alpha is baked into the per-theme CSS var on
        // purpose: this project maps colors to `var(...)` strings, and Tailwind
        // v3 cannot inject an alpha into a `var()` color — `bg-ink-muted/30`
        // silently emits nothing. So the tint lives in the token, not the class.
        divider: "var(--nuomi-divider, rgba(154, 163, 173, 0.26))",
        ink: {
          DEFAULT: "var(--nuomi-ink, #e6e6e6)",
          muted: "var(--nuomi-ink-muted, #9aa3ad)",
          accent: "var(--nuomi-accent, #5b9bf8)",
        },
        state: {
          ok: "var(--nuomi-ok, #9ece6a)",
          warn: "var(--nuomi-warn, #e0af68)",
          danger: "var(--nuomi-danger, #f7768e)",
        },
        // Capability badge ink (KiloCode-style Re/I/Vo/Vi) — token, not raw
        // palette, so all four themes pick a legible variant (review §10).
        cap: {
          re: "var(--nuomi-cap-re, #0369a1)",
          i: "var(--nuomi-cap-i, #047857)",
          vo: "var(--nuomi-cap-vo, #a16207)",
          vi: "var(--nuomi-cap-vi, #86198f)",
        },
        // Diff "added" ink (DiffView). Blue rather than the green `state.ok`:
        // red/blue stays separable under the common red-green deficiencies.
        // Removed lines use `state.danger` (the app's single red).
        diff: {
          add: "var(--nuomi-diff-add, #0369a1)",
        },
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "sans-serif"],
        mono: ["JetBrains Mono", "Consolas", "monospace"],
        // Hand-drawn line-art faces (review §3.1). Latin only — CJK falls
        // back to system-ui via the font stack in global.css.
        hand: ["Patrick Hand", "cursive"], // 标题/品牌
        note: ["Kalam", "system-ui"], // 正文强调/空状态
        scribble: ["Caveat", "cursive"], // 标注/便签/时间戳
      },
      // Hard-edged, blur-free offset shadows — the signature "sketch" shadow
      // that mimics a hand-drawn drop shadow (review §7.2.1).
      boxShadow: {
        "sketch-sm": "2px 2px 0 var(--nuomi-ink-muted)",
        "sketch-md": "3px 3px 0 var(--nuomi-ink-muted)",
        "sketch-lg": "5px 5px 0 var(--nuomi-ink-muted)",
        "sketch-accent": "3px 3px 0 var(--nuomi-accent)",
      },
      keyframes: {
        // "Pen stroke" entrance — opacity + slight rise (review §8.2.2).
        "sketch-draw-in": {
          from: { opacity: "0", transform: "translateY(8px)" },
          to: { opacity: "1", transform: "translateY(0)" },
        },
        // SVG stroke "draws itself" in (paired with .animate-stroke).
        "sketch-stroke-draw": {
          from: { strokeDashoffset: "100" },
          to: { strokeDashoffset: "0" },
        },
      },
      animation: {
        // `backwards` (not `both`): after the entrance finishes the element
        // returns to its natural styles, so hover transforms (TaskCard lift)
        // are not pinned by the fill-mode's final keyframe.
        "draw-in": "sketch-draw-in 200ms ease-out backwards",
        "stroke": "sketch-stroke-draw 400ms ease-out both",
      },
    },
  },
  plugins: [],
};
