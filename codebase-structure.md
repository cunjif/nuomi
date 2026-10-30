# nuomi · 糯米 Codebase 文档

> 📁 本文档为项目总览。nuomi 是一个插件化 Agent Harness：Rust 自研插件内核为心脏，Tauri 2 桌面壳 + React 18 前端为产品表面，Provider 编排、多 Agent 协作、长期记忆与 Self-Evolution。

## 1. 技术栈

- **Node.js ≥ 20**（来自 AGENTS.md 工具链约定；package.json 未显式声明 engines，@types/node ^26.6.5）
- **语言**：TypeScript ~5.5.4（来自 package.json devDependencies，strict 模式）
- **运行时 / 框架**：React 18.3.1 + Vite 5.3.5（来自 package.json dependencies/devDependencies）
- **桌面壳**：Tauri 2（@tauri-apps/api ^2.0.0、@tauri-apps/cli ^2.11.4）
- **后端核心**：Rust edition 2021 + tokio（来自 Cargo.toml workspace）
- **持久化**：SQLite + rusqlite 0.37（WAL，append-only 迁移）
- **IPC 契约**：tauri-specta 生成 TS bindings
- **包管理器**：pnpm（来自 AGENTS.md 工具链）
- **UI 状态**：Zustand 4.5.4（本地）+ TanStack Query 5.51.0（IPC 数据态）
- **样式**：Tailwind CSS 3.4.7（暗色优先）
- **编辑器**：Monaco Editor 0.56.0（@monaco-editor/react ^4.7.0）
- **终端**：xterm 6.0.0（@xterm/xterm）
- **测试**：Vitest 2.0.5（前端）/ cargo test（Rust）
- **Lint / 格式化**：ESLint 9.9.0 + typescript-eslint 8.0.0 / cargo fmt + cargo clippy

## 2. 项目目录结构

> 💡 TS/JS 项目按实际目录层级展开。src/ 前端源码树约 40 行，内联于主文档。

```
nuomi/
├── crates/                                # Rust 核心 workspace（内核优先阶段主战场）
│   ├── nuomi-core/                        # 业务逻辑 lib：领域模型、插件内核、编排引擎、存储、Evolution
│   │   └── src/
│   │       ├── domain/                    # 领域实体 + Run 状态机（单一事实源）
│   │       ├── harness/                   # 插件内核：registry · context · event bus · sideload 第三方插件
│   │       │   └── sideload/              # 第三方插件 NPP 协议加载
│   │       ├── providers/                 # Provider 客户端与 Master-Slave 编排
│   │       ├── orchestrator/              # Role/Team 执行器：Pipeline · Router · 群聊 · WhiteBoard
│   │       ├── adapters/                  # Agent 适配器（cli 等方言）
│   │       ├── store/                     # SQLite repositories + 迁移 runner
│   │       │   └── repos/                 # 各领域仓储实现
│   │       ├── evolution/                 # Self-Evolution：轨迹聚合 · prompt 版本化 · 白名单联网调研
│   │       ├── exchange/                  # 交换/导入导出
│   │       ├── integrations/              # 外部集成（遥测/bot 等）
│   │       ├── plugins/                   # 内建插件
│   │       ├── sandbox/                   # 沙箱执行
│   │       ├── services/                  # 应用服务层（team_runner 等物化执行）
│   │       │   └── presets/              # 预置配置
│   │       └── steward/                   # Steward 工作台相关领域
│   └── nuomi-cli/                         # headless 验收入口 bin：run / resume / REPL / plugin list
├── src-tauri/                             # Tauri 薄壳 bin，依赖 nuomi-core（commands/adapters 桥接）
│   └── src/
│       └── bin/                           # contracts_gen 等 bin 入口
├── src/                                   # React 前端 frontend（UI 里程碑已回归）
│   ├── components/                        # 可复用 UI 组件
│   │   └── ui/                            # 设计系统原子 primitives
│   │       └── Icon/                      # 图标组件
│   ├── features/                          # 按领域组织的功能模块
│   │   ├── approvals/                     # 审批收件箱
│   │   ├── board/                         # 看板
│   │   ├── chat/                          # 聊天
│   │   ├── common/                        # 跨功能公共组件
│   │   ├── conversation/                  # 会话/转录
│   │   │   ├── background/                # 后台会话
│   │   │   ├── composer/                  # 输入编辑器
│   │   │   ├── group/                     # 群聊
│   │   │   ├── new/                       # 新建会话
│   │   │   ├── scheduled/                 # 定时会话
│   │   │   └── sidebar/                   # 会话侧栏
│   │   ├── git/                           # Git 集成
│   │   ├── plugins/                       # 插件管理
│   │   ├── sandbox/                       # 沙箱前端
│   │   ├── scheduler/                     # 定时任务
│   │   ├── settings/                      # 设置（provider 密钥等）
│   │   ├── shell/                         # Shell/工作区管理（工作目录、文件 tab）
│   │   ├── steward/                       # Steward 工作台前端
│   │   ├── terminal/                      # 终端
│   │   └── trace/                         # 追踪/可观测
│   ├── i18n/                              # 国际化
│   │   └── locales/                       # 语言资源
│   ├── lib/                               # 前端基础库
│   │   ├── commands/                      # IPC 命令封装
│   │   ├── conversation/                  # 会话工具
│   │   ├── editor-ext/                    # Monaco 编辑器扩展
│   │   │   ├── builtin/                   # 内建扩展
│   │   │   └── indexer/                   # 索引器
│   │   ├── events/                        # 事件
│   │   ├── format/                        # 格式化
│   │   ├── ipc/                           # 生成的 IPC 绑定（禁止手改）
│   │   ├── perf/                          # 性能
│   │   └── store/                         # zustand stores
│   ├── styles/                            # 全局样式
│   └── test/                              # 测试工具
├── migrations/                            # NNN__name.sql 只增不改 append-only
├── examples/plugins/                      # 第三方插件示例
├── docs/                                  # ADR / SPEC / 插件开发文档
├── package.json                           # 前端依赖与脚本
├── Cargo.toml                             # Rust workspace 根
└── pr.md                                  # 需求权威
```

