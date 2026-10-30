# ADR 0025: 沙箱入口重定位与个性化代码自进化（K9）

- **状态**: Accepted（2026-10-30 用户确认启动实施）
- **日期**: 2026-10-30
- **修订日期**: 2026-10-30（v1.1 — 补充 M1~M7 遗漏决策、O1~O5 优化、E1~E7 增强）
- **决策者**: user + conductor
- **前置设计**: ADR-0021（工作区两级模型）、ADR-0022（插件化代码沙箱）、ADR-0024（工作区内置终端）、`steward_workbench_v2` design §2.1.3.4 / §2.3.2.2、Hermes-Agent 轨迹沉淀范式
- **依赖**: ADR-0021、ADR-0022、ADR-0009（NPP 侧载，L1 演进目录对接）
- **修订**: ADR-0022 §3（入口假设）、ADR-0009 §4（新增 evolved 侧载层）、AGENTS.md §9（路线图追加 K9）

---

## 背景与范式转移

### 1. 现状痛点与入口失配
当前代码运行沙箱以"手抄 snippet"为主入口：前端通过 `SandboxCodeEditor`（`src/features/sandbox/SandboxCodeEditor.tsx`）与独立沙箱页（`SandboxView.tsx`，newRun/newBuild 对话框）组织。然而：
1. 开发者在日常编码中需要的是直接运行当前工程或文件的"工作台原生内生执行"，手抄代码片段模式与工作流严重割裂；
2. 后端 `sandbox_build_app`（`commands.rs:10870`）已实现将 `authorized_dirs` 自动收敛至 active workspace 的 mounted dirs——即"以当前工作台挂载目录为构建目标"的能力已在内核就绪，前端入口与交互模型却未对齐；
3. 沙箱能力长期停留于一过性执行器工具，未能与 Harness 的长期演进体系形成闭环。

### 2. 范式转移一：沙箱定位重塑
彻底废弃低频手抄代码入口，将沙箱能力收敛并服务于两个正交场景：
- **场景 A（工作台原生沙箱执行 Workbench-native In-situ Execution）**：在工作台编辑器中直接运行当前文件或整个项目（类似 VSCode 的 Run / Debug），但执行目标环境完全被封禁在隔离沙箱而非物理宿主。代码源直接映射为当前工作区挂载目录。
- **场景 B（目标导向的闭环自进化 Goal-oriented Self-evolution·忒修斯之船）**：自进化 Agent 对 Harness 自身进行修改（覆盖插件、内核逻辑、前端视图、样式主题、UX 交互与快捷键），修改后的版本在沙箱中启动运行并接受全面评估；评估优于当前版本且契合用户预期后，通过原子替换更新宿主。逐板替换，直至整艘船被重构，但其本质仍是 nuomi。

### 3. 范式转移二：自进化终态（Telos）的重定义
传统的自进化往往陷入缺乏目标函数的“通用盲目优化”。ADR 0025 明确场景 B 的终极价值：
> **自进化的方向不是抽象的“通用更强”，而是朝特定用户的使用习惯与行为偏好收敛，终态为“高度契合用户个人的专属 Harness”。**

这把自进化从无方向的随机突变转变为有方向的定向拟合。场景 B 进一步分解为两层递进机制：
- **B1 用户层 self-evolution**（对话内·轻量·高频·资产沉淀）：生效于 RoleAgent 与管家 Steward 对话。检测到有效轨迹时，弹窗询问用户是否沉淀为 Skill、规约或 Memory，**不碰应用代码**。
- **B2 项目层 self-evolution**（整个 Harness·重量·低频·代码替换·忒修斯之船）：当 B1 资产积累产生结构性诉求、或必须修改 Harness 组件才能满足，或用户配置的联网学习在白名单站点发现有益更新时，启动对 Harness 自身代码的修改、沙箱评估与宿主替换。

### 4. 物理约束与现实断层分析
在基于 Tauri 2（Rust 宿主 + 系统 WebView）的跨平台桌面应用架构下落地代码自进化，存在以下必须攻克的物理与工程现实约束：
1. **OS 进程文件锁排他性**：在 Windows 等主流操作系统上，正在运行的 `.exe` 二进制文件受到操作系统内核强制排他写锁定（`ERROR_SHARING_VIOLATION`），宿主绝对无法原地自我覆盖写入；
2. **沙箱构建工具链与资源配额断层**：编译 Rust 桌面宿主依赖庞大的工具链（Cargo、rustc、C/C++ 编译器、平台 SDK）及多 GB 的缓存（`target/` 动辄 2~5 GB），耗时远超沙箱常规运行限额（ADR-0022 默认超时仅 120s），轻量脚本容器无法直接承担全量编译；
3. **启动崩溃死锁（Crash Loop）与回滚盲区**：沙箱测试通过不能保证在真实宿主环境（真实 OS Keyring、物理显卡、杀毒软件拦截）100% 成功启动。若替换后启动即 panic，UI 无法渲染且进化引擎随之挂死，用户陷入无法通过界面回滚的完全瘫痪状态；
4. **数据库 Schema 单向演进与代码回滚的向前兼容冲突**：系统遵从 migrations 只增不改铁律，若新版本执行了新 SQL 迁移后发生回滚，旧代码可能无法兼容新数据结构；
5. **B1 轨迹捕获的时机抖动与弹窗疲劳**：单纯基于对话“空闲”检测会导致高频 Token 浪费与打扰性弹窗轰炸；
6. **防御纵深缺失**：外部联网学习可能引入恶意 Prompt 注入或代码投毒，必须建立多重安全隔离；
7. **POSIX 平台文件替换原子性**（新增）：macOS / Linux 的 `rename()` 跨文件系统不原子；macOS `.app` bundle 替换需额外处理 `Contents/MacOS/` 目录结构；
8. **二进制接力链信任根缺失**（新增）：接力启动器执行文件换名前若不验证新产物数字签名，攻击者一旦获得 `nuomi-selfeval-{id}/` 目录写入权限即可植入恶意二进制；
9. **Trampoline 版本绑定脆弱性**（新增）：接力启动器是独立二进制，协议升级后旧版 Trampoline 与新版宿主可能握手失败，且 Trampoline 本身无法自我替换；
10. **L2 热重载的 CSP 安全边界**（新增）：Tauri 2 默认 CSP 拦截动态加载的非授权脚本源，演进代码若引入外部依赖会静默失效，且热重载触发 IPC 命令未定义。

---

## 决策

### 设计决策全览

