-- 0035_steward_main_role_seed: 管家主 Role 种子。
-- Append-only — never edit shipped migrations.
--
-- 本 Role 为管家对话窗口的配置载体：
--   SOUL.md       = system_prompt_override（管家人格提示词，NULL = 回退内置默认）
--   绑定 Provider = provider_id / provider_ids
--   绑定 CLI     = params_json.agent_profile_id
-- 不参与研发团队角色分发（role_steward_researcher 等 5 角色由 0031 种子）。
--
-- INSERT OR IGNORE 保证幂等：重复运行不报错，已存在则跳过。
-- builtin = 1 保护：reference_pre_check::delete_role 拒绝删除 builtin Role。
-- created_at/updated_at 用 0 保证迁移确定性（内置角色创建时间无业务意义）。

INSERT OR IGNORE INTO roles (id, name, system_prompt_override, tool_allowlist, params_json, builtin, created_at, updated_at)
VALUES ('role_steward_main', 'Steward: Main', NULL, '[]', '{}', 1, 0, 0);
