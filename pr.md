<!-- fullWidth: false tocVisible: false tableWrap: true -->
# Nuomi Agent 开发需求

1. 模型配置方面

- 支持主流的Provider和OpenAICompatible、AnthropicCompatible
- 支持Master-Slave模式
- 支持配置不同功能的Provider，主Provider可以调用子Provider

2. Agent方面

- 支持不同Cli Agent接入，如：Codex Cli、Claude code、OpenCode Cli、KiloCode Cli、Kiro Cli、Codebuddy Cli、CodeArts Cli、Antigravity Cli等
- 支持同一个Provider/Cli配置不同Role角色
- 支持自定义Role Agent
- 支持自发Agent Team
- 支持自定义Agent Team
- 支持主流的Multi Agent实现机制，如路由模型、串行模型、抢占式模型等

3. 架构

- 参考Deepseek Harness（或考虑基于），以Cordis为核心构建（Cordis理论参考：https://cordis.moe/zh-CN/guide/）
- 把Command、Hook、Skills、MCP、Plugins、ReAct、Loop-Run、Memory、SystemPrompt、Loop Engine等所有Agent Harness相关的组件均插件化
- 支持Self-Evolution，基于GEPA + Long-term Memory，不以当前会话作为演进的唯一来源，需要综合考虑用户画像、记忆、历史轨迹和当前轨迹
- 若用户同意，可以定期联网查询Agent Harness相关优秀实现案例（Pi Agent、Hermes Agent、EvoMap、Deepseek Harness），获取最新实现成果，融合到本身的evolution进程中

4. 工具编排

- 原生支持SystemPrompt、UserMessage、History、Assistant、Tool Results、Memory等上下文动态剪枝（dynamic context prune）
- 支持MultiAgent编排/自由组队、Skills编排、Mcp编排、Command编排、Hook编排以及混合编排

5. UI

- 主界面\
  |                             |              |                   |\
  | {git worktree|git|sessions} | {对话bubble} | {文件目录|其他功能} |\
  |                             | {输入框}     |                    |
- 打开文件\
  |                             |              | {文件tab} |                     |\
  | {git worktree|git|sessions} | {对话bubble} | {编辑页面} |  {文件目录|其他功能} |\
  |                             | {输入框}     |           |                     |
- 打开SubAgent执行情况\
  |                             |              | {SubAgent}           |                     |\
  | {git worktree|git|sessions} | {对话bubble} | {SubAgent的执行Trace} |  {文件目录|其他功能} |\
  |                             | {输入框}     |                       |                    |

6. MulitAgent

- 支持看板
- 支持WhiteBoard
- 支持Telemetry、飞书Bot、QQBot
- 支持将以创建的Agent组成群聊推进工作
- 支持自动创建Agent群里
- 支持所有主流MulitAgent协作方式

7. 参考项目
- Codeg（https://github.com/xintaofei/codeg?ref=aitoolnet.com）
- auto-claude（https://github.com/AndyMik90/Aperant?ref=aitoolnet.com）
- Orca
- Hermes
- EvoMap
- Pi Agent
- Shikigami（https://shikigami.dev/?ref=aitoolnet.com）
- T3 Code (https://t3.codes/?ref=aitoolnet.com)
- **Deepseek Harness**（https://www.deepseek.com/harness/en/） + dsh-better-sidebar
- OpenCode + oh-my-openagent
- Agents Team（https://agentteams.live/?ref=aitoolnet.com）
- 1Code（https://1code.dev/?ref=aitoolnet.com）
- Agent Orchestrator（https://aoagents.dev/?ref=aitoolnet.com）
- Parallel Code（https://parallelcode.app/?ref=aitoolnet.com）
- Agor（https://agor.live/?ref=aitoolnet.com）

8. **目标**
- **创建超越MultiAgent、个人Agent的跨时代的Agent项目**
- UI/UX将超越用户预期
- 在Token消耗、缓存命中、cli和原生Agent沟通协作上有创新性突破