| 模块 | # | 决策点 | 选择 | 关键理由与机制 |
|---|---|---|---|---|
| **场景与入口** | 1 | 场景划分与 C 去留 | **保留 A 与 B（B1/B2），彻底废除场景 C** | 删除 `SandboxCodeEditor`，沙箱页降为只读审计，`sandbox_run_code` 降为 Agent 内部探针能力 |
| | 2 | 入口重定位与交付 | **A 入口直达工作台编辑器，B 内部驱动** | A 支持右键/工具栏启动，流式输出至终端/输出面板；B2 内部静默驱动 |
| **B1 用户层** | 3 | 生效范围泛化 | **覆盖 RoleAgent 与管家 Steward 对话** | 打破当前 `evolution/` 仅管家专属的限制，全量捕获人机协作偏好 |
| | 4 | 防抖触发与分层沉淀 | **高置信度事件防抖触发 + 三级资产归属** | 仅在纠错/显式指令/复杂任务完成后判定；规约进全局，Skill 进工作区，Memory 进用户库；系统铁律绝对优先 |
| **B2 梯次演进** | 5 | 梯次分级模型（Tiered） | **L0 软偏好 → L1 插件热装配 → L2 前端热更 → L3 内核编译** | 拒绝一揽子重编译，将 90% 的自进化控制在免编译的轻量安全区间 |
| | 6 | 沙箱规格分级与临时目录 | **Profile 分离 + 第四层 `nuomi-selfeval-{id}`** | 区分轻量执行与全量构建 Profile；评估归档后强制执行 Build Cache GC 抹除 `target/` |
| **评估与验证** | 7 | 双轨制评估体系 | **硬指标（100% 测试通过）+ 软指标（`better_for_user` 偏好审查）** | 杜绝大模型审查的幻觉与喜新厌旧；辅助以历史典型任务**影子轨迹回放（Shadow Replay）** |
| | 8 | 外部输入防御纵深 | **规范提取 + AST 静态扫描 + 强审批门禁** | 严禁联网抓取代码直通 patch；拦截敏感系统调用与未受控网络 Socket；无条件挂起为 `awaiting_approval` |
| **可靠性与替换**| 9 | 进程接力与文件锁解除 | **独立轻量接力启动器（Trampoline Launcher）** | 宿主安全退出释放句柄，接力进程执行换名原子替换并拉起新版，彻底兼容 Windows 文件锁 |
| | 10 | 启动看门狗与版本快照树 | **2 次崩溃自动回退 Safe Mode + 快照时光穿梭** | 启动计数标记，崩溃自愈；快照链支持任意历史版本一键回滚；Schema 必须向前兼容 |
| | 11 | 自举豁免与防篡改 | **路径黑名单 + 构建配置哈希校验** | 豁免自身评估与迁移引擎；哈希校验 `Cargo.lock` / `build.rs` 防止间接依赖投毒 |
| **运行态与治理**| 12 | CLI 急救与状态机铁律 | **新增 CLI 应急回滚命令 + 事件先于副作用** | 桌面宿主启动崩溃时 CLI 可救援；B2 状态流转 100% 遵守 Run 状态机落库铁律 |
| | 13 | 架构分离与偏好看板 | **共享内核 vs 个性化层 + Evolution Compass UI** | 保留上游升级能力；Settings 提供偏好看板（画像透明/可纠偏）与单模块一键出厂重置 |
| | 14 | 路线图立项 | **独立立项为 K9 里程碑** | K8（prompt 进化）保持不变，K9 专攻代码级个性化自进化 |
| **B1 增强** | 15 | B1 资产撤回（E1） | **30 秒软撤回 Toast + `evolution.asset_revoked` 事件** | 用户确认后仍可反悔，append-only 兼容 |
| | 16 | B1/K8 接口边界（M1） | **`user_preference_profiles` 为单一画像源，K8 通过专用 Service 接口读取** | 杜绝两套并行偏好源；K8 的 PromptVersion 状态机消费同一画像 |
| | 17 | 防抖参数动态化（O2） | **基于 Session 活跃度动态调整防抖/冷却阈值，Compass 提供用户滑块** | 静态参数在高频/低频 Session 下均会失配 |
| **B2 增强** | 18 | L1 演进目录与 ADR-0009 对接（E4） | **专用第四层侧载目录 `.nuomi/plugins/evolved/`，`plugin.toml` 增加 `evolution_snapshot_id`** | 演进产出插件与手动安装插件精准区分，支持精准回滚 |
| | 19 | L2 热重载 IPC 与 CSP（M3） | **新增 `evolution_reload_frontend` IPC 命令；前端 bundle 验证后动态更新 CSP 白名单** | Tauri 2 默认 CSP 会静默拦截演进代码的外部依赖 |
| | 20 | GC 策略两阶段化（O3） | **评估失败保留 `target/` 缓存 24h TTL；成功替换后才全清** | 避免重试演进因缓存失效导致编译时间膨胀 |
| | 21 | Dry-Run 预演模式（E3） | **新增 `evolution_dry_run_preview` IPC，仅生成 Diff + 影响范围，不编译不替换** | 低成本感知演进方向，与 AGENTS.md dry-run 候选增强对齐 |
| | 22 | 演进自动授权分级（M6） | **L0/L1 可配置自动授权；L2 默认询问；L3 与联网来源强制 `awaiting_approval`** | 区分演进风险等级，高级用户可提升自动化程度 |
| | 23 | 人工触发演进入口（E5） | **Compass UI 与 REPL 新增 `nuomi evolution trigger --tier <L>` 命令** | 高级用户主动发起演进，不依赖系统积累触发 |
| | 24 | Bot 通知集成（E6） | **`succeeded` / `failed` / `crash_rollback` 三态触发 M-BOT1 Telemetry sink** | 复用已交付飞书 webhook，高影响事件主动推送 |
| **评估增强** | 25 | 影子回放数量可配置（O1） | **`evolution.shadow_replay_count` 默认 3，L3 强制 ≥ 5；冷启动降级为仅第一轨** | 冷启动 / 成熟用户场景各异，硬编码 3 条失配 |
| | 26 | 退化量化阈值（O5） | **任意核心指标退化 > 15% 视为软拒绝，阈值可在 Compass 调节** | 无量化基准则影子回放结论无法机械判定 |
| | 27 | UX 锚点检查第四轨（E7） | **L2/L3 演进追加 E2E 探针校验 `user_preference_profiles` 高置信度 UI 锚点** | 防止 UX 语义倒退——自动化测试感知不到快捷键被改 |
| | 28 | Compass 演进预测（E2） | **基于累积画像由 LLM 生成下一步演进方向摘要** | 从纯历史视图升级为前瞻感知 |
| **可靠性增强** | 29 | Trampoline 信任根（M2） | **文件换名前验证 Authenticode / codesign 签名；Trampoline 自身哈希固化在宿主内** | 防止 selfeval 目录被写入攻击 |
| | 30 | POSIX 原子替换（M4） | **同文件系统内用 `rename()`；macOS `.app` 替换 `Contents/MacOS/` 目录结构；跨 fs 拒绝** | POSIX 跨文件系统替换不原子，macOS bundle 结构特殊 |
| | 31 | Trampoline 版本冻结（M7） | **Trampoline 职责冻结为「等待退出→换名→拉起」，接口永远向后兼容，不承担版本协商** | 最简 Trampoline 无需自更新，消除版本绑定脆弱性 |
| **数据面** | 32 | 快照父子链字段（M5） | **`evolution_snapshots` 增加 `parent_snapshot_id` 外键** | 支持版本树拓扑，发布 migration 后再加代价陡增 |
| | 33 | profile_summary 强类型（O4） | **`ProfileSummary` Rust serde struct 为单一定义，migration 注释引用类型路径** | 防止非结构化 JSON 在 UI 解析时意外失败 |

---

## 详细架构与机制设计

### 1. 场景 A：工作台原生沙箱执行（Workbench-native Execution）

