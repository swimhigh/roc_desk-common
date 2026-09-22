use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use roc_desk_core::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub text: String,
    pub modified_at: i64,
}

/// Minimum filesystem contract shared by tools that persist workspace files.
#[async_trait]
pub trait FileOps: Send + Sync {
    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, AppError>;
    async fn read_file(&self, path: &str) -> Result<FileContent, AppError>;
    async fn write_file(&self, path: &str, text: &str) -> Result<(), AppError>;
    async fn delete(&self, path: &str, is_dir: bool) -> Result<(), AppError>;
    async fn create_dir(&self, path: &str) -> Result<(), AppError>;
}
