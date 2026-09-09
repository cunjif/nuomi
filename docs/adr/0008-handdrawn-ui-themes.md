# ADR 0008: 卡通纸质手绘线稿 UI 与 4 主题系统

- 状态：Accepted（依据 reviews/ui-handdrawn-paper-style-review.md，用户批准「完整方案 P0→P2」与「@fontsource 优先」两项决策）
- 日期：2026-09-09
- 决策人：james（经 plan-mode 批准）

## 背景 / Context

评审 `reviews/ui-handdrawn-paper-style-review.md` 提出把现有「纯黑背景 + 线稿」界面升级为**卡通纸质手绘线稿风格（Cartoon Paper Hand-drawn Line-art）**：描边即语言、纸面即容器、抖动即性格、手写即层级、像素即点缀、动画即笔触。同时要求多主题（纸质亮色 / 蓝网格 / 粉笔板暗色 / 高对比）与统一图标语言。

## 决策 / Decision

### 1. 六原则落地为三层资产

| 层 | 载体 | 内容 |
|---|---|---|
| Token / 原语 | `global.css` + `tailwind.config.cjs` | `--nuomi-paper-texture`（feTurbulence 牛皮纸）、`--nuomi-chalk-texture`、`--nuomi-grid-texture`；`shadow-sketch-*`（硬边无模糊偏移阴影）；`font-hand/note/scribble`；`animate-draw-in/stroke` |
| 组件类 | `@layer components` | `.sketch-card` / `.sketch-input`（虚线 + wobbly radius）、`.pixel-fill-*`、`.binder-holes`、`.torn-note` |
| 原子组件 | `src/components/ui/` | Button · Card · Dialog（含胶带）· Field · Badge · Tabs · Tooltip · EmptyState · Icon · Spinner（手绘弧线旋转） |

### 2. 4 主题 = 2 个基线块 + 2 个 data-theme 块

`:root`（paper-light）与 `.dark`（chalkboard-dark）保留为基线，`[data-theme="grid-notebook"]` 与 `[data-theme="high-contrast"]` 扩展。切换由 `uiStore.Theme` + `useTheme.applyTheme` 驱动：设 `documentElement.dataset.theme`，暗色族再挂 `.dark`。保留 `.dark` 语义使 Tailwind `darkMode: "class"` 与旧迁移路径继续工作。

**关键约束**：`themeContrast.test.ts` 从 global.css 解析 4 个块的全部语义 token 并断言 WCAG（文本 ≥4.5:1、UI ≥3:1）。新增能力徽章 token `--nuomi-cap-re/i/vo/vi` 同样入门禁。纹理只做 background-image 装饰，不改实色 token，故不破坏门禁。

### 3. 字体自托管优先

Patrick Hand（标题）/ Kalam（正文强调）/ Caveat（标注）经 `@fontsource/*` 自托管（离线可用、无 CDN 依赖）；CJK 回退 system-ui——手写字体仅覆盖拉丁字符，中文不强行伪手写。

### 4. 图标自建，不引 lucide-react

24×24 viewBox、strokeWidth 1.8、round caps、`currentColor` 继承的 ~45 枚手绘路径图标（`iconRegistry` + `satisfies` 类型收口）。理由：现成图标库的几何精确感与手绘抖动冲突；自建注册表让替换/补齐零依赖。轻微路径抖动即「抖动即性格」。

### 5. reduced-motion 与打印

`prefers-reduced-motion: reduce` 按具体选择器关停动画与微过渡（**不用 `!important`**，遵守 AGENTS.md §7.5）；`@media print` 去纹理。

## 后果 / Consequences

- 正面：一套 token 同时喂饱 4 主题；对比度由 349 项测试中的 4×28 对 WCAG 配对永久看护；组件样式收敛后新页面直接复用原子。
- 负面：自建图标需自行补齐新 glyph；wobbly radius 是任意值 utility，Tailwind 不会按需去重。
- 缓解：图标注册表单文件收口，`satisfies` 保证缺字编译期报错；新组件优先用原子而非再写 `rounded-[12px_255px...]`。

## 关联

- Phase A–G 提交：`b6d83cd`（地基）→ `30e0ccc`（4 主题+图标）→ `e88a8aa`（图标替换）→ D/G/F/E 阶段（原子库/样板/token 收口/打磨）。
- 引用：ADR 0001（token 唯一来源纪律延续）、`src/styles/themeContrast.test.ts`（视觉回归门禁）。