#### 1.1 交互入口与代码源收敛
- **入口**：工作台文件树/编辑器右键菜单新增 `"Run in Sandbox"`；编辑器顶部操作栏新增 `"Run Project in Sandbox"`。
- **路径解析**：复用 `sandbox_build_app` 的目录收敛契约（`commands.rs:10870`），代码源直接绑定为当前活动工作区的挂载目录集合，禁止跨越到工作区外部。
- **任务形态支持**：
  - **一次性脚本**：直接运行 Python、Node、Rust 单文件脚本，结果流式返回；
  - **交互式程序（Interactive CLI）**：双向标准输入输出流对接工作区内置终端（复用 ADR-0024 的 portable-pty 与 xterm.js 管道通道）；
  - **长驻 Web 服务（Dev Server）**：针对前端 Vite 开发服务器或后端 API，沙箱运行时负责端口转发，并在宿主本地绑定代理端口（Localhost Proxy），供工作台 WebView 实时预览。

#### 1.2 场景 C 彻底退场
- 前端删除 `src/features/sandbox/SandboxCodeEditor.tsx`，彻底移除手动粘贴代码运行界面；
- 独立沙箱页面 `SandboxView.tsx` 移除 `New Run` 和 `New Build` 弹窗入口，降级为**全局沙箱任务审计看板**（专注于展示在途与历史任务状态、资源用量、标准输出与产物审计）；
- 后端 IPC 命令 `sandbox_run_code` 降级为系统内部能力，仅供自进化 Agent 运行代码探针或 REPL 评估时调用，不再作为用户界面入口暴露。

---

### 2. B1 用户层自进化：轻量软偏好与资产沉淀

#### 2.1 触发机制与防打扰控制（Anti-Dialog Fatigue）
为防止频繁调用大模型造成的 Token 浪费以及无休止的确认弹窗破坏沉浸式人机协作体验，B1 实施严格的防抖与事件门禁：
1. **静默观察**：对话进行中不打断用户，后台仅记录轨迹特征；
2. **高置信度触发门槛**：仅在满足以下关键条件之一时触发沉淀判定：
   - 用户发出显式的肯定或纠错指令（例如：“以后写 Rust 不要用 unwrap”、“这个组件排版很好，记下来”）；
   - 包含多轮工具调用的复杂任务执行成功并由用户完成验收确认；
   - 连续交互超过阈值且对话进入空闲窗口（防抖等待时间设定为 60 秒）；
3. **预算控制（Budget & Cooldown Gate）**：设立每日静默判定 Token 消耗配额，同一会话的两次沉淀提议之间强制冷却 10 分钟；
4. **动态防抖调节**：空闲防抖窗口（默认 60 秒）与强制冷却间隔（默认 10 分钟）依据 Session 活跃度自适应调整——高频工具调用 Session 中阈值提高、低频碎片 Session 中阈值降低；两个参数同时在 Evolution Compass 面板提供用户可调节滑块。

#### 2.2 资产分级沉淀与规则继承链
检测到有效偏好与知识沉淀时，弹窗征得用户确认后，按严格的作用域分类入库：
- **规约（Conventions）**：沉淀为跨项目通用的用户行为准则，持久化写入用户全局目录 `~/.nuomi/rules/user-preferences.md`；
- **长期记忆（Memory）**：提取用户偏好知识事实，打标签持久化写入系统长期记忆库（SQLite `memory_entries`）；
- **技能（Skill）**：与特定工程技术栈紧密相关的操作流与脚本，写入当前活动工作区私有技能目录 `.nuomi/skills/<skill-name>/`。

```
┌────────────────────────────────────────────────────────┐
│                   规则生效优先级继承链                   │
├────────────────────────────────────────────────────────┤
│ Layer 1: 系统底层硬规则 (AGENTS.md §7) —— 绝对最高不可动  │
│          (密钥安全 / Migrations只增不改 / 沙箱权限边界)   │
├────────────────────────────────────────────────────────┤
│ Layer 2: 用户全局偏好规约 (~/.nuomi/rules/user-preferences)│
├────────────────────────────────────────────────────────┤
│ Layer 3: 项目级工作区规约 (workspace/.nuomi/rules/)    │
└────────────────────────────────────────────────────────┘
```
> [!IMPORTANT]
> **安全继承铁律**：B1 沉淀出的规约与技能拥有高优先级个性化建议权，但**绝对无权覆盖或放宽 Layer 1 的系统底层安全红线**。

#### 2.3 沉淀软撤回（Undo）机制
弹窗确认后，系统在 UI 底部展示 **30 秒软撤回 Toast**。用户点击"撤回"时：
- 在事件日志中追加 `evolution.asset_revoked`（记录被撤销资产类型、key、来源会话 id），**保持 append-only 特性**；
- 对应规约条目标记为 `revoked`（通过 Memory/Rule 层逻辑删除实现，不物理删除历史记录）；
- Toast 超时（30 秒）或用户主动关闭后，撤回窗口关闭，资产状态固化为 `confirmed`。

#### 2.4 B1 与 K8（Prompt 自进化）的接口边界
B1 和 K8 共享同一偏好画像源，避免产生两套平行的偏好存储：
- **单一画像源**：`user_preference_profiles` 表是全局唯一的偏好事实源；B1 写入，K8 与 B2 均从该表读取；
- **访问契约**：K8 的 `PromptVersion` 反思引擎通过 `evolution::PreferenceService::snapshot()` 接口（而非直接 SQL 查询）获取偏好快照，实现松耦合；
- **写入权限**：仅 B1 沉淀流程和用户 Compass UI 手动修正可写入；K8 只读、B2 只读。

---

### 3. B2 项目层自进化：梯次分层演进模型（Tiered Evolution）

打破“自进化必须全量编译整个桌面应用”的沉重误区。将 B2 划分为梯次清晰的四级演进模型，实现**“低风险轻量演进高频发生，高风险原生重构极度收敛”**：

```
┌───────────────────────────────────────────────────────────────────────────┐
│                       B2 梯次分层演进模型 (Tiered Evolution)              │
├─────────┬──────────────────────┬──────────────────────┬───────────────────┤
│ 等级    │ 作用域               │ 机制与载体           │ 风险与重载成本    │
├─────────┼──────────────────────┼──────────────────────┼───────────────────┤
│ **L0**  │ 软偏好与配置参数     │ 数据/配置更新        │ 零风险，即时生效  │
│         │ (快捷键/主题Token/UI)│ SQLite/配置文件直写  │ (无需重启)        │
├─────────┼──────────────────────┼──────────────────────┼───────────────────┤
│ **L1**  │ 扩展与工具能力       │ 插件热装配           │ 极低风险，免编译  │
│         │ (新适配器/工具/Agent)│ NPP第三方/内置插件Sideload│ (运行时热插拔)   │
├─────────┼──────────────────────┼──────────────────────┼───────────────────┤
│ **L2**  │ 界面结构与交互体验   │ 前端资源热重载       │ 中低风险，秒级生效│
│         │ (React组件/布局/CSS) │ 沙箱打包静态 Dist    │ (WebView 热重载)  │
├─────────┼──────────────────────┼──────────────────────┼───────────────────┤
│ **L3**  │ 原生底层与运行时     │ 二进制全量编译       │ 高风险，接力替换  │
│         │ (Tokio内核/存储/协议)│ 沙箱 Heavy 构建 + 接力│ (宿主优雅重启)    │
└─────────┴──────────────────────┴──────────────────────┴───────────────────┘
```

