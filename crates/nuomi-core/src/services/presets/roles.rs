//! Preset Role definitions + idempotent seeding into the `roles` table.
//!
//! Seeding rules (first launch and the "restore presets" button share this
//! single code path):
//! - a preset whose `name` is **absent** is inserted with `builtin = true`;
//! - a preset whose `name` exists **and is builtin** is refreshed in place
//!   (system prompt / capabilities / description) so preset improvements
//!   propagate — the provider binding and user edits to other columns are
//!   preserved;
//! - a preset whose `name` belongs to a **user-created** role is skipped —
//!   user data always wins.
//!
//! Built-in roles cannot be deleted (`role.builtin_protected`); users may
//! disable/rebind them instead.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{new_id, now_ms, Capability, Role};
use crate::store::{migrations, repos, StoreError};

/// One cross-domain preset role (static catalog entry).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PresetRole {
    pub name: &'static str,
    pub description: &'static str,
    pub system_prompt: &'static str,
    pub required_capabilities: &'static [Capability],
    /// Free-form domain labels (display/grouping hints), stored in
    /// `params.domain_tags`.
    pub domain_tags: &'static [&'static str],
}

impl PresetRole {
    /// Projects the preset onto a fresh (or refreshing) [`Role`] row.
    pub fn to_role(&self) -> Role {
        Role {
            id: new_id(),
            name: self.name.to_string(),
            provider_id: None,
            provider_ids: vec![],
            system_prompt_override: Some(self.system_prompt.to_string()),
            tool_allowlist: vec![],
            required_capabilities: self.required_capabilities.to_vec(),
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({
                "description": self.description,
                "domain_tags": self.domain_tags,
                "preset": true,
            }),
            builtin: true,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: now_ms(),
            updated_at: now_ms(),
        }
    }
}

