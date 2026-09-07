-- 0010_role_capabilities: Role capability system (Agent = Role + Provider).
-- Append-only: new columns on `roles` only; no existing column is touched.
--   required_capabilities — JSON array of system capabilities
--     ("reasoning"|"image"|"voice"|"video") the role requires from its providers.
--   provider_ids — JSON array of provider_configs ids (multi-binding; the
--     legacy `provider_id` column stays in sync with the first entry).
--   builtin / generated / ephemeral — lifecycle flags: preset-catalog seeds,
--     Role-Director products (provenance in source_json), and
--     capability-router temp roles (GC'd after runs) respectively.
ALTER TABLE roles ADD COLUMN required_capabilities TEXT NOT NULL DEFAULT '[]';
ALTER TABLE roles ADD COLUMN provider_ids TEXT NOT NULL DEFAULT '[]';
ALTER TABLE roles ADD COLUMN builtin INTEGER NOT NULL DEFAULT 0;
ALTER TABLE roles ADD COLUMN generated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE roles ADD COLUMN ephemeral INTEGER NOT NULL DEFAULT 0;
ALTER TABLE roles ADD COLUMN source_json TEXT;