- **90% 的自进化收敛于 L0~L2**：
  - 新增一个外部 API 调研工具或 CLI 适配器，自进化引擎将其产出为标准 NPP 插件（遵循 ADR-0009 规范）并写入专用第四层侧载目录 `.nuomi/plugins/evolved/`（修订 ADR-0009 §4 扫描链，新增该层位于 `.nuomi/plugins` 之后）；`plugin.toml` 增加 `evolution_snapshot_id` 字段，用于追踪产出快照、精准回滚；通过动态加载生效；
  - 调整面板拖拽宽度、优化深色模式对比度或新增侧边栏按钮，沙箱仅执行 `pnpm build` 产出前端静态 bundle 并通知 Tauri 宿主重新挂载前端，完全规避漫长且危险的 Rust 全量构建。
- **L3 原生重构触发严格收口**：仅当必须变更底层核心通信协议、调整 Rust 数据结构或扩展不可绕过的原生系统能力时，才启动 L3 流程。

---

### 4. 沙箱执行规格分级与临时工作目录 GC

#### 4.1 执行规格分离（Profile Separation）
为应对轻量脚本执行与重型桌面构建的巨大资源差异，修订 ADR-0022，在沙箱内核中支持两种构建规格：
1. **`Profile-Light`（场景 A 及 L0~L2 轻量演进）**：
   - 默认超时 120 秒，内存上限 2GB，使用标准隔离执行器；
2. **`Profile-Heavy`（L3 原生全量编译）**：
   - 专属于自进化引擎，超时放宽至 1800 秒，内存放宽至 8GB；
   - **工具链缓存挂载**：沙箱只读挂载宿主的 Cargo Registry 缓存与 `sccache` 共享缓存目录，避免在沙箱内重复从网络拉取依赖，将全量构建时间从 20 分钟压缩至 2 分钟以内。

#### 4.2 第四层应用级临时目录生命周期与 GC
- **命名与结构**：正式设立系统第四层文件分层——`nuomi-selfeval-{id}`，与现有的附件目录 `nuomi-att-{id}`、插件安装暂存目录 `nuomi-plugin-install-{id}` 统一归为应用级 Ephemeral 目录，不归属任何用户工作区；
- **构建缓存垃圾回收（Build Cache GC）**：
  - 代码在 `nuomi-selfeval-{id}` 内克隆副本、修改、编译并运行测试；
  - GC 采用**两阶段策略**以平衡磁盘占用与重试编译效率：
    - **第一阶段（评估结束即触发）**：清除临时克隆的源码目录与 `node_modules/.cache/`；若评估成功，同时清除 `target/`；若评估失败（含影子回放失败），**保留 `target/` 缓存 24 小时（可配置 TTL）**，供重试时增量编译复用，避免重新耗费 20 分钟全量构建；
    - **第二阶段（演进成功完成宿主替换后）**：强制物理删除所有剩余中间产物 `target/`，仅保留精简二进制、前端构建产物及 Diff Patch 归档至 `.nuomi/snapshots/`；
  - 系统对临时目录总体积设置硬性磁盘配额（Disk Quota，默认上限 10GB），超限时按 LRU 策略强制回收早期失败残留。

---

### 5. 双轨判定体系与影子任务回放评估

判定公式不再依赖单一维度的模糊评判，而是建立客观与主观相结合的判定闭环：
$$\text{Verdict} = \text{ObjectiveTestSuitePass}(\text{new}) \land \text{better\_for\_user}(\text{new}, \text{old}, \text{user\_profile})$$

```
                  ┌────────────────────────────────────────┐
                  │          沙箱构建产物准备就绪           │
                  └───────────────────┬────────────────────┘
                                      │
                   [第一轨: 客观测验硬基线 (红线拦截)]
                                      ▼
                 ┌──────────────────────────────────────────┐
                 │  执行全量自动化回归验证测试套件:            │
                 │  - cargo test (Rust 后端单测/集成测试)    │
                 │  - pnpm test (Vitest 前端单元测试)       │
                 │  - pnpm typecheck (严格类型检查)          │
                 │  - cargo clippy -- -D warnings (零警告)  │
                 └────────────────────┬─────────────────────┘
                                      │
                      ┌───────────────┴───────────────┐
                 [测试未通过]                    [100% 绿灯]
                      ▼                               ▼
            ┌──────────────────┐     [第二轨: 影子任务与主观偏好审查]
            │ 自动拒绝, 中止演进 │                      ▼
            │ 归档失败日志     │    ┌────────────────────────────────────────┐
            └──────────────────┘    │ 1. 影子轨迹无头回放 (Shadow Replay):     │
                                    │    在新版实例中回放 3 组历史 Golden 任务   │
                                    │    量化对比: 耗时 / Token / 成功率     │
                                    │ 2. LlmReviewGate 拟合度评判:           │
                                    │    结合 user_profile 评估用户偏好吻合度 │
                                    └───────────────────┬────────────────────┘
                                                        │
                                                        ▼
                                             ┌──────────────────────┐
                                             │ 发起审批并准备接力替换 │
                                             └──────────────────────┘
```

1. **第一轨：客观测验硬基线（Zero Regression Baseline）**
   - 新版本必须在沙箱中 100% 通过现有全量自动化测试套件；任何单一单测断言失败、类型报错或 Clippy 警告即刻一票否决，坚决杜绝代码逻辑与架构契约的退化。
2. **第二轨：影子任务回放评估（Shadow Trajectory Replay）**
   - 筛选历史真实成功的典型人机任务（Golden Benchmarks）进行无头重放，样本数量由 `evolution.shadow_replay_count` 配置（**默认 3，L3 演进强制 ≥ 5**）；冷启动（可用轨迹不足最低数量）时，第二轨降级跳过，仅依赖第一轨客观基线，Compass UI 显示"样本不足"警告；
   - 量化对比指标：**任务完成耗时、LLM Token 消耗量、工具调用准确率**；退化判定采用**量化阈值**：任意核心指标较基准（最近 10 次同类任务中位数）退化 **> 15%** 视为软拒绝，需用户在审批页显式强制确认方可继续；阈值可在 Compass 调节（范围 5%~50%）；
   - 基准值来源：从 `evolution_shadow_benchmarks` 视图（基于 `event_records` 聚合）取最近 10 次同类任务的中位数，自动更新无需手动维护。
3. **第三轨：基于画像的主观偏好审查**
   - 复用 `crates/nuomi-core/src/evolution/review.rs` 中的 `LlmReviewGate`，将偏好模型聚合提炼的 `user_profile`、代码变更 Diff 以及影子回放数据注入评审上下文，完成偏好契合度最终判定。
4. **第四轨：用户习惯锚点检查（UX Anchor Check，仅 L2/L3）**
   - 在新版前端 bundle 完成构建后，从 `user_preference_profiles` 中提取 `confidence ≥ 0.8` 的 UI/交互类偏好条目（快捷键绑定、面板布局、默认字体大小等），以自动化 E2E 探针（Playwright headless）逐条验证这些习惯锚点在新版中依然生效；
   - 任意锚点校验失败，视同第一轨测试失败，演进被一票否决。锚点数量不足时（`< 3` 条高置信度 UI 偏好），第四轨跳过并在审批页标注"UX 锚点覆盖不足"。

---

### 6. 可靠性工程：进程接力、看门狗、状态机与 CLI 急救通道

