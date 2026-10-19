//! Preserve the desktop search ordering while keeping its SQLite guard inside Core.
use super::clipboard_repo::SqliteClipboardRepository;
use crate::{
    domain::models::ClipboardEntry,
    services::search::{build_search_query, register_regexp, SearchMode, SearchPlan},
};
impl SqliteClipboardRepository {
    pub(crate) fn search_desktop(
        &self,
        mode: SearchMode,
        limit: u32,
    ) -> Result<Vec<ClipboardEntry>, String> {
        let mut plan = build_search_query(&mode, limit.clamp(1, 500));
        if let SearchPlan::Sql { sql, params } = &mut plan {
            if sql.contains("?1*") {
                *sql = sql.replacen("?1*", "?1", 1);
                if let Some(first) = params.first_mut() {
                    if !first.ends_with('*') {
                        first.push('*');
                    }
                }
            }
        }
        let SearchPlan::Sql { sql, params } = plan else {
            return Ok(Vec::new());
        };
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        if matches!(mode, SearchMode::Regex { .. }) {
            register_regexp(&conn).map_err(|error| error.to_string())?;
        }
        let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
        let ids = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        ids.into_iter()
            .filter_map(|id| self.get_entry_by_id_with_conn(&conn, id).transpose())
            .collect()
    }
}
