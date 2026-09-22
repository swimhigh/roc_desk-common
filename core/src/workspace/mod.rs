//! Shared "workspace" concept: a remembered root folder with a recent list,
//! used by coding/editing-style tools (`roc_desk-workspace` today; the
//! resource manager could adopt it later for a "recent folders" list).
//!
//! Ported from the host's `src-tauri/src/workspace/` module. **Local-only**:
//! the host's original also supported remote (SSH/Agent) workspaces backed by
//! `roc_desk-ssh`'s connection pools, but that tool hasn't been split out of
//! the host yet, so remote workspace support is deliberately left as a
//! host-only concept for now. See `docs/MULTI_REPO_SPLIT_PROGRESS.md`
//! ("编程工作区" section) in the host repository for the full context.

mod profile;
mod repo;

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

pub use profile::{WorkspaceKind, WorkspaceProfile};
pub use repo::WorkspaceRepo;

/// A workspace's runtime handle -- not persisted, rebuilt every time the
/// workspace is opened (see CODE_DESIGN.md §3.8 in the host repository for
/// the original rationale).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    pub workspace_id: Uuid,
    pub kind: WorkspaceKind,
    pub root_path: String,
}

fn metadata_for(profile: &WorkspaceProfile) -> WorkspaceMetadata {
    WorkspaceMetadata {
        workspace_id: profile.id,
        kind: profile.kind,
        root_path: profile.root_path.clone(),
    }
}

/// Manages the SQLite-backed "recent workspaces" list plus the small
/// `.rock_desk/workspace.json` marker file dropped into each opened folder
/// (used to keep the same workspace id if the same folder gets re-opened
/// from a different machine/profile that shares the folder but not the
/// database).
pub struct WorkspaceManager {
    repo: WorkspaceRepo,
    cache_root: PathBuf,
}

impl WorkspaceManager {
    pub fn new(repo: WorkspaceRepo, cache_root: PathBuf) -> Self {
        Self { repo, cache_root }
    }

    fn write_fallback_metadata(&self, metadata: &WorkspaceMetadata) -> Result<(), AppError> {
        let dir = self.cache_root.join(metadata.workspace_id.to_string());
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("workspace.json"),
            serde_json::to_vec_pretty(metadata).map_err(|e| AppError::Internal(e.to_string()))?,
        )?;
        Ok(())
    }

    /// Opens a local folder as a workspace, reusing the same id if this path
    /// was opened before (either via the database, or via the embedded
    /// `.rock_desk/workspace.json` marker left in the folder itself).
    pub fn open_local(&self, path: &str) -> Result<WorkspaceProfile, AppError> {
        let root = Path::new(path);
        if !root.is_dir() {
            return Err(AppError::NotFound(format!("目录不存在: {path}")));
        }

        let display_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        let embedded = std::fs::read(root.join(".rock_desk").join("workspace.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<WorkspaceMetadata>(&bytes).ok())
            .filter(|meta| meta.root_path.eq_ignore_ascii_case(path));

        let profile = match self.repo.find_by_local_path(path)? {
            Some(mut existing) => {
                existing.last_opened_at = Some(Utc::now().to_rfc3339());
                existing
            }
            None => WorkspaceProfile {
                id: embedded
                    .map(|meta| meta.workspace_id)
                    .unwrap_or_else(Uuid::new_v4),
                kind: WorkspaceKind::Local,
                root_path: path.to_string(),
                display_name,
                last_opened_at: Some(Utc::now().to_rfc3339()),
            },
        };
        self.repo.upsert(&profile)?;

        let metadata = metadata_for(&profile);
        self.write_fallback_metadata(&metadata)?;
        let metadata_dir = root.join(".rock_desk");
        std::fs::create_dir_all(&metadata_dir)?;
        std::fs::write(
            metadata_dir.join("workspace.json"),
            serde_json::to_vec_pretty(&metadata).map_err(|e| AppError::Internal(e.to_string()))?,
        )?;
        Ok(profile)
    }

    /// Edits an already-saved workspace's directory in place (user feedback
    /// ported from the host: "picked the wrong folder, could only delete and
    /// re-add").
    pub fn update_path(&self, id: Uuid, new_path: &str) -> Result<WorkspaceProfile, AppError> {
        let mut profile = self
            .repo
            .find_by_id(id)?
            .ok_or_else(|| AppError::NotFound(format!("workspace not found: {id}")))?;

        let root = Path::new(new_path);
        if !root.is_dir() {
            return Err(AppError::NotFound(format!("目录不存在: {new_path}")));
        }
        if let Some(existing) = self.repo.find_by_local_path(new_path)? {
            if existing.id != id {
                return Err(AppError::Conflict(format!(
                    "该目录已经是另一个工作区：{}",
                    existing.display_name
                )));
            }
        }
        profile.display_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| new_path.to_string());
        profile.root_path = new_path.to_string();

        self.repo.upsert(&profile)?;
        let metadata = metadata_for(&profile);
        let _ = self.write_fallback_metadata(&metadata);
        Ok(profile)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<WorkspaceProfile>, AppError> {
        self.repo.list_recent(limit)
    }

    pub fn remove_from_recent(&self, id: Uuid) -> Result<(), AppError> {
        self.repo.remove(id)
    }

    pub fn ensure_schema(&self) -> Result<(), AppError> {
        self.repo.ensure_schema()
    }
}
