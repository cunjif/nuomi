//! Workspace identity naming: the single source of truth for directory
//! display names, same-name collision disambiguation, and the
//! workspace-level label join (ADR-0021, task 6.4).
//!
//! Everything here is pure derivation — results are never persisted. The
//! directory set is the only source of truth; a stored label would go stale
//! the moment a directory is added or removed (spec 5.6.1-4). The
//! computation is plain string work over a handful of rows, comfortably
//! within the ≤50ms budget, so it can run on every read without blocking
//! the UI.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::store::repos::work_directories::WorkDirectoryEntry;

/// Maximum directory names shown in the truncated label (spec 5.6.1-2a).
const MAX_LABEL_PARTS: usize = 4;
/// Suffix appended to the truncated label; the tooltip shows `label_full`.
const TRUNCATION_SUFFIX: &str = "...";

/// A derived workspace label: the compact form for space-constrained
/// surfaces (bottom bar, switch list) and the full form for tooltips.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceLabel {
    /// Compact label: up to [`MAX_LABEL_PARTS`] directory names joined with
    /// `-`, plus `-...` when truncated. Empty when the workspace has no
    /// usable directories (e.g. auto-closed after removing the last one).
    pub label: String,
    /// Full label: every directory name joined with `-`.
    pub label_full: String,
}

/// Splits an absolute path into `(display_name, prefix)` where `display_name`
/// is the final path segment (cross-platform separators, trailing slashes
/// trimmed) and `prefix` is everything before it. Returns `None` when the
/// path has no usable final segment (empty or separators only).
pub fn split_display_name(abs_path: &str) -> Option<(String, String)> {
    let trimmed = abs_path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rfind(['/', '\\']) {
        Some(pos) => Some((trimmed[pos + 1..].to_string(), trimmed[..pos].to_string())),
        None => Some((trimmed.to_string(), String::new())),
    }
}

/// Returns the final segment of a path prefix, e.g. `C:\proj\a` → `a`.
pub fn parent_segment(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    Some(match trimmed.rfind(['/', '\\']) {
        Some(pos) => trimmed[pos + 1..].to_string(),
        None => trimmed.to_string(),
    })
}

/// Derives the display name for a directory being added to a workspace whose
/// existing display names are `existing`: the final path segment, with the
/// same two-level collision disambiguation the boot-time correction uses
/// (`name (parent)` then `name (full-prefix)`), so no two directories in one
/// workspace ever share a display name. Only the newcomer is renamed —
/// existing names stay stable. Returns `None` when the path has no usable
/// final segment.
pub fn derive_display_name(abs_path: &str, existing: &[String]) -> Option<String> {
    let (display, prefix) = split_display_name(abs_path)?;
    if !existing.iter().any(|n| n == &display) {
        return Some(display);
    }
    let level1 = match parent_segment(&prefix) {
        Some(parent) => format!("{} ({})", display, parent),
        None => display.clone(),
    };
    if !existing.iter().any(|n| n == &level1) {
        return Some(level1);
    }
    if !prefix.is_empty() {
        return Some(format!("{} ({})", display, prefix));
    }
    // Unreachable in practice: a colliding bare display with an empty prefix
    // implies an identical abs_path, which the UNIQUE constraint forbids.
    Some(level1)
}

/// Resolves same-display-name collisions within one workspace. `pairs`
/// carries `(display_name, path_prefix)` per directory. Two levels: first
/// `name (parent-segment)`; if still ambiguous, `name (full-prefix)` —
/// the full prefix is guaranteed unique because `abs_path` is unique within
/// the workspace and the trailing segment is identical across the group.
pub fn disambiguate_names(pairs: &[(String, String)]) -> Vec<String> {
    let mut names: Vec<String> = pairs.iter().map(|(display, _)| display.clone()).collect();

    // Collect indices per display name to find collision groups.
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (idx, (display, _)) in pairs.iter().enumerate() {
        groups.entry(display.as_str()).or_default().push(idx);
    }
    for (_, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        // Level 1: append the parent segment, e.g. `foo (a)`. A row whose
        // prefix is empty keeps the bare name (cannot collide within the
        // group: identical bare display + empty prefix would mean identical
        // abs_path, which the UNIQUE constraint forbids).
        let level1: Vec<String> = indices
            .iter()
            .map(|&i| match parent_segment(&pairs[i].1) {
                Some(parent) => format!("{} ({})", pairs[i].0, parent),
                None => pairs[i].0.clone(),
            })
            .collect();
        let unique = {
            let mut seen = BTreeSet::new();
            level1.iter().all(|s| seen.insert(s.clone()))
        };
        if unique {
            for (&slot, name) in indices.iter().zip(level1) {
                names[slot] = name;
            }
        } else {
            // Level 2: append the full prefix — unique by construction.
            for &slot in &indices {
                let (display, prefix) = &pairs[slot];
                if !prefix.is_empty() {
                    names[slot] = format!("{} ({})", display, prefix);
                }
            }
        }
    }
    names
}

