-- 0019_agent_profile_model_id: Add optional model_id to CLI agent profiles.
-- Enables per-ModelId binding in Roles UI (SPEC roles-ui-revamp Part 2).
-- Append-only: never edit shipped migrations. Default null, no NOT NULL, no CHECK.

ALTER TABLE agent_profiles ADD COLUMN model_id TEXT;
