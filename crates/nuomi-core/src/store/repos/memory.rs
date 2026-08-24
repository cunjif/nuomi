//! Long-term memory repository (keyword / structured retrieval; vector later).

use rusqlite::{params, Connection};

use crate::domain::MemoryEntry;
use crate::store::StoreError;

pub fn insert(conn: &Connection, m: &MemoryEntry) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO memory_entries
         (id, content, source_session_id, tags, kind, user_profile, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            m.id,
            m.content,
            m.source_session_id,
            serde_json::to_string(&m.tags)?,
            m.kind,
            m.user_profile as i64,
            m.created_at,
            m.updated_at
        ],
    )?;
    Ok(())
}

/// Keyword search across content with optional tag filter.
/// Matches are ranked by recency (newest first).
pub fn search(
    conn: &Connection,
    keyword: Option<&str>,
    tag: Option<&str>,
    limit: u32,
) -> Result<Vec<MemoryEntry>, StoreError> {
    let mut sql = String::from(
        "SELECT id, content, source_session_id, tags, kind, user_profile, created_at, updated_at
         FROM memory_entries WHERE 1=1",
    );
    if keyword.is_some() {
        sql.push_str(" AND content LIKE '%' || ?1 || '%'");
    }
    if tag.is_some() {
        sql.push_str(if keyword.is_some() {
            " AND tags LIKE '%' || ?2 || '%'"
        } else {
            " AND tags LIKE '%' || ?1 || '%'"
        });
    }
    sql.push_str(match (keyword.is_some(), tag.is_some()) {
        (true, true) => " ORDER BY created_at DESC LIMIT ?3",
        (true, false) | (false, true) => " ORDER BY created_at DESC LIMIT ?2",
        (false, false) => " ORDER BY created_at DESC LIMIT ?1",
    });

    let kw = keyword.unwrap_or_default();
    let tg = tag.unwrap_or_default();
    let mut stmt = conn.prepare(&sql)?;
    let map_rows =
        |row: &rusqlite::Row<'_>| -> rusqlite::Result<MemoryEntry> { row_to_memory(row) };
    let rows = match (keyword, tag) {
        (Some(_), Some(_)) => stmt.query_map(params![kw, tg, limit], map_rows)?,
        (Some(_), None) => stmt.query_map(params![kw, limit], map_rows)?,
        (None, Some(_)) => stmt.query_map(params![tg, limit], map_rows)?,
        (None, None) => stmt.query_map(params![limit], map_rows)?,
    };
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// All user-profile facts (always injected by evolution).
pub fn list_user_profile(conn: &Connection) -> Result<Vec<MemoryEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, content, source_session_id, tags, kind, user_profile, created_at, updated_at
         FROM memory_entries WHERE user_profile = 1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_memory)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let tags: String = row.get(3)?;
    Ok(MemoryEntry {
        id: row.get(0)?,
        content: row.get(1)?,
        source_session_id: row.get(2)?,
        tags: crate::store::json_col(0, &tags)?,
        kind: row.get(4)?,
        user_profile: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn entry(id: &str, content: &str, tags: &[&str], profile: bool, at: i64) -> MemoryEntry {
        MemoryEntry {
            id: id.into(),
            content: content.into(),
            source_session_id: None,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            kind: "note".into(),
            user_profile: profile,
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn keyword_search_matches_content_only() {
        let conn = db();
        insert(&conn, &entry("m1", "loves rust", &["lang"], false, 1)).unwrap();
        insert(&conn, &entry("m2", "prefers python", &["lang"], false, 2)).unwrap();
        insert(
            &conn,
            &entry("m3", "rust tags mention", &["python"], false, 3),
        )
        .unwrap();
        let hits = search(&conn, Some("rust"), None, 10).unwrap();
        let ids: Vec<&str> = hits.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["m3", "m1"]); // recency order
    }

    #[test]
    fn tag_filter_and_profile_listing() {
        let conn = db();
        insert(&conn, &entry("a", "x", &["work"], false, 1)).unwrap();
        insert(&conn, &entry("b", "name is james", &["me"], true, 2)).unwrap();
        insert(&conn, &entry("c", "y", &["work"], false, 3)).unwrap();
        assert_eq!(search(&conn, None, Some("work"), 10).unwrap().len(), 2);
        let profile = list_user_profile(&conn).unwrap();
        assert_eq!(profile.len(), 1);
        assert_eq!(profile[0].id, "b");
        assert!(profile[0].user_profile);
    }

    #[test]
    fn empty_search_returns_all_recency_first() {
        let conn = db();
        insert(&conn, &entry("old", "x", &[], false, 1)).unwrap();
        insert(&conn, &entry("new", "y", &[], false, 2)).unwrap();
        let all = search(&conn, None, None, 10).unwrap();
        assert_eq!(
            all.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "old"]
        );
    }
}
