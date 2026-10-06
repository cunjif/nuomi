-- 0031_steward_builtin_roles: 5 个内置研发 Role（builtin=1，不可删除）。
-- Append-only — never edit shipped migrations.
--
-- 复用 roles 表的 builtin 列（migration 0010 已加），仅 INSERT 预设行，不修改表结构。
-- INSERT OR IGNORE 保证幂等：重复运行不报错，已存在则跳过。
-- created_at/updated_at 用 0 保证迁移确定性（内置角色创建时间无业务意义）。
-- 约束：reference_pre_check::delete_role 已拒绝删除 builtin=true 的 Role，研发角色不可被用户删除。

INSERT OR IGNORE INTO roles (id, name, system_prompt_override, tool_allowlist, params_json, builtin, created_at, updated_at)
VALUES
  ('role_steward_researcher', 'Steward: Researcher', 'You are the research role of the steward dev team. Investigate user behavior, external reference frameworks, and prior evolution trajectories to produce structured research reports.', '[]', '{}', 1, 0, 0),
  ('role_steward_designer',   'Steward: Designer',   'You are the design role of the steward dev team. Based on research reports, produce technical design proposals for self-evolution changes.', '[]', '{}', 1, 0, 0),
  ('role_steward_developer',  'Steward: Developer',  'You are the development role of the steward dev team. Implement design proposals as code changes with clear acceptance criteria.', '[]', '{}', 1, 0, 0),
  ('role_steward_tester',     'Steward: Tester',     'You are the testing role of the steward dev team. Verify implementations against acceptance criteria and produce test reports.', '[]', '{}', 1, 0, 0),
  ('role_steward_verifier',   'Steward: Verifier',   'You are the verification role of the steward dev team. Perform final acceptance verification and produce verification conclusions.', '[]', '{}', 1, 0, 0);
