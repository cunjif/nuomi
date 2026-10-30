//! Preference service — B1/K8/B2 shared preference snapshot interface
//! (ADR-0025 §2.4 Slice 3).
//!
//! `user_preference_profiles` is the single source of truth for user
//! preferences. B1 writes; K8 (PromptVersion reflection) and B2 (project
//! evolution) read via this service — never direct SQL — to keep the
//! layers decoupled.
//!
//! The service also bridges legacy `memory_entries` (user_profile=1) into
//! the snapshot, so existing reflection callers work unchanged until they
//! migrate to the new interface.

use rusqlite::Connection;

use crate::store::repos::{memory, user_preference_profiles};
use crate::store::StoreError;

/// A preference snapshot for reflection input (ADR-0025 §2.4).
///
/// Combines `user_preference_profiles` (confirmed entries) with legacy
/// `memory_entries` (user_profile=1) so K8 reflection sees a unified
/// picture during the migration period.
#[derive(Debug, Clone, PartialEq)]
pub struct PreferenceSnapshot {
    /// Content strings from confirmed `user_preference_profiles` rows,
    /// ordered by confidence descending.
    pub profiles: Vec<String>,
    /// Content strings from legacy `memory_entries` (user_profile=1).
    pub legacy_memory: Vec<String>,
}

impl PreferenceSnapshot {
    /// Flattened list for `ReflectionInput::user_profile` — profiles first
    /// (higher signal), then legacy memory.
    pub fn flattened(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.profiles.len() + self.legacy_memory.len());
        out.extend(self.profiles.iter().cloned());
        out.extend(self.legacy_memory.iter().cloned());
        out
    }
}

/// The preference service. Stateless — all reads go through the connection.
pub struct PreferenceService;

impl PreferenceService {
    /// Returns a unified preference snapshot (ADR-0025 §2.4).
    ///
    /// Combines `user_preference_profiles` (confirmed, by confidence desc)
    /// with legacy `memory_entries` (user_profile=1). The flattened result
    /// is a drop-in replacement for `MemoryService::user_profile()` in
    /// reflection callers.
    pub fn snapshot(conn: &Connection) -> Result<PreferenceSnapshot, StoreError> {
        let confirmed = user_preference_profiles::list_confirmed(conn)?;
        let profiles: Vec<String> = confirmed.into_iter().map(|p| p.content).collect();

        let legacy = memory::list_user_profile(conn)?;
        let legacy_memory: Vec<String> = legacy.into_iter().map(|m| m.content).collect();

        Ok(PreferenceSnapshot {
            profiles,
            legacy_memory,
        })
    }

    /// Returns confirmed profiles with confidence ≥ `min_confidence`
    /// (ADR §5 第四轨: UX anchor check uses ≥ 0.8).
    pub fn high_confidence(
        conn: &Connection,
        min_confidence: f64,
    ) -> Result<Vec<user_preference_profiles::UserPreferenceProfile>, StoreError> {
        user_preference_profiles::list_high_confidence(conn, min_confidence)
    }

    /// Convenience: flattened preference strings for `ReflectionInput`.
    /// Equivalent to `Self::snapshot()?.flattened()`.
    pub fn snapshot_flattened(conn: &Connection) -> Result<Vec<String>, StoreError> {
        Ok(Self::snapshot(conn)?.flattened())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::new_id;
    use crate::store::migrations;
    use crate::store::repos::user_preference_profiles::{
        insert, PreferenceCategory, PreferenceStatus, UserPreferenceProfile,
    };

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn snapshot_returns_empty_on_fresh_db() {
        let conn = fresh_db();
        let snap = PreferenceService::snapshot(&conn).unwrap();
        assert!(snap.profiles.is_empty());
        assert!(snap.legacy_memory.is_empty());
        assert!(snap.flattened().is_empty());
    }

    #[test]
    fn snapshot_includes_confirmed_profiles() {
        let conn = fresh_db();
        insert(
            &conn,
            &UserPreferenceProfile {
                id: new_id(),
                category: PreferenceCategory::Convention,
                key: "rust.no_unwrap".to_string(),
                content: "Avoid unwrap() in production Rust".to_string(),
                confidence: 0.9,
                source_session_id: None,
                status: PreferenceStatus::Confirmed,
                created_at: 1000,
                updated_at: 1000,
                revoked_at: None,
            },
        )
        .unwrap();
        let snap = PreferenceService::snapshot(&conn).unwrap();
        assert_eq!(snap.profiles.len(), 1);
        assert!(snap.profiles[0].contains("unwrap"));
    }

    #[test]
    fn snapshot_excludes_revoked() {
        let conn = fresh_db();
        let id = new_id();
        insert(
            &conn,
            &UserPreferenceProfile {
                id: id.clone(),
                category: PreferenceCategory::Memory,
                key: "ui.compact".to_string(),
                content: "Prefer compact UI".to_string(),
                confidence: 0.8,
                source_session_id: None,
                status: PreferenceStatus::Confirmed,
                created_at: 1000,
                updated_at: 1000,
                revoked_at: None,
            },
        )
        .unwrap();
        user_preference_profiles::revoke(&conn, &id, 2000).unwrap();
        let snap = PreferenceService::snapshot(&conn).unwrap();
        assert!(snap.profiles.is_empty());
    }

    #[test]
    fn high_confidence_filters_by_threshold() {
        let conn = fresh_db();
        for (key, conf) in [("low", 0.3), ("mid", 0.6), ("high", 0.9)] {
            insert(
                &conn,
                &UserPreferenceProfile {
                    id: new_id(),
                    category: PreferenceCategory::Convention,
                    key: key.to_string(),
                    content: key.to_string(),
                    confidence: conf,
                    source_session_id: None,
                    status: PreferenceStatus::Confirmed,
                    created_at: 1000,
                    updated_at: 1000,
                    revoked_at: None,
                },
            )
            .unwrap();
        }
        let result = PreferenceService::high_confidence(&conn, 0.8).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "high");
    }

    #[test]
    fn snapshot_flattened_is_drop_in_for_reflection() {
        let conn = fresh_db();
        insert(
            &conn,
            &UserPreferenceProfile {
                id: new_id(),
                category: PreferenceCategory::Convention,
                key: "ts.strict".to_string(),
                content: "Always use strict TypeScript".to_string(),
                confidence: 0.95,
                source_session_id: None,
                status: PreferenceStatus::Confirmed,
                created_at: 1000,
                updated_at: 1000,
                revoked_at: None,
            },
        )
        .unwrap();
        let flattened = PreferenceService::snapshot_flattened(&conn).unwrap();
        assert_eq!(flattened.len(), 1);
        assert!(flattened[0].contains("strict TypeScript"));
        // This vec is directly usable as ReflectionInput::user_profile.
    }
}