/// Derives the workspace label from its directories (spec 5.6.1): each
/// directory's stored `display_name`, ordered by `added_at`, joined with
/// `-`; more than [`MAX_LABEL_PARTS`] directories truncate the compact label
/// to the first four plus `-...`. Same-name directories are disambiguated by
/// appending the parent directory name, so no two parts of a label are
/// identical. Directories whose path cannot be parsed are skipped with a
/// warning (spec 5.6.1-5a) — a broken row never blocks the label.
pub fn derive_label(directories: &[WorkDirectoryEntry]) -> WorkspaceLabel {
    // Stable sort by added_at: repo reads arrive pre-sorted, but the
    // derivation stays correct for any caller-supplied order. Ties keep
    // input order.
    let mut ordered: Vec<&WorkDirectoryEntry> = directories.iter().collect();
    ordered.sort_by_key(|d| d.added_at);

    let mut pairs: Vec<(String, String)> = Vec::with_capacity(ordered.len());
    for dir in ordered {
        match split_display_name(&dir.abs_path) {
            Some((_, prefix)) => pairs.push((dir.display_name.clone(), prefix)),
            None => tracing::warn!(
                directory_id = %dir.id,
                abs_path = %dir.abs_path,
                workspace_id = %dir.workspace_id,
                "workspace label: skipping directory with unparseable path"
            ),
        }
    }

    let names = disambiguate_names(&pairs);
    let label_full = names.join("-");
    let label = if names.len() > MAX_LABEL_PARTS {
        format!(
            "{}-{}",
            names[..MAX_LABEL_PARTS].join("-"),
            TRUNCATION_SUFFIX
        )
    } else {
        label_full.clone()
    };
    WorkspaceLabel { label, label_full }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(abs_path: &str, display_name: &str, added_at: i64) -> WorkDirectoryEntry {
        WorkDirectoryEntry {
            id: format!("d{added_at}"),
            workspace_id: "ws".into(),
            abs_path: abs_path.into(),
            display_name: display_name.into(),
            mounted: true,
            added_at,
        }
    }

    // ---- split_display_name / parent_segment

    #[test]
    fn split_display_name_handles_separators_and_trailing_slashes() {
        assert_eq!(
            split_display_name("C:\\proj\\alpha").unwrap(),
            ("alpha".into(), "C:\\proj".into())
        );
        assert_eq!(
            split_display_name("/home/user/beta/").unwrap(),
            ("beta".into(), "/home/user".into())
        );
        // Mixed separators: the last one wins.
        assert_eq!(
            split_display_name("C:/proj\\gamma\\").unwrap(),
            ("gamma".into(), "C:/proj".into())
        );
        // No separator: whole string is the name, prefix empty.
        assert_eq!(
            split_display_name("rootless").unwrap(),
            ("rootless".into(), String::new())
        );
        assert!(split_display_name("").is_none());
        assert!(split_display_name("\\\\\\/").is_none());
    }

    #[test]
    fn parent_segment_returns_last_prefix_component() {
        assert_eq!(parent_segment("C:\\proj\\a").unwrap(), "a");
        assert_eq!(parent_segment("/home/user").unwrap(), "user");
        assert_eq!(parent_segment("solo").unwrap(), "solo");
        assert_eq!(parent_segment("C:\\").unwrap(), "C:");
        assert!(parent_segment("").is_none());
        assert!(parent_segment("/").is_none());
        assert!(parent_segment("\\\\").is_none());
    }

    // ---- derive_display_name (incremental add path, consumed by registry v2)

    #[test]
    fn derive_display_name_bare_when_no_collision() {
        assert_eq!(derive_display_name("C:\\a\\foo", &[]).unwrap(), "foo");
        assert_eq!(
            derive_display_name("C:\\a\\foo", &["bar".into(), "baz".into()]).unwrap(),
            "foo"
        );
        // Trailing slash and mixed separators.
        assert_eq!(
            derive_display_name("/home/user/beta/", &["foo".into()]).unwrap(),
            "beta"
        );
    }

    #[test]
    fn derive_display_name_escalates_through_two_levels() {
        // Level 1: parent segment suffix.
        assert_eq!(
            derive_display_name("C:\\b\\foo", &["foo".into()]).unwrap(),
            "foo (b)"
        );
        // Level 2: parent collides with an existing level-1 name.
        assert_eq!(
            derive_display_name("C:\\a\\foo", &["foo".into(), "foo (a)".into()]).unwrap(),
            "foo (C:\\a)"
        );
    }

    #[test]
    fn derive_display_name_unparseable_returns_none() {
        assert!(derive_display_name("", &[]).is_none());
        assert!(derive_display_name("\\\\\\", &[]).is_none());
    }

    // ---- disambiguate_names

    #[test]
    fn disambiguate_names_appends_parent_on_collision() {
        let pairs = vec![
            ("foo".into(), "C:\\a".into()),
            ("bar".into(), "C:\\z".into()),
            ("foo".into(), "C:\\b".into()),
        ];
        assert_eq!(
            disambiguate_names(&pairs),
            vec![
                "foo (a)".to_string(),
                "bar".to_string(),
                "foo (b)".to_string()
            ]
        );
    }

    #[test]
    fn disambiguate_names_escalates_to_full_prefix_when_parents_collide() {
        let pairs = vec![
            ("foo".into(), "C:\\a\\x".into()),
            ("foo".into(), "C:\\b\\x".into()),
        ];
        assert_eq!(
            disambiguate_names(&pairs),
            vec!["foo (C:\\a\\x)".to_string(), "foo (C:\\b\\x)".to_string()]
        );
    }

    // ---- derive_label (spec 5.6.1: order, join, truncate, disambiguate, skip)

    #[test]
    fn derive_label_joins_display_names_in_added_order() {
        // Input deliberately out of order: added_at decides, not input order.
        let dirs = vec![
            dir("C:\\proj\\gamma", "gamma", 30),
            dir("C:\\proj\\alpha", "alpha", 10),
            dir("C:\\proj\\beta", "beta", 20),
        ];
        let label = derive_label(&dirs);
        assert_eq!(label.label, "alpha-beta-gamma");
        assert_eq!(label.label_full, "alpha-beta-gamma");
    }

    #[test]
    fn derive_label_truncates_beyond_four_directories() {
        let dirs = vec![
            dir("C:\\p\\one", "one", 1),
            dir("C:\\p\\two", "two", 2),
            dir("C:\\p\\three", "three", 3),
            dir("C:\\p\\four", "four", 4),
            dir("C:\\p\\five", "five", 5),
        ];
        let label = derive_label(&dirs);
        assert_eq!(label.label, "one-two-three-four-...");
        assert_eq!(label.label_full, "one-two-three-four-five");
        // Exactly four: no truncation.
        let four: Vec<_> = dirs.into_iter().take(4).collect();
        let label = derive_label(&four);
        assert_eq!(label.label, "one-two-three-four");
        assert_eq!(label.label_full, "one-two-three-four");
    }

    #[test]
    fn derive_label_disambiguates_duplicate_display_names() {
        let dirs = vec![
            dir("C:\\a\\foo", "foo", 1),
            dir("C:\\b\\foo", "foo", 2),
            dir("C:\\c\\bar", "bar", 3),
        ];
        let label = derive_label(&dirs);
        assert_eq!(label.label, "foo (a)-foo (b)-bar");
        assert_eq!(label.label_full, "foo (a)-foo (b)-bar");
    }

    #[test]
    fn derive_label_skips_unparseable_paths() {
        let dirs = vec![
            dir("C:\\a\\ok", "ok", 1),
            dir("", "broken", 2),
            dir("\\\\\\", "also-broken", 3),
            dir("C:\\a\\fine", "fine", 4),
        ];
        let label = derive_label(&dirs);
        assert_eq!(label.label, "ok-fine");
        assert_eq!(label.label_full, "ok-fine");
    }

    #[test]
    fn derive_label_empty_input_yields_empty_label() {
        let label = derive_label(&[]);
        assert_eq!(label.label, "");
        assert_eq!(label.label_full, "");
    }
}
