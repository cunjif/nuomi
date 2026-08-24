# Rules — TypeScript / React 前端

> 适用范围 Scope: `src/**`。与 AGENTS.md 冲突时以 AGENTS.md 为准。
> Applies to `src/**`. AGENTS.md wins on conflict.

## 类型 / Types
- `strict: true` 永远开启；禁止 `any`、`as any`、`@ts-ignore`、`@ts-expect-error`。No any or suppression directives — use `unknown` + narrowing.
- 跨边界数据（IPC payload）类型只从 `lib/ipc/bindings.gen.ts` 导入，不手抄。IPC payload types come ONLY from generated bindings.
- 优先 `type` 别名 + 判别联合 (discriminated union)；`interface` 仅用于可被扩展的对象契约。

## 组件 / Components
- 只用函数组件；props 用显式 `type XxxProps` 并导出。Function components only, exported props type.
- 单文件 > 200 行必须拆分（子组件 / hooks / utils）。Split anything over ~200 LOC.
- 禁止在 render 中写业务逻辑；副作用一律进 hooks 或事件回调。
- 条件渲染早返回；列表渲染必须有稳定 `key`（领域 id，不用 index）。

## 数据流 / Data flow
- 服务端数据（一切来自 Rust 核心的数据）走 **TanStack Query**；queryKey 约定：`['tasks']`、`['runs', taskId]`、`['agents']`，mutation 成功后精确 invalidate。All IPC data via TanStack Query with conventional queryKeys; invalidate precisely after mutations.
- **Zustand** 只存易失 UI 态（选中项、面板开合、过滤器），禁止把 IPC 数据复制进 store 形成第二事实源。Zustand is ephemeral-UI-only; never duplicate IPC data into it.
- 流式日志/事件：经 `useDomainEvents()` 订阅唯一事件通道；高频更新用 rAF 批量 flush；长列表用 `@tanstack/react-virtual` 虚拟化。Stream via the single domain-event hook; batch high-frequency updates; virtualize long lists.

## 样式 / Styling
- Tailwind 工具类；颜色/间距只用 theme token，不写魔法值。Theme tokens only — no magic color values.
- 暗色优先设计，亮色主题必须同样可用。Dark-first, but light must work.

## i18n
- 所有用户可见文案经 i18n 资源（默认 `zh-CN`，同键 `en`），组件内禁止硬编码文案。All user-facing strings through i18n resources (zh-CN default, en parallel); no hardcoded strings.

## 异步表面三态 / Async surface triad
- 每个异步区域必须有 loading、empty、error 三态 UI；action 失败用 toast 反馈并保留用户输入。Every async surface renders loading/empty/error; failed actions toast and preserve input.
- 路由级挂 ErrorBoundary。

## 可访问性 / Accessibility
- 语义化标签；交互元素键盘可达；焦点环不得移除；拖拽操作必须提供等价按钮/菜单（看板卡片尤其）。Semantic HTML, keyboard reachable, visible focus rings; drag interactions need button/menu equivalents (kanban cards included).

## 验证 / Verification（完成前必跑 run before claiming done）
```bash
pnpm typecheck && pnpm lint && pnpm test
```
