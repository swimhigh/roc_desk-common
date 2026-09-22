use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use crate::db::DbPool;
use crate::error::AppError;

use super::profile::{WorkspaceKind, WorkspaceProfile};

/// SQLite-backed "recent workspaces" list, ported from the host's
/// `db::repo::workspace_repo::WorkspaceRepo` with the remote-workspace
/// columns (`connection_id`, `last_sftp_*`) dropped -- this crate only
/// supports local workspaces (see `profile.rs`).
pub struct WorkspaceRepo {
    pool: DbPool,
}

impl WorkspaceRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Creates the `workspaces` table if it doesn't exist yet. Call once
    /// during startup, after `roc_desk_core::db::create_pool`.
    pub fn ensure_schema(&self) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                root_path TEXT NOT NULL,
                display_name TEXT NOT NULL,
                last_opened_at TEXT,
                created_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    pub fn upsert(&self, profile: &WorkspaceProfile) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO workspaces (id, kind, root_path, display_name, last_opened_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                root_path = excluded.root_path,
                display_name = excluded.display_name,
                last_opened_at = excluded.last_opened_at",
            params![
                profile.id.to_string(),
                profile.kind.as_str(),
                profile.root_path,
                profile.display_name,
                profile.last_opened_at,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn find_by_local_path(
        &self,
        root_path: &str,
    ) -> Result<Option<WorkspaceProfile>, AppError> {
        let conn = self.pool.get()?;
        let result = conn
            .query_row(
                "SELECT id, kind, root_path, display_name, last_opened_at
                 FROM workspaces WHERE kind = 'local' AND root_path = ?1",
                params![root_path],
                Self::map_row,
            )
            .optional()?;
        Ok(result)
    }

    pub fn find_by_id(&self, id: Uuid) -> Result<Option<WorkspaceProfile>, AppError> {
        let conn = self.pool.get()?;
        let result = conn
            .query_row(
                "SELECT id, kind, root_path, display_name, last_opened_at
                 FROM workspaces WHERE id = ?1",
                params![id.to_string()],
                Self::map_row,
            )
            .optional()?;
        Ok(result)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<WorkspaceProfile>, AppError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, kind, root_path, display_name, last_opened_at
             FROM workspaces
             ORDER BY last_opened_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn remove(&self, id: Uuid) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM workspaces WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    fn map_row(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceProfile> {
        let id: String = row.get(0)?;
        let kind: String = row.get(1)?;
        Ok(WorkspaceProfile {
            id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
            kind: WorkspaceKind::from_str(&kind),
            root_path: row.get(2)?,
            display_name: row.get(3)?,
            last_opened_at: row.get(4)?,
        })
    }
}
