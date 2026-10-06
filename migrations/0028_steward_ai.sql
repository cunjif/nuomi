-- 0028_steward_ai: 管家 AI 单例 + 研发团队单例 + 研发角色绑定。
-- Append-only — never edit shipped migrations.
--
-- steward_ai: 管家 AI 单例（应用级，全局唯一）。
--   ready=1 表示研发团队已初始化（5 个内置角色绑定就绪）。
--   online_authorized=1 表示联网调研已授权（与 evolution::research 共享语义）。
-- steward_dev_team: 研发团队单例（1:1 引用 steward_ai），约定 id='steward_dev_team'。
-- steward_dev_role_bindings: 5 个研发角色绑定（PK=role_kind），每角色绑定一个 CLI Agent 或 RoleAgent。

CREATE TABLE steward_ai (
    id                 TEXT PRIMARY KEY,
    ready              INTEGER NOT NULL DEFAULT 0,
    online_authorized  INTEGER NOT NULL DEFAULT 0,
    created_at         INTEGER NOT NULL
);

CREATE TABLE steward_dev_team (
    id          TEXT PRIMARY KEY,
    steward_id  TEXT NOT NULL REFERENCES steward_ai(id),
    created_at  INTEGER NOT NULL
);

CREATE TABLE steward_dev_role_bindings (
    role_kind    TEXT NOT NULL CHECK (role_kind IN ('researcher','designer','developer','tester','verifier')),
    agent_kind   TEXT NOT NULL CHECK (agent_kind IN ('cli','role')),
    agent_ref_id TEXT NOT NULL,
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (role_kind)
);
