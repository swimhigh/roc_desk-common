use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which machine/connection an AI agent session's tools (file ops, `git`,
/// `run_command`) act against. Shared by any tool that stages AI-proposed
/// file edits via [`super::ChangeStore`] -- a purely local tool (e.g. the
/// SQL workbench's AI assist panel) only ever uses `Local`; a tool with
/// remote workspaces (the coding agent) uses all three variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum CodingTarget {
    Local,
    Remote {
        connection_id: Uuid,
        host_label: String,
    },
    /// Remote Windows Agent workspace: `run_command`/`search_files` go
    /// through the Agent protocol instead of SSH `exec`/`grep`, and command
    /// syntax is native Windows (`cmd.exe /C` + an argument array, not "a
    /// hand-assembled POSIX shell string").
    Agent {
        connection_id: Uuid,
        host_label: String,
    },
}