针对 Windows 等 OS 的可执行文件锁定以及新版本潜在的启动崩溃风险，实施全方位的工程兜底：

#### 6.1 进程接力更新协议（Trampoline Launcher）
```
┌──────────────────┐       1. 校验就绪 & 启动       ┌────────────────────────┐
│  正在运行的宿主   │ ────────────────────────────> │  轻量接力启动器         │
│  (nuomi.exe)     │                               │  (nuomi-trampoline.exe)│
└────────┬─────────┘                               └───────────┬────────────┘
         │ 2. 释放句柄 & 退出                                   │
         ▼                                                     │
      [已终止]                                                 │ 3. 等待宿主完全退出
                                                               │ 4. 签名校验 + 原子替换
                                                               ▼
                                                    ┌────────────────────────┐
                                                    │  将新产物覆盖为原程序名  │
                                                    │  (old -> archive)      │
                                                    │  (new -> nuomi.exe)    │
                                                    └───────────┬────────────┘
                                                               │ 5. 拉起新程序
                                                               ▼
                                                    ┌────────────────────────┐
                                                    │  新版宿主正常启动运行   │
                                                    │  (nuomi.exe - vNext)   │
                                                    └────────────────────────┘
```
1. **启动与校验**：宿主校验准备完成后，拉起轻量级接力辅助进程（`nuomi-trampoline`），传递固定 CLI 参数：`--host-pid <PID> --old-bin <PATH> --new-bin <PATH> --archive-dir <PATH> --expected-hash <SHA256>`；
2. **信任根与签名校验（M2）**：
   - 宿主启动接力器前，校验 `nuomi-trampoline` 自身的哈希与内置发布指纹匹配，防止接力器被篡改；
   - 接力器在替换前，无条件执行 **数字签名与哈希双重校验**：在 Windows 上校验 Authenticode 签名，在 macOS 上校验 codesign，若为自编译无签名产物则校验 SHA-256 哈希与传入的 `--expected-hash` 严格一致；任何校验失败立即拒绝替换并中止；
3. **句柄释放与优雅退出**：宿主进程调用优雅停机（释放所有 SQLite 连接、中断并持久化活跃会话上下文、释放文件锁）后退出；若活跃会话正在执行，提示用户"演进已就绪，将在当前轮次完成后自动生效"；
4. **跨平台原子替换机制（M4）**：
   - 接力启动器轮询检测宿主 PID 完全消亡；
   - **Windows**：通过换名移动（`MoveFileExW` 带 `MOVEFILE_REPLACE_EXISTING`）将旧二进制归档至 `.nuomi/archive/`，再将新产物移至目标路径；
   - **POSIX (Linux/macOS)**：验证目标路径与新产物位于同一文件系统（`st_dev` 校验），调用原生 `rename()` 保证原子替换；若跨文件系统则先在目标同目录下生成临时副本再原子 `rename()`；
   - **macOS Bundle 结构处理**：macOS 下目标为 `.app` Bundle，替换范围为 `nuomi.app/Contents/MacOS/nuomi` 主可执行文件，配套更新 `Contents/Info.plist` 内版本号与签名；
5. **拉起新程序与版本职责冻结（M7）**：接力进程拉起全新宿主进程，在确认存活后退出；**Trampoline 职责与 CLI 接口永久冻结**（仅负责"等待 PID 终止 → 验签验哈希 → 原子换名 → 拉起新程序"），不承担版本协商，永不自更新，彻底消除版本绑定脆弱性。

#### 6.2 启动崩溃看门狗（Crash Watchdog & Safe Mode）
- **心跳计数器**：接力启动器拉起新版本前，在持久化存储中写入 `boot_attempt = { version, count: 1 }` 标记；
- **自检稳态确认**：新版本启动后执行全套自检（SQLite 读写校验、事件总线通畅度、WebView 挂载验证），稳定运行 30 秒且未发生未捕获 Panic，将 `count` 清零并正式标记版本为 `Healthy`；
- **崩溃自动回滚**：若新版本在启动自检阶段发生严重崩溃（Crash Loop），计数器累加达到 2 次：
  - 启动器判定新版本在宿主物理环境下存在致命不兼容，**自动执行逆向回退操作**：将 `.nuomi/archive/` 中的旧版程序还原；
  - 以 `--safe-mode` 命令行参数启动旧版程序，禁止任何自进化尝试，并在 UI 弹出显式的演进故障诊断对话框，向用户报告导致崩溃的调用栈信息。

#### 6.3 数据库向前兼容原则（Forward-compatible Schema Evolution）
为保证在触发看门狗紧急回滚时，旧版代码能够继续稳定读写已被新版本执行过迁移的 SQLite 数据库：
- **铁律约束**：自进化引入的所有新数据库表与列，**必须遵循非破坏性向前兼容规范**：
  - 新增列必须显式带有 `DEFAULT` 默认值，或者声明为 `NULLABLE`；
  - 严禁通过自进化修改或删除已有表、既有列、既有外键约束与索引；
  - 旧版查询逻辑中避免使用 `SELECT *` 直接绑定固定字段结构，确保新列存在不会引发旧版反序列化 Panic。

#### 6.4 演进快照链与时光穿梭（Evolution Snapshot Tree & Time Travel）
- 每次成功的 B2 演进均在 `.nuomi/snapshots/<id>/` 写入元数据档案：
  - 触发源、用户偏好特征映射、代码 Diff Patch、完整构建产物哈希与自动化测试审计报告；
- 提供类似版本控制分支的快照历史树，用户可在任意时间点选择跳回某个指定的历史版本进行回滚（Time Travel）。

#### 6.5 运行状态机与事件溯源铁律（AGENTS.md §4 对齐）
B2 自进化本身是一个重型、高风险异步流程，其生命周期必须接入状态机管理并遵循事件溯源铁律：
```
queued ─▶ compiling ─▶ evaluating ─▶ awaiting_approval ─▶ staging ─▶ replacing ─▶ succeeded
   │         │            │                  │                     │
   └─────────┴────────────┴──────────────────┴─────────────────────┴────▶ failed / cancelled
```
- **落库先于副作用铁律**：状态迁移时，必须**先向数据库追加 `evolution.state_changed` 事件**（记录 snapshot_id、from_state、to_state、metrics 快照），然后才允许触发构建、拉起弹窗或通知接力器执行物理替换。

#### 6.6 密钥安全与 OS Keyring 跨版本平滑继承（AGENTS.md §7-4 对齐）
- **沙箱零密钥输入**：沙箱内部编译与测试期间，严禁传入任何真实 API Key 或敏感私钥；
- **凭据平滑继承**：编译生成的新版二进制程序使用与宿主完全一致的 OS Keyring 服务标识符（`nuomi-app`）。新版二进制启动后，直接以系统调用从本地操作系统 Keyring（Windows Credential Manager / macOS Keychain / Linux SecretService）安全读取现有密钥，实现零中断平滑继承。

#### 6.7 CLI 终端急救通道与无头运维（AGENTS.md §5 对齐）
为防范桌面 UI 在极端崩溃时无法进入的问题，headless CLI（`nuomi-cli`）与桌面端共享底层 SQLite，并新增对应的急救命令通道：
- `nuomi evolution list`：无头环境下打印历史进化快照清单、当前激活版本与健康状态；
- `nuomi evolution rollback [snapshot_id]`：无头环境下直接调用回滚协议，从 `.nuomi/archive/` 还原指定稳定版本的二进制与配置；
- `nuomi run --safe-mode`：以纯净安全模式启动桌面或 CLI 会话，跳过所有第三方插件与个性化覆盖层。

