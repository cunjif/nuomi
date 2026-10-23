-- 0040_session_sources_auth: 会话工作目录绑定 + 文件操作授权 + 默认授权策略
-- + 授权超时阈值种子（chat_rich_bind）。
-- Append-only — never edit shipped migrations.
--
-- ① sessions.work_directory_id：会话级工作目录绑定（spec 5.4/6.4）。不带
--    REFERENCES——绑定归属校验（目录须属目标工作区且 mounted）在 repo 层
--    事务内执行（对齐 work_directories 的 repo 层校验惯例）；无 FK 使目录
--    行可被显式移除，而绑定值保留并由查询派生 available=false（append 语义）。
-- ② file_authorizations：授权请求行（spec 6.2）。状态单调向终态迁移；
--    waiting_notified 为派生态（pending + timeout_notified 事件），不占表状态。
-- ③ agent_auth_policies：RoleAgent 默认授权策略（spec 6.3）。"unset" 语义为
--    无行——避免三态魔法值。
-- ④ app_settings 种子 authorization.timeout_seconds = '120'（INSERT OR IGNORE
--    幂等，对齐 0035 种子模式；updated_at=0 保证迁移确定性）。
-- ⑤ attachments 表零变更（D8：压缩包落库 kind='file'，类别运行时派生）；
--    来源证据零表（D1：sources 随 message 事件 payload 落库）。

ALTER TABLE sessions ADD COLUMN work_directory_id TEXT NULL;

CREATE TABLE file_authorizations (
    id           TEXT PRIMARY KEY,            -- uuid-v7
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role_id      TEXT NOT NULL,               -- 发起授权请求的 RoleAgent
    op           TEXT NOT NULL CHECK (op IN ('write', 'delete')),
    target_path  TEXT NOT NULL,
    state        TEXT NOT NULL CHECK (state IN ('pending', 'granted_once', 'granted_session', 'denied', 'timeout_default', 'voided')),
    requested_at INTEGER NOT NULL,            -- unix-ms
    expires_at   INTEGER NOT NULL,            -- unix-ms
    resolved_at  INTEGER,
    resolution   TEXT
);

CREATE INDEX idx_file_auth_session ON file_authorizations (session_id, state);
CREATE INDEX idx_file_auth_sweep ON file_authorizations (state, expires_at);

CREATE TABLE agent_auth_policies (
    role_id    TEXT PRIMARY KEY,
    policy     TEXT NOT NULL CHECK (policy IN ('authorize', 'deny')),
    updated_at INTEGER NOT NULL               -- unix-ms
);

INSERT OR IGNORE INTO app_settings (key, value, updated_at)
VALUES ('authorization.timeout_seconds', '120', 0);
