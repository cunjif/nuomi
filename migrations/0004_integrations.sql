-- 0004_integrations: outbound bots & telemetry endpoints (SPEC bots-telemetry-m1 D2).
CREATE TABLE integrations (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL CHECK (kind IN ('feishu_bot', 'qq_webhook', 'telemetry')),
    config_json TEXT NOT NULL DEFAULT '{}',
    events      TEXT NOT NULL DEFAULT '[]',
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