#### 6.8 Telemetry 与 Bot 通知集成（M-BOT1 对齐，E6）
深度复用已交付的 M-BOT1 出站 sink 抽象（飞书/QQ 签名 Webhook），在关键演进生命周期节点主动推送结构化卡片通知：
- **`evolution.succeeded`（演进成功）**：推送包含版本跳变（`from_version → to_version`）、演进层级（L0~L3）、用户偏好标签汇总及影子回放性能量化对比卡片；
- **`evolution.failed`（演进评估失败）**：推送失败原因卡片（单元测试未通过项 / 影子回放性能退化幅度），附带审批重试或放弃按钮；
- **`evolution.crash_rollback`（崩溃回滚触发）**：高优先级报警卡片，报告连续崩溃事件、导致崩溃的 Panic 调用栈、已自动还原的上一安全版本及 Safe Mode 启动指引。

---

### 7. 防御纵深：自举豁免集与联网学习安全网

#### 7.1 自举豁免集与构建配置防篡改
- **路径自举豁免集**：`{sandbox/, evolution/, migrations/, keyring, AGENTS.md, pr.md, docs/adr}`。自进化引擎不可触碰自身评估判定、安全沙箱、回滚看门狗以及权威系统规则契约；
- **构建链路完整性哈希校验**：
  - 针对 Rust 构建链，将根目录 `Cargo.lock`、`Cargo.toml`（核心依赖段落）以及任意 `build.rs` 纳入构建防篡改哈希校验；
  - 自进化 Agent 生成的代码补丁在进入沙箱前，系统计算核心构建配置的 Hash 指纹；一旦检测到试图修改构建脚本注入未知外部 crate 或修改编译宏的行为，立即触发越权拦截（`sandbox.build_tamper_detected`）。

#### 7.2 外部联网学习安全隔离带
针对白名单联网学习触发 B2 进化的场景，建立**三权分立的防投毒安全隔离带**：
1. **语义抽象提取**：外部抓取的网页文章、文档或开源仓库代码仅允许提取“高阶设计模式与抽象算法描述”，严禁直接抽取原始代码片段作为 Patch 应用；
2. **AST 静态安全审查**：进入沙箱前对生成的代码执行抽象语法树（AST）静态扫描，拦截任何直接接触敏感 OS API、系统底层注册表、外部未受控 Socket 网络连接以及 Keyring 凭据的代码；
3. **强审批门禁（Awaiting Approval）**：外部联网学习触发的硬实现变更，无条件挂起为 `awaiting_approval` 状态；在 Approvals 页面显式展示“外部学习源 URL”、“AST 扫描安全报告”以及清晰的代码 Diff，**绝对禁止无人值守的静默自动替换**。

#### 7.3 L2 前端热重载的 CSP 与安全边界（M3）
Tauri 2 默认在 WebView 中强制执行严格的内容安全策略（Content Security Policy, CSP）：
1. **静态 CSP 合规扫描**：在构建静态前端 Dist 阶段，AST 静态扫描器检查生成的 React / TS 代码，严格拦截引入外部 CDN 脚本、动态 `eval()`、内联不可信脚本或非本地域名资源的行为；
2. **热重载安全挂载**：沙箱打包产出静态文件后，向 Tauri 宿主发送 `evolution_reload_frontend` IPC 命令；宿主校验产物 Hash 指纹与权限后，通知 WebView 主窗口通过安全 `asset://` 协议重新加载页面，毫秒级生效，杜绝向外暴露不可信代码执行面。

#### 7.4 演进自动化授权分级控制（M6）
为平衡演进效率与用户系统安全掌控，设立梯度可配的自动化授权策略，持久化记录于 `user_preference_profiles`（键 `evolution.auto_approval_policy`）：
- **L0（软偏好参数）**：默认静默自动应用（配置生效，无需重启）；
- **L1（NPP 插件）**：默认自动热装配，UI 弹出轻量 Toast 提示新能力就绪；
- **L2（前端热重载）**：默认在右下角弹出确认卡片，经用户点击确认后执行热重载；亦可配置为“自动确认”；
- **L3（原生内核替换）**：**绝对红线，强制要求显式审批**；在 Approvals 页面完整展示代码 Diff、测试与影子回放报告，严禁任何形式的静默替换；
- **外部联网学习来源**：无论最终判定为 L0、L1 还是 L2，**无条件强制挂起为 `awaiting_approval`**。

---

### 8. 架构治理：个性化层 vs 共享内核与用户掌控看板

#### 8.1 共享内核与个性化层的物理分离
为了保证经历深度自进化后的 Harness 仍然能够平滑接收上游官方版本的核心安全补丁升级：
- **共享内核（Shared Core）**：插件底座、IPC 契约引擎、事件总线、数据迁移框架、基础安全沙箱。此部分代码由官方演进维护，本地自进化受到高门槛豁免限制；
- **个性化层（Personalized Layer）**：自适应 UI 布局、快捷键映射集、交互响应策略、CLI Agent 方言适配器、专业领域专用工具。允许用户 Agent 自由重写分叉。

#### 8.2 进化罗盘与偏好看板（Evolution Compass UI）
在 Settings 与看板中提供独立的“自进化与偏好控制台”，彻底消除黑盒不确定性，将掌控权交还用户：
- **用户画像透明可控**：显式展示 Agent 当前学习到的偏好标签（如：“习惯 TypeScript 严格类型”、“偏好紧凑暗色排版”、“测试驱动开发驱动”），用户可手动进行校准、增补或删除；
- **“忒修斯之船”改造进度可视化**：直观展示系统各模块的重构比例，哪些组件依然是“原厂代码”，哪些已经被“个性化重构”；
- **一键恢复出厂设置（Reset to Factory Default）**：提供细粒度的重置选项，允许用户随时将某个已被改写的模块、或者整个 Harness 一键回滚到初始官方状态；
- **演进方向前瞻预测（Future Direction Prediction，E2）**：基于累积画像标签的聚合分析及近期 B1 触发热度，由 LLM 生成“下一步最可能发生的演进预测摘要”（例如：“检测到您最近频繁排查异步并发问题，推荐在下次演进中扩展 Tokio 追踪日志插件”），使用户从单纯审查历史升级为前瞻掌控演进趋势；
- **演进沙盒预演（Dry-Run Preview，E3）**：新增“演进预演”入口（对应 IPC `evolution_dry_run_preview`），在沙箱中仅生成代码补丁 Diff、评估受影响组件清单并进行静态 AST 扫描，不启动耗时编译与实际替换，零风险直观预览演进全景；
- **人工定向触发演进入口（Manual Trigger，E5）**：支持用户在 Compass 界面或终端 CLI（`nuomi evolution trigger --tier <L0..L3>`）输入自然语言指令主动发起演进，系统将当前画像上下文与指定目标范围作为输入，立即启动沙箱演进闭环。

---

### 9. 数据面：迁移 0043 设计

根据 AGENTS.md §7-3 只增不改铁律，新增 `migrations/0043_evolution_snapshots_and_preferences.sql`：

