# nuomi 部署文档 / Deployment Guide

> 版本：v0.2.0（Role 能力体系 + Harness Journal + 编辑器插件化）。更新：2026-09-06。

## 1. 交付形态 / Deliverables

| 形态 | 产物 | 说明 |
|---|---|---|
| 桌面应用 | `nuomi_0.2.0_x64-setup.exe`（NSIS）/ `.msi` | Tauri 2 打包，Windows 验证；macOS(.dmg/.app 未签名)、Linux(deb/rpm/AppImage) 由 CI 矩阵产出 |
| CLI | `nuomi-cli-{win64\|macos-arm64\|macos-x64\|linux-x64}` | headless run/resume/REPL，与桌面共享会话库 |
| 源码构建 | `pnpm tauri build` / `cargo build --release -p nuomi-cli` | 见 §3 |

## 2. 环境要求 / Prerequisites

- **桌面运行时**：无外部依赖（WebView2 随 Windows 分发；首次安装包会引导安装缺失组件）
- **开发/源码构建**：Node ≥ 20 + pnpm ≥ 9 + Rust stable（MSVC/Linux GNU/Apple ClT）
- **Linux 构建额外系统库**：`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf`
- **macOS 构建**：`xcode-select --install`

## 3. 构建与发布 / Build & Release

```bash
pnpm install                 # 安装前端依赖
pnpm tauri build             # 桌面双格式安装包 → target/release/bundle/{msi,nsis}/
cargo build --release -p nuomi-cli   # CLI 二进制 → target/release/nuomi-cli(.exe)
```

- **CI 入口**：`.github/workflows/release.yml`——push tag `v*` 或手动触发；三平台矩阵 + 契约 drift 校验 job（`pnpm contracts:check` 必须零 diff）
- **发版步骤**：①改 `src-tauri/tauri.conf.json` 与 workspace `Cargo.toml` 版本号 → ②`pnpm contracts:gen` 确认 bindings 无 drift → ③全量门禁（§5）→ ④打 tag 推送 → ⑤CI artifacts 即分发包
- macOS 产物为**未签名**包（个人使用右键打开；分发需自行配置 Apple 签名/公证）

## 4. 数据与目录布局 / Data Layout

| 路径（Windows 示例） | 内容 |
|---|---|
| `%APPDATA%\dev.nuomi.shell\nuomi.db` | 会话/事件/Provider/Role/Team 等全部状态（SQLite WAL） |
| `%APPDATA%\dev.nuomi.shell\sandbox\` | 默认工作区（首启可改） |
| `%APPDATA%\dev.nuomi.shell\evolution-journal\` | Harness Journal 段文件（演进审计） |
| `%APPDATA%\dev.nuomi.shell\exchange\` | Exchange FS 段文件与 blob（工具调用审计） |
| OS Keyring | API Key（不进数据库/日志） |

- CLI 与桌面共享数据：设 `NUOMI_DB_PATH` 指向同一 `nuomi.db`
- 升级：直接覆盖安装；SQLite 迁移只增不改，旧库自动升级（不可降级到旧版本）
- 卸载：安装包卸载不删除 `%APPDATA%\dev.nuomi.shell`（用户数据保留，手动删除即彻底清理）

## 5. 发布前门禁 / Release Gates（必须全绿）

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck && pnpm lint && pnpm test
pnpm contracts:gen && git diff --exit-code -- src/lib/ipc   # 契约 drift 校验
```

## 6. 本版本新能力配置要点 / New Capabilities Notes

- **Role 能力体系**：首启自动 seed 11 个内置角色（含「角色导演」）；` RolesSection` 可恢复预置/自然语言生成角色；Role 绑定 Provider 时校验能力覆盖（Reasoning/Image/Voice/Video），不匹配提示缺失清单
- **能力路由**：`RoutingRules`（prefer_local / capability_overrides）存 `app_settings` kv；主 Role 能力不足时自动路由到匹配 Role，无 Role 时按 Provider 创建临时（ephemeral）Role，run 结束 GC
- **Harness Journal**：`%APPDATA%\...\evolution-journal` 段文件 + `journal` 合成会话镜像进 events 表；Trace 页双 Tab（Trace / Journal），Applied 条目可时间旅行回滚；漂移检测阈值默认（24h 内 Applied≥10 且回滚率≥30% 或 diff>20k 字符）
- **编辑器扩展**：`src/lib/editor-ext/` 插件框架（对齐 Cordis 理念），内置 7 扩展（符号大纲/MD 双栏预览/图片/PDF/Office 信息面板/hover 文档/代码库索引挂载点）；`Alt+H` 切对话面板、`Alt+E` 切编辑面板（capture 阶段锁定，不可被覆盖）
