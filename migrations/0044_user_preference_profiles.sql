-- 0044_user_preference_profiles: B1 用户偏好画像事实源（ADR-0025 Slice 3）。
-- Append-only — never edit shipped migrations.
--
-- 背景：ADR-0025 §2.4 设立 user_preference_profiles 为全局唯一偏好事实源。
-- B1 沉淀流程写入，K8（PromptVersion 反思引擎）与 B2（项目层演进）只读。
-- 访问契约：通过 evolution::PreferenceService::snapshot() 接口获取偏好快照，
-- 不直接 SQL 查询（松耦合）。
--
-- 资产分级（ADR §2.2）：
--   convention — 跨项目通用的用户行为准则（写入 ~/.nuomi/rules/user-preferences.md）
--   memory     — 用户偏好知识事实（写入 memory_entries，user_profile=1）
--   skill      — 工程技术栈相关的操作流与脚本（写入 .nuomi/skills/<name>/）
--
-- 软撤回（ADR §2.3）：弹窗确认后 30s 内可撤回，status → revoked，
-- 追加 evolution.asset_revoked 事件，不物理删除。

CREATE TABLE user_preference_profiles (
    id                TEXT PRIMARY KEY,
    -- convention | memory | skill
    category          TEXT NOT NULL,
    -- 资产键（如 "rust.no_unwrap", "ui.compact_dark"）
    key               TEXT NOT NULL,
    -- 偏好内容描述
    content           TEXT NOT NULL,
    -- 置信度 0.0~1.0（高置信度 ≥ 0.8 触发 UX 锚点检查，ADR §5 第四轨）
    confidence        REAL NOT NULL DEFAULT 0.5,
    -- 来源会话（M5 parent 外键）
    source_session_id TEXT REFERENCES sessions(id),
    -- confirmed | revoked
    status            TEXT NOT NULL DEFAULT 'confirmed',
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    -- unix-ms; NULL until status becomes 'revoked'
    revoked_at        INTEGER
);

CREATE INDEX idx_pref_profiles_category ON user_preference_profiles (category);
CREATE INDEX idx_pref_profiles_status ON user_preference_profiles (status);
CREATE INDEX idx_pref_profiles_confidence ON user_preference_profiles (confidence DESC);
CREATE INDEX idx_pref_profiles_session ON user_preference_profiles (source_session_id);