```sql
-- 0043_evolution_snapshots_and_preferences.sql
-- 进化快照树表 (支持版本树时光穿梭与父子分支追溯)
CREATE TABLE IF NOT EXISTS evolution_snapshots (
    id TEXT PRIMARY KEY NOT NULL,
    parent_snapshot_id TEXT REFERENCES evolution_snapshots(id), -- 父快照 ID (根版本为 NULL, 构造版本演进树)
    trigger_source TEXT NOT NULL,         -- 'b1_accumulation' | 'online_research' | 'user_manual'
    tier TEXT NOT NULL,                   -- 'L0' | 'L1' | 'L2' | 'L3'
    from_version TEXT NOT NULL,
    to_version TEXT NOT NULL,
    profile_summary TEXT NOT NULL,        -- JSON (强类型契约: nuomi_core::evolution::ProfileSummary)
    diff_patch_ref TEXT NOT NULL,         -- 相对产物目录路径
    artifact_ref TEXT,                    -- L3 对应的新版二进制归档路径
    status TEXT NOT NULL CHECK(status IN ('staged', 'active', 'reverted', 'discarded')),
    metrics TEXT,                         -- JSON (强类型契约: nuomi_core::evolution::EvaluationMetrics)
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_evolution_snapshots_status ON evolution_snapshots(status);
CREATE INDEX IF NOT EXISTS idx_evolution_snapshots_created ON evolution_snapshots(created_at);
CREATE INDEX IF NOT EXISTS idx_evolution_snapshots_parent ON evolution_snapshots(parent_snapshot_id);

-- 用户偏好画像表 (B1 产物与用户微调画像)
CREATE TABLE IF NOT EXISTS user_preference_profiles (
    key TEXT PRIMARY KEY NOT NULL,        -- 唯一偏好标识符 (如: 'editor.type_strictness')
    category TEXT NOT NULL,               -- 'coding_style' | 'ui_theme' | 'interaction' | 'tool_usage'
    value JSON NOT NULL,                  -- 偏好具体结构化值
    confidence REAL NOT NULL DEFAULT 1.0, -- 置信度 (0.0 ~ 1.0)
    source_session_id TEXT,               -- 来源会话 ID (可空)
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_user_preferences_category ON user_preference_profiles(category);
```

---

### 10. IPC 命令组契约清单（tauri-specta）

严格对齐“契约先行（Contract-first）”规则，更新 Rust 端 `src-tauri/src/commands.rs` 与前端 `src/lib/ipc/bindings.gen.ts`：

| 命令名称 | 类型 | 参数 | 返回值 | 语义说明 |
|---|---|---|---|---|
| `sandbox_run_workbench_target` | **新增** | `target_path: String`, `is_project: bool` | `SandboxTaskDto` | 场景 A：在沙箱运行工作台当前文件或项目 |
| `sandbox_build_app` | **修改** | `request: SandboxBuildAppRequest` (增加 `profile`) | `SandboxTaskDto` | 扩展支持 `Profile-Light` 与 `Profile-Heavy` |
| `sandbox_run_code` | **降级** | 内部调用 | `SandboxTaskDto` | 取消前端公开暴露，仅供自进化 Agent 探针内部调用 |
| `evolution_get_compass` | **新增** | 无 | `EvolutionCompassDto` | 获取用户偏好画像标签与系统模块改造百分比 |
| `evolution_update_preference` | **新增** | `key: String`, `value: Value` | `Result<(), IpcError>` | 用户在设置页面手动校正/更新某项画像偏好 |
| `evolution_list_snapshots` | **新增** | `limit: Option<u32>` | `Vec<EvolutionSnapshotDto>` | 获取历史演进快照链列表 |
| `evolution_rollback_to_snapshot` | **新增** | `snapshot_id: String` | `Result<String, IpcError>` | 触发时光穿梭，回滚至指定历史快照 |
| `evolution_reset_component` | **新增** | `component_id: String` | `Result<(), IpcError>` | 将指定模块一键重置恢复为出厂官方实现 |
| `evolution_reload_frontend` | **新增** | `bundle_hash: String` | `Result<(), IpcError>` | L2 前端热重载：通知 WebView 安全重新挂载前端 dist (M3) |
| `evolution_dry_run_preview` | **新增** | `tier: Option<String>, prompt: Option<String>` | `EvolutionDryRunDto` | 演进沙盒预演：生成 Diff Patch 与影响面，不编译不替换 (E3) |
| `evolution_trigger` | **新增** | `tier: Option<String>, instruction: Option<String>` | `EvolutionTaskDto` | 人工显式触发演进闭环 (E5) |
| `evolution_revoke_asset` | **新增** | `asset_type: String, key: String` | `Result<(), IpcError>` | B1 沉淀 30 秒软撤回：追加 `evolution.asset_revoked` 事件 (E1) |

---

### 11. 立项与版本演进：K9 独立立项

- **K8（Prompt 自进化）**：保持范围不变，专注于 SystemPrompt candidates 版本化、GEPA 式反思与白名单联网调研；
- **K9（代码级个性化自进化·忒修斯之船）**：独立立项，承载从用户层资产沉淀（B1）到 Harness 代码级分层沙箱构建、双轨评估、接力替换与自愈（B2）的完整工程闭环。`AGENTS.md` 路线图同步追加 K9。

---

## 向后兼容性（Backwards Compatibility）

1. **存量沙箱任务与数据**：存量数据库无缝升级，`migrations/0043` 仅新增两张独立表，对存量表零变更、重复执行幂等；
2. **沙箱前端视图降级**：旧版手抄运行历史数据完全保留，`SandboxView` 转为只读后仍可检索与查看所有历史 Task 的标准输出与 Exit Info；
3. **安全模式保障**：旧版环境若加载了新版编写的高级用户画像，偏好模型层将通过 `serde(default)` 自动降级忽略无法识别的新字段，保障向前与向后兼容；
4. **渐进式生效**：未配置沙箱或沙箱插件处于关闭状态时，自进化功能静默挂起并降级为纯 B1 提示，工作台右键入口给出清晰的不可用指引，不阻碍主应用运行。

---

## 权衡与替代方案（Trade-offs & Alternatives Considered）

- **替代方案 (a)：采用动态链接库（`.dll` / `.so` dlopen）热加载替换**
  - *否决理由*：Rust 的 ABI 不稳定，跨版本、跨编译器版本的动态重载极其容易引发内存段错误（Segmentation Fault）；且桌面壳基于 Tauri 静态绑定，无法将整个 Tokio 运行时与核心总线抽离为纯动态库。
- **替代方案 (b)：采用 Git Worktree 作为沙箱工作区**
  - *否决理由*：自进化包含大量临时中间编译对象（`target/`），Git Worktree 会污染本地主仓库的元数据索引，且在并发构建清理时容易破坏 `.git` 引用锁定；第四层独立的 Ephemeral 临时目录配合硬配额 GC 更干净、隔离性更强。
- **替代方案 (c)：仅采用 LLM-as-judge，不引入自动化测试基线与影子回放**
  - *否决理由*：大模型代码审查存在幻觉、喜新厌旧和对深层并发 Bug 不敏感的问题，必须有 100% 测试通过作为客观测验红线防线。
- **替代方案 (d)：仅支持单步回滚，不建立版本快照树**
  - *否决理由*：单步回滚无法防范隐蔽缺陷在经历多次演进后才暴露的场景；快照树记录完整元数据，具备时光穿梭能力，大幅降低演进试错成本。