/// The built-in catalog (11 presets; referenced in AGENTS.md K6 follow-ups).
pub const PRESET_ROLES: &[PresetRole] = &[
    PresetRole {
        name: "Coder",
        description: "Writes production-quality code: implements features, fixes bugs, refactors.",
        system_prompt: "You are an expert software engineer. Write clean, idiomatic, secure code \
with minimal diff. State assumptions briefly, then give the complete code. Prefer the project's \
existing conventions, add tests for behavior changes, and never invent APIs you cannot verify.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["coding", "dev"],
    },
    PresetRole {
        name: "Code Reviewer",
        description: "Reviews diffs for correctness, security, performance and style.",
        system_prompt: "You are a rigorous code reviewer. Examine the change for correctness, \
security vulnerabilities, performance traps, error handling and readability. Output findings \
ordered by severity (blocker / major / minor / nit), each with file, line, why it matters and a \
concrete suggested fix. End with an overall verdict: approve, request changes, or block.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["coding", "quality"],
    },
    PresetRole {
        name: "Planner",
        description:
            "Decomposes goals into ordered, executable task plans with acceptance criteria.",
        system_prompt: "You are a technical planner. Decompose the goal into a minimal ordered \
list of executable steps. For each step give: title, why it is needed, inputs/outputs, risks and \
a verifiable acceptance criterion. Flag dependencies and the smallest safe first step. Do not \
implement anything yourself.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["planning", "management"],
    },
    PresetRole {
        name: "Docs Writer",
        description: "Writes and maintains user-facing documentation, guides and changelogs.",
        system_prompt: "You are a technical writer. Produce clear, structured documentation for \
the stated audience. Lead with purpose, use short sections, concrete examples and copy-pasteable \
commands. Match the project's existing tone and terminology; avoid marketing fluff.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["docs", "writing"],
    },
    PresetRole {
        name: "Test Engineer",
        description: "Designs and writes test plans and automated tests, hunts edge cases.",
        system_prompt: "You are a test engineer. Derive the behavior contract, then enumerate \
test cases: happy paths, boundary values, error paths and concurrency/race risks. Write the \
automated tests in the project's framework with deterministic fixtures. Every reported bug needs \
a failing test that reproduces it.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["testing", "quality"],
    },
    PresetRole {
        name: "Data Analyst",
        description: "Explores datasets, writes analysis queries and explains findings.",
        system_prompt: "You are a data analyst. Clarify the question, inspect schema and data \
quality, then run exploratory analysis (SQL/pandas as appropriate). Report findings with the \
exact query/code used, key numbers, charts where useful, and clearly separated facts vs. \
interpretation vs. caveats.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["data", "analysis"],
    },
    PresetRole {
        name: "Translator",
        description: "Translates and localizes text between languages, preserving tone and format.",
        system_prompt: "You are a professional translator and localization specialist. Translate \
the source text faithfully, preserving meaning, register, formatting and inline markup. Adapt \
idioms and locale conventions (dates, units, names) to the target audience. Output only the \
translation unless clarification is impossible to avoid.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["i18n", "writing"],
    },
    PresetRole {
        name: "Ops Rescuer",
        description: "Diagnoses production incidents, reads logs, proposes safe remediation.",
        system_prompt: "You are an SRE on incident duty. Triage the symptom: gather evidence from \
logs/metrics/traces, form ranked hypotheses, and propose the least-risky remediation with an \
explicit blast-radius note and a rollback plan. Never run destructive commands without approval; \
communicate in short incident-timeline updates.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["ops", "sre"],
    },
    PresetRole {
        name: "Researcher",
        description: "Investigates questions, compares sources, produces cited briefings.",
        system_prompt: "You are a research analyst. Scope the question, gather and compare \
sources, and synthesize a briefing: key findings up front, evidence with citations, open \
questions and confidence levels. Distinguish verified facts from inference; never fabricate \
sources.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["research", "analysis"],
    },
    PresetRole {
        name: "Creative Writer",
        description: "Drafts stories, copy and naming ideas with a distinct voice.",
        system_prompt: "You are a versatile creative writer. Match the requested voice, format \
and length; favor concrete imagery over abstraction; vary sentence rhythm. Offer 2-3 distinct \
variants when direction is open-ended, and accept revision notes without defensiveness.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["writing", "creative"],
    },
    PresetRole {
        name: "Role Director",
        description: "Meta role: turns a plain-language need into a complete, ready-to-run Role \
(name, prompt, required capabilities). Also the engine behind the Role Director dialog.",
        system_prompt:
            "You are the Role Director of an agent harness. When given a plain-language \
description of a desired assistant, design one executable Role for it: a concise name, a one-line \
description, a high-quality system prompt encoding expertise, style and guardrails, and the \
modality capabilities it requires (reasoning/image/voice/video). Respond with ONLY the JSON role \
object in the requested schema.",
        required_capabilities: &[Capability::Reasoning],
        domain_tags: &["meta", "planning"],
    },
];

