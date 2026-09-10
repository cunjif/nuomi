# ADR 0010: 内核插件 × 编辑器扩展贯通（Editor Plugin Bridge）

- 状态：Accepted（2026-09-10）
- 关联：ADR 0009（插件侧载 SPI）、ADR 0001（Rust 插件内核）
- 背景：桌面壳存在两套"插件"——内核侧 NPP 插件（`crates/nuomi-core/src/harness`，贡献 tools/hooks/events）与前端编辑器扩展（`src/lib/editor-ext`，贡献 preview/outline/hover 等）。二者概念同源（Cordis"一切皆插件"），管理入口与启停语义却彼此独立；`registry.ts` 头注释自始预留了"Rust 插件经 IPC 下发扩展描述"的路径。

## 决策

1. **一套系统，两个执行面**。内核插件是能力的生产者，编辑器扩展是能力在编辑器中的消费者之一。统一模型：扩展 = 有 id / 标题 / 启停态 / 贡献的注册项；内核插件经桥接层派生出 id 为 `plugin.<id>.editor` 的 EditorExtension，与本体内置扩展同列同管。

2. **桥接 = NPP v1 增量方法 + manifest 声明，不升协议 major**。NPP 既有前向兼容设计（未知键警告不拒、`api_version <= NPP_API_VERSION`）。新增 `editor/hover`、`editor/symbols`；`editor/command` 由宿主改写为清单声明的 `tools/call`（插件零新增方法即可提供命令）。清单 `[editor]` 段是权限边界：宿主绝不调用未声明的方法。升级 v2 的握手矩阵/文档分裂成本远超收益。

3. **运行时通道：Context 服务 `editor_bridge`**。`SideloadedPlugin::init` 将活跃 NPP 连接与 `[editor]` 贡献注册进 `EditorBridgeRegistry`（qualifier `editor_bridge`，沿用 ADR 0009 §6 服务约定，不公开 `SideloadedPlugin` 私有字段）。Tauri 命令 `plugin_editor_call` 经 `state.kernel.context()` 解析服务转发。kernel 未 boot / 插件未加载返回结构化错误（`kernel_not_ready` / `plugin_not_loaded`），前端 provider 降级为 null——编辑器永不因插件故障卡死。

4. **启停状态统一入 SQLite `app_settings`**。单一 JSON blob（键 `editorext.enabled.`），IPC `app_setting_get/set` 读写；读路径经内存缓存保持同步语义（Monaco provider 过滤是同步的）。旧 localStorage 键迁移一次并清除，IPC 不可用时回落 localStorage。启停只依赖扩展 id 字符串，与 `plugin_list` 的磁盘扫描天然解耦。

5. **管理面：单一扩展中心**。PluginsView 升级为统一视图：内置扩展与内核插件同列（类型徽章），统一启停；插件行内开关仅控制其编辑器贡献（派生扩展），插件本体的安装/卸载维持"下次 boot 生效"既有语义——不引入运行态杀进程的复杂度。编辑器工具栏 ExtensionsPanel 保留为快捷入口。

6. **iframe overlay 的安全边界**。manifest 校验层强制 URL 仅 `https://` 或 loopback `http://`；渲染层 `sandbox="allow-scripts"`（无 `allow-same-origin`、无 `allow-top-navigation`）。插件进程无法提供 React 组件，iframe 是 v1 唯一的 UI 贡献形态。

## 权衡

- **符号提供者同步缓存**：编辑器 outline 管线为同步设计（workspace 索引、MonacoTab useMemo）。插件符号 RPC 异步，桥接层以"同步缓存 + 后台刷新 + 版本通知"适配——首轮空、落地后重渲染，索引管线保持本地速度不被插件拖慢。
- **编辑器贡献启停 ≠ 插件启停**：粒度更细且避免运行态管理；代价是用户需要理解两层开关（行内提示文案说明）。
- **已 flush 的 Monaco provider 无法反注册**：卸载插件后派生扩展即刻从读侧消失，但已注册进 Monaco 命名空间的 provider 至重载前仍存活——其调用失败被 provider 捕获并返回 null，无用户可见错误。

## 后置项（roadmap）

- iframe 消息协议（plugin→iframe postMessage）与 overlay 生命周期事件
- 权限强制执行（[editor] 纳入 permissions 模型）
- WASM 载体插件直接进程内提供编辑器扩展
