use serde::{Deserialize, Serialize};

/// A persisted remote endpoint. Secrets are referenced by `credential_ref` and
/// never carried in this serializable structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credential_ref: Option<String>,
    pub kind: ConnectionKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind { Ssh, Sftp, Rdp }

impl ConnectionProfile {
    pub fn ssh(id: impl Into<String>, name: impl Into<String>, host: impl Into<String>, username: impl Into<String>) -> Self {
        Self { id: id.into(), name: name.into(), host: host.into(), port: 22, username: username.into(), credential_ref: None, kind: ConnectionKind::Ssh }
    }
}
