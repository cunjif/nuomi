-- 0020_conversation_role_agent_cli_session: ADR 0012 —
-- (1) session_cli_handles: per (session, role_agent) CLI Agent session id.
-- (2) agent_profiles.resume_args: CLI 会话保持参数模板（覆盖方言默认）。
-- (3) app_settings: cli_context_handover_tokens（绑定变更时传截断上下文 token 上限）。
-- (4) 清空既有 agent_kind='cli' 的 session agent 绑定（CLI Agent 须通过 Role 绑定）。
-- Append-only: never edit shipped migrations.

CREATE TABLE session_cli_handles (
    session_id       TEXT NOT NULL,
    role_agent_id    TEXT NOT NULL,
    agent_profile_id TEXT NOT NULL,
    cli_session_id   TEXT,
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (session_id, role_agent_id)
);

ALTER TABLE agent_profiles ADD COLUMN resume_args TEXT;

INSERT OR IGNORE INTO app_settings (key, value, updated_at)
    VALUES ('cli_context_handover_tokens', '65536', 0);

UPDATE sessions SET agent_kind = NULL, agent_ref_id = NULL WHERE agent_kind = 'cli';
