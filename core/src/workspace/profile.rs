use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A saved "workspace" -- a root folder a coding/editing tool remembers
/// across launches (recent list), ported from the host's
/// `src-tauri/src/workspace/profile.rs`.
///
/// `connection_id`/`last_sftp_*` support remote (SSH/Agent) workspaces. This
/// crate stays dependency-light (no `roc_desk-ssh` dependency), so it only
/// stores the connection id as an opaque `Uuid` and never resolves it to an
/// actual connection/`FileOps` itself -- that resolution, and the associated
/// remote-metadata-file bookkeeping `open_local` does for local folders, is
/// the caller's job (see `roc_desk-workspace`'s own wrapper around
/// `WorkspaceManager`, which does have access to `roc_desk-ssh`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceProfile {
    pub id: Uuid,
    pub kind: WorkspaceKind,
    /// Local absolute path, or a remote host's absolute path.
    pub root_path: String,
    /// Required when `kind == Remote`; the connection profile this workspace
    /// is opened through (owned by `roc_desk-ssh`, not this crate).
    pub connection_id: Option<Uuid>,
    pub display_name: String,
    pub last_opened_at: Option<String>,
    /// Last-remembered pair of directories the SFTP/Agent dual-pane browser
    /// was showing for this workspace -- `None` until first opened.
    pub last_sftp_local_path: Option<String>,
    pub last_sftp_remote_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    Local,
    Remote,
}

impl WorkspaceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspaceKind::Local => "local",
            WorkspaceKind::Remote => "remote",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "remote" => WorkspaceKind::Remote,
            _ => WorkspaceKind::Local,
        }
    }
}
