use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A saved "workspace" -- a root folder a coding/editing tool remembers
/// across launches (recent list), ported from the host's
/// `src-tauri/src/workspace/profile.rs`.
///
/// Unlike the host's original type, this crate-level port is **local-only**:
/// the host's `connection_id`/remote-path fields existed to support SSH/Agent
/// remote workspaces, which depend on `roc_desk-ssh`'s connection pools --
/// that tool hasn't been split out yet, so remote workspace support stays a
/// host-only concept for now (see `docs/MULTI_REPO_SPLIT_PROGRESS.md` in the
/// host repository, "编程工作区" section). `kind` is kept (rather than
/// dropped) so a future remote variant can be added without a breaking
/// rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceProfile {
    pub id: Uuid,
    pub kind: WorkspaceKind,
    /// Local absolute path.
    pub root_path: String,
    pub display_name: String,
    pub last_opened_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    Local,
}

impl WorkspaceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspaceKind::Local => "local",
        }
    }

    pub fn from_str(_s: &str) -> Self {
        WorkspaceKind::Local
    }
}
