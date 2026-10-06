-- 0029_evolution_cycles: 进化周期 + 任务 + 产物。
-- Append-only — never edit shipped migrations.
--
-- evolution_cycles: 自进化周期状态机，phase 流转 cleanse→research→design→develop→test→verify→gate→merge。
-- evolution_tasks: 周期内任务，按依赖图派发给研发团队角色。
--   depends_on_json: JSON 数组 TEXT（任务 id 列表，任务数 ≤ 10，无需关系表）。
-- evolution_artifacts: 研发产物，status 流转 pending_review→approved/rejected/needs_revision。

CREATE TABLE evolution_cycles (
    id              TEXT PRIMARY KEY,
    trigger_source  TEXT NOT NULL CHECK (trigger_source IN ('user','scheduled','self_reflect')),
    trigger_context TEXT NOT NULL DEFAULT '',
    phase           TEXT NOT NULL DEFAULT 'cleanse'
                    CHECK (phase IN ('cleanse','research','design','develop','test','verify','gate','merge')),
    status          TEXT NOT NULL DEFAULT 'running'
                    CHECK (status IN ('running','completed','cancelled','failed')),
    created_at      INTEGER NOT NULL,
    ended_at        INTEGER
);
CREATE INDEX idx_cycles_status ON evolution_cycles (status, created_at DESC);

CREATE TABLE evolution_tasks (
    id                  TEXT PRIMARY KEY,
    cycle_id            TEXT NOT NULL REFERENCES evolution_cycles(id),
    phase               TEXT NOT NULL CHECK (phase IN ('research','design','develop','test','verify')),
    dev_role            TEXT NOT NULL CHECK (dev_role IN ('researcher','designer','developer','tester','verifier')),
    depends_on_json     TEXT NOT NULL DEFAULT '[]',
    status              TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending','running','completed','failed','cancelled')),
    acceptance_criteria TEXT NOT NULL,
    trigger_source      TEXT NOT NULL,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);
CREATE INDEX idx_tasks_cycle ON evolution_tasks (cycle_id, status);

CREATE TABLE evolution_artifacts (
    id                TEXT PRIMARY KEY,
    task_id           TEXT NOT NULL REFERENCES evolution_tasks(id),
    produced_by_role  TEXT NOT NULL,
    artifact_type     TEXT NOT NULL CHECK (artifact_type IN (
                        'research_report','design_proposal','prompt_candidate',
                        'config_change','new_role','new_team','test_report','verification')),
    content_json      TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'pending_review'
                      CHECK (status IN ('pending_review','approved','rejected','needs_revision')),
    diff_preview      TEXT,
    rollback_plan_json TEXT,
    created_at        INTEGER NOT NULL
);
CREATE INDEX idx_artifacts_status ON evolution_artifacts (status, created_at DESC);
CREATE INDEX idx_artifacts_task ON evolution_artifacts (task_id);