/// Outcome counts of one seeding pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedReport {
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Idempotently seeds [`PRESET_ROLES`] into `roles` (see module docs for the
/// insert / refresh / skip rules). Runs migrations first so it can be called
/// on a fresh database at boot.
pub fn seed_builtin_roles(conn: &mut Connection) -> Result<SeedReport, StoreError> {
    migrations::run(conn)?;
    let tx = conn.transaction()?;
    let report = seed_in_tx(&tx)?;
    tx.commit()?;
    Ok(report)
}

fn seed_in_tx(tx: &rusqlite::Transaction<'_>) -> Result<SeedReport, StoreError> {
    let mut report = SeedReport {
        inserted: 0,
        updated: 0,
        skipped: 0,
    };
    for preset in PRESET_ROLES {
        let existing = repos::roles::list(tx)?
            .into_iter()
            .find(|r| r.name == preset.name);
        match existing {
            None => {
                repos::roles::insert(tx, &preset.to_role())?;
                report.inserted += 1;
            }
            Some(mut row) if row.builtin => {
                let fresh = preset.to_role();
                row.system_prompt_override = fresh.system_prompt_override;
                row.required_capabilities = fresh.required_capabilities;
                // Keep user-set provider binding / temperature / params
                // extras; only refresh the preset-owned description+tags.
                if let Some(obj) = row.params.as_object_mut() {
                    if let Some(fresh_obj) = fresh.params.as_object() {
                        for (key, value) in fresh_obj {
                            obj.insert(key.clone(), value.clone());
                        }
                    }
                }
                row.updated_at = now_ms();
                repos::roles::update(tx, &row)?;
                report.updated += 1;
            }
            Some(_) => report.skipped += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Db;

    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("t.db")).unwrap();
        migrations::run(&conn).unwrap();
        (dir, conn)
    }

    #[test]
    fn catalog_covers_required_domains() {
        assert!(PRESET_ROLES.len() >= 8 && PRESET_ROLES.len() <= 12);
        let names: Vec<&str> = PRESET_ROLES.iter().map(|p| p.name).collect();
        for required in [
            "Coder",
            "Code Reviewer",
            "Planner",
            "Docs Writer",
            "Test Engineer",
            "Data Analyst",
            "Translator",
            "Ops Rescuer",
            "Researcher",
            "Creative Writer",
            "Role Director",
        ] {
            assert!(names.contains(&required), "missing preset {required}");
        }
        for preset in PRESET_ROLES {
            assert!(!preset.system_prompt.trim().is_empty(), "{}", preset.name);
            assert!(!preset.required_capabilities.is_empty(), "{}", preset.name);
        }
    }

    #[test]
    fn seeding_is_idempotent_and_refreshes_builtin_rows() {
        let (_dir, mut conn) = db();
        let first = seed_builtin_roles(&mut conn).unwrap();
        assert_eq!(first.inserted, PRESET_ROLES.len());
        assert_eq!((first.updated, first.skipped), (0, 0));

        // Second pass: pure refresh, no duplicates.
        let second = seed_builtin_roles(&mut conn).unwrap();
        assert_eq!(
            (second.inserted, second.updated, second.skipped),
            (0, PRESET_ROLES.len(), 0)
        );
        assert_eq!(
            repos::roles::count(&conn).unwrap() as usize,
            PRESET_ROLES.len() + 5
        );

        // Builtin refresh preserves a user-set provider binding.
        let mut coder = repos::roles::list(&conn)
            .unwrap()
            .into_iter()
            .find(|r| r.name == "Coder")
            .unwrap();
        coder.system_prompt_override = Some("custom".into());
        coder.temperature = Some(0.3);
        repos::roles::update(&conn, &coder).unwrap();
        seed_builtin_roles(&mut conn).unwrap();
        let coder = repos::roles::list(&conn)
            .unwrap()
            .into_iter()
            .find(|r| r.name == "Coder")
            .unwrap();
        assert_eq!(coder.temperature, Some(0.3), "user params survive refresh");
        assert!(coder.builtin);
    }

    #[test]
    fn user_role_with_preset_name_is_never_touched() {
        let (_dir, mut conn) = db();
        let user_role = Role {
            id: new_id(),
            name: "Planner".into(),
            provider_id: None,
            provider_ids: vec![],
            system_prompt_override: Some("my own planner".into()),
            tool_allowlist: vec![],
            required_capabilities: vec![],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        repos::roles::insert(&conn, &user_role).unwrap();

        let report = seed_builtin_roles(&mut conn).unwrap();
        assert_eq!(report.skipped, 1);
        let row = repos::roles::list(&conn)
            .unwrap()
            .into_iter()
            .find(|r| r.name == "Planner")
            .unwrap();
        assert_eq!(
            row.system_prompt_override.as_deref(),
            Some("my own planner")
        );
        assert!(!row.builtin);
    }

    #[test]
    fn seed_runs_migrations_on_a_fresh_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh.db");
        let mut conn = Db::open(path.to_string_lossy().as_ref()).unwrap().0;
        let report = seed_builtin_roles(&mut conn).unwrap();
        assert_eq!(report.inserted, PRESET_ROLES.len());
    }
}
