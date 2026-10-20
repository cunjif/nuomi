/** Design tokens — the ONLY source for colors/spacing (no magic values). */
module.exports = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Dark-first palette; light theme mirrors via CSS vars in M-UI1 polish.
        // Alpha-capable colors map to `rgb(var(--nuomi-X-rgb) / <alpha-value>)`
        // so Tailwind's alpha modifier compiles: `bg-state-warn/10` emits
        // `rgb(var(--nuomi-warn-rgb) / 0.1)`. Each `-rgb` triple lives in
        // global.css beside its hex twin `--nuomi-X` (still used by boxShadow
        // and raw var() references) and is guarded 1:1 by themeContrast.test.ts.
        surface: {
          DEFAULT: "rgb(var(--nuomi-surface-rgb, 11 12 14) / <alpha-value>)",
          raised: "rgb(var(--nuomi-surface-raised-rgb, 20 21 24) / <alpha-value>)",
          overlay: "rgb(var(--nuomi-surface-overlay-rgb, 27 29 33) / <alpha-value>)",
          // Scrim keeps its alpha baked into the per-theme var on purpose: it
          // is never used with an alpha modifier, so it has no rgb mirror.
          scrim: "var(--nuomi-scrim, rgba(0, 0, 0, 0.55))",
        },
        // Hairline separators. Same baked-alpha reason as scrim — never
        // alpha-modified, no rgb mirror.
        divider: "var(--nuomi-divider, rgba(154, 163, 173, 0.26))",
        ink: {
          DEFAULT: "rgb(var(--nuomi-ink-rgb, 230 230 230) / <alpha-value>)",
          muted: "rgb(var(--nuomi-ink-muted-rgb, 154 163 173) / <alpha-value>)",
          accent: "rgb(var(--nuomi-accent-rgb, 91 155 248) / <alpha-value>)",
        },
        state: {
          ok: "rgb(var(--nuomi-ok-rgb, 158 206 106) / <alpha-value>)",
          warn: "rgb(var(--nuomi-warn-rgb, 224 175 104) / <alpha-value>)",
          danger: "rgb(var(--nuomi-danger-rgb, 247 118 142) / <alpha-value>)",
        },
        // Capability badge ink (KiloCode-style Re/I/Vo/Vi) — token, not raw
        // palette, so all four themes pick a legible variant (review §10).
        // Never alpha-modified, so no rgb mirror.
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
          add: "rgb(var(--nuomi-diff-add-rgb, 3 105 161) / <alpha-value>)",
        },
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "sans-serif"],
        // Single source: the --font-mono stack in global.css (P2-E) — keeps
        // the `font-mono` class, .md-wysiwyg code, and Monaco in lockstep.
        mono: ["var(--font-mono, monospace)"],
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
