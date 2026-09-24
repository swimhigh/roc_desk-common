use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use crate::db::DbPool;
use crate::error::AppError;

use super::profile::{WorkspaceKind, WorkspaceProfile};

/// SQLite-backed "recent workspaces" list, ported from the host's
/// `db::repo::workspace_repo::WorkspaceRepo`.
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
                connection_id TEXT,
                display_name TEXT NOT NULL,
                last_opened_at TEXT,
                last_sftp_local_path TEXT,
                last_sftp_remote_path TEXT,
                created_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Deliberately does not touch `last_sftp_local_path`/`last_sftp_remote_path`
    /// on conflict -- `upsert` runs on every workspace open (`open_local`/
    /// `open_remote`), and the `WorkspaceProfile` literal built there always
    /// carries `None` for those two fields; updating them here would wipe the
    /// remembered directories on every reopen. They're only ever changed by
    /// `update_last_sftp_paths`.
    pub fn upsert(&self, profile: &WorkspaceProfile) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO workspaces (id, kind, root_path, connection_id, display_name, last_opened_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                root_path = excluded.root_path,
                display_name = excluded.display_name,
                last_opened_at = excluded.last_opened_at",
            params![
                profile.id.to_string(),
                profile.kind.as_str(),
                profile.root_path,
                profile.connection_id.map(|id| id.to_string()),
                profile.display_name,
                profile.last_opened_at,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn touch_last_opened(&self, id: Uuid) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE workspaces SET last_opened_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), id.to_string()],
        )?;
        Ok(())
    }

    /// The SFTP/Agent dual-pane browser calls this on every navigation --
    /// both arguments are the *current* values, not a delta; the caller is
    /// responsible for always passing the full pair.
    pub fn update_last_sftp_paths(
        &self,
        id: Uuid,
        local_path: &str,
        remote_path: &str,
    ) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE workspaces SET last_sftp_local_path = ?1, last_sftp_remote_path = ?2 WHERE id = ?3",
            params![local_path, remote_path, id.to_string()],
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
                "SELECT id, kind, root_path, connection_id, display_name, last_opened_at, last_sftp_local_path, last_sftp_remote_path
                 FROM workspaces WHERE kind = 'local' AND root_path = ?1",
                params![root_path],
                Self::map_row,
            )
            .optional()?;
        Ok(result)
    }

    pub fn find_by_remote(
        &self,
        connection_id: Uuid,
        root_path: &str,
    ) -> Result<Option<WorkspaceProfile>, AppError> {
        let conn = self.pool.get()?;
        let result = conn
            .query_row(
                "SELECT id, kind, root_path, connection_id, display_name, last_opened_at, last_sftp_local_path, last_sftp_remote_path
                 FROM workspaces WHERE kind = 'remote' AND connection_id = ?1 AND root_path = ?2",
                params![connection_id.to_string(), root_path],
                Self::map_row,
            )
            .optional()?;
        Ok(result)
    }

    pub fn find_by_id(&self, id: Uuid) -> Result<Option<WorkspaceProfile>, AppError> {
        let conn = self.pool.get()?;
        let result = conn
            .query_row(
                "SELECT id, kind, root_path, connection_id, display_name, last_opened_at, last_sftp_local_path, last_sftp_remote_path
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
            "SELECT id, kind, root_path, connection_id, display_name, last_opened_at, last_sftp_local_path, last_sftp_remote_path
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
        let connection_id: Option<String> = row.get(3)?;
        Ok(WorkspaceProfile {
            id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
            kind: WorkspaceKind::from_str(&kind),
            root_path: row.get(2)?,
            connection_id: connection_id.and_then(|s| Uuid::parse_str(&s).ok()),
            display_name: row.get(4)?,
            last_opened_at: row.get(5)?,
            last_sftp_local_path: row.get(6)?,
            last_sftp_remote_path: row.get(7)?,
        })
    }
}