---

## 风险与缓解（Risks & Mitigations）

| 风险场景 | 影响程度 | 核心防御与缓解措施 |
|---|---|---|
| **Windows 运行中文件锁导致替换崩溃** | 严重（阻断演进） | 引入独立轻量接力启动器（Trampoline Launcher），待宿主安全退出释放所有句柄后再执行文件原子换名。 |
| **宿主替换后启动即 Panic (Crash Loop)** | 致命（瘫痪主应用） | 启动看门狗监测自检心跳，连续 2 次崩溃自动逆向回滚上一稳定归档版本，并以 `--safe-mode` 启动输出诊断。 |
| **沙箱内全量编译撑爆磁盘** | 高（系统变卡） | 评估归档后强制执行 Build Cache GC 物理清空 `target/`；临时工作目录实施总容量 10GB 硬配额与 LRU 回收。 |
| **外部联网学习代码投毒 / 注入** | 致命（安全提权） | 三权分立机制：仅提取设计模式，禁止外部代码直通 Patch；AST 扫描拦截高危调用；强制挂起人工审批。 |
| **频繁空闲判定引发弹窗轰炸与 Token 浪费** | 中（体验受损） | 增加 60s 空闲防抖；仅在纠错、显式偏好或复杂任务完成等高置信度事件发生时触发；设置每日 Token 预算上限。 |
| **新版本数据库迁移导致旧版本回滚报错** | 高（数据不可读） | 自进化涉及的表结构迁移遵循“非破坏性向前兼容规范”：新列必须有默认值或为 NULLABLE，禁止删除既有表列。 |
| **CLI 应急通道缺失** | 高（无头死锁） | `nuomi-cli` 新增 `evolution list/rollback/--safe-mode`，保证桌面 UI 崩溃时可通过终端直接急救恢复。 |
| **接力启动链被篡改 / 投毒 (M2)** | 致命（提权攻击） | 宿主启动前核实 Trampoline 哈希；接力器强制校验新产物 Authenticode / codesign 签名与 SHA-256 哈希。 |
| **POSIX 跨文件系统替换非原子性 (M4)** | 高（替换中断） | 替换前检测 `st_dev`，跨文件系统时在目标同盘生成临时副本再原子 `rename()`；macOS 正确替换 `.app` 子树。 |
| **L2 前端演进代码破坏 Tauri 2 CSP (M3)** | 中（界面挂死） | AST 扫描器严格拦截外链脚本与动态 `eval()`；前端通过安全 `asset://` 协议热挂载。 |

---

## 实施路线规划与 Dev Team 责任人分工

| 切片 | 主题 | 核心范围与交付物 | 牵头角色 (Owner) | 验收证据 |
|---|---|---|---|---|
| **Slice 1** | 沙箱入口重构与工作台原生接入 | 删除 `SandboxCodeEditor`，沙箱页转只读；工作台编辑器实现右键与工具栏 "Run in Sandbox"（对接 `sandbox_build_app` 契约）；完成 Dry-Run 预演（E3）命令骨架 | `bridge-engineer` + `ui-engineer` | 工作台单文件/工程在沙箱正常运行，独立沙箱页可查看审计 |
| **Slice 2** | 第四层临时目录与两阶段 GC | 正式化 `nuomi-selfeval-{id}` 目录管理；实现两阶段缓存 GC（失败保留 24h，成功全清，O3）与磁盘配额 LRU 算法；扩展内核授权目录收敛逻辑 | `core-engineer` | 临时目录自动挂载，重试演进复用 target 缓存，归档后体积迅速归零 |
| **Slice 3** | B1 用户层沉淀、动态防抖与软撤回 | 偏好模型与 `user_preference_profiles` 迁移 0043（M5 parent 外键）；动态防抖与冷却调节（O2）；30 秒软撤回 Toast（E1）；K8 统一接口对接（M1） | `core-engineer` + `ui-engineer` | 模拟纠错对话触发弹窗，支持 30 秒内撤回；规约精准入库且不破安全铁律 |
| **Slice 4** | B2-L1/L2 轻量演进、CSP 与四轨评测 | NPP 插件动态装配至 `.nuomi/plugins/evolved/`（E4）；前端 Vite 静态产物构建、CSP 静态合规扫描与安全热重载（M3）；四轨评估管线（硬基线 + 影子回放量化阈值 O1/O5 + LlmReview + UX 锚点探针 E7） | `core-engineer` + `ui-engineer` | 自动化测试未过一票否决；影子回放退化超 15% 拦截；前端修改无需重启即时热重载生效 |
| **Slice 5** | B2-L3 重型内核替换、自愈看门狗与 Bot 集成 | 跨平台接力启动器（Trampoline，职责永久冻结 M7）；Windows 文件锁解约与 POSIX 原子替换（M4）；Authenticode/codesign 验签信任根（M2）；看门狗自愈；CLI 应急通道；M-BOT1 Telemetry 通知（E6） | `core-engineer` + `bridge-engineer` | 模拟宿主启动 Panic，看门狗自动回退 Safe Mode 并推送飞书告警卡片；CLI 终端可急救 |
| **Slice 6** | 防御纵深、授权分级与偏好看板 (UI) | 联网学习 AST 静态审查；自动化授权分级控制（M6）；设置页落地**进化罗盘（Evolution Compass）**：画像透明化、前瞻预测（E2）、Dry-Run 预演 UI（E3）、人工触发演进（E5）与一键出厂重置 | `ui-engineer` + `qa-guardian` | 偏好画像可视化增删；点击一键出厂重置瞬间恢复官方版本；前瞻预测卡片正常显示 |

---

## 开放问题

1. **跨平台接力启动器（Trampoline Launcher）的物理载体**：优先采用随应用一起编译的超轻量独立 Rust 子二进制（`nuomi-trampoline`，体积 < 500KB），相比跨平台 Shell 脚本具备更好的 Windows 句柄追踪能力与跨平台一致性——留待 Slice 5 进行最终架构落地验证；
2. **影子任务回放的数据敏感度与脱敏策略**：历史真实任务轨迹中若包含用户输入，需要在回放前进行脱敏，或维护一组由系统预置的 Golden Benchmarks（覆盖代码生成、重构、编译与测试排查典型用例）——留待 Slice 4 细化；
3. **`nuomi-trampoline` 的版本绑定与防篡改分发（M7/M2）**：接力启动器职责已永久冻结为纯工具进程，是否在安装包构建阶段将 Trampoline 的 SHA-256 签名硬编码进宿主二进制常量以彻底阻断本地替换攻击——留待 Slice 5 确定；
4. **活跃会话中触发 L3 演进的无感接力策略**：当宿主准备退出由 Trampoline 替换时，若当前有进行中的长任务对话，是等待当前轮次完成、还是通过会话快照持久化在新宿主拉起后自动续传（Session Resume）——留待 Slice 5 细化；
5. **历史演进快照（Snapshot）的磁盘配额独立核算**：与第四层临时构建目录（10GB LRU）不同，`.nuomi/snapshots/` 内归档的 L3 二进制和 Patch 属于持久历史版本，需设立独立的数量上限（默认保留最近 5 个版本，更早版本仅保留 Patch 剔除二进制产物）——留待 Slice 2 决策。