## 3. 开发命令（核心）

> ⚠️ 所有命令均来源于项目文档，每条命令后注明出处。

### 构建命令
```bash
# 前端构建（来源：package.json > scripts.build）
pnpm build

# Rust 核心构建（来源：AGENTS.md > 6. 开发命令）
cargo build --release -p nuomi-cli

# 桌面打包（来源：AGENTS.md > 6. 开发命令）
pnpm tauri build

# 重新生成 IPC 绑定（来源：package.json > scripts.contracts:gen；改 Rust 类型后必跑）
pnpm contracts:gen
```

### 测试命令
```bash
# 前端单测（来源：package.json > scripts.test）
pnpm test

# Rust 测试（来源：AGENTS.md > 6. 开发命令）
cargo test

# Rust 行覆盖率（来源：package.json > scripts.coverage:rust）
pnpm coverage:rust
```

### 运行 / 启动命令
```bash
# 前端开发服务器（来源：package.json > scripts.dev）
pnpm dev

# 桌面开发模式（来源：AGENTS.md > 6. 开发命令）
pnpm tauri dev
```

### 静态检查
```bash
# 前端类型检查（来源：package.json > scripts.typecheck）
pnpm typecheck

# 前端 lint（来源：package.json > scripts.lint）
pnpm lint

# Rust lint 零警告（来源：AGENTS.md > 6. 开发命令）
cargo clippy --all-targets -- -D warnings

# Rust 格式化（来源：AGENTS.md > 6. 开发命令）
cargo fmt --all
```

## 4. 开发环境（简要）

### 前置要求

- Node.js ≥ 20（推荐使用 nvm 管理版本）
- pnpm（来自 AGENTS.md 工具链）
- Rust toolchain（edition 2021，cargo）
- Linux 额外系统库：libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf
- macOS：Xcode CLT（xcode-select --install）
- Windows：无额外依赖

### 配置说明

- `NUOMI_DB_PATH`：指向 SQLite 库文件，桌面壳与 CLI 共享同一会话库
- `opencode.json`：opencode 项目配置（权限、规则注册）
- `pr.md`：需求权威文档